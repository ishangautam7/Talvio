import type { ServerWebSocket } from "bun";
import { type DeviceRow, type Store, TOUCH_EVERY_MS, userJson } from "./db";
import { isObject } from "./http";
import { capabilities } from "./validate";

export interface SocketData {
  deviceId: string;
  userId: string;
  /** From the latest `hello`; [] until then. */
  capabilities: string[];
  /** When the client's last message arrived: its `lastSeenAt` when it goes stale. */
  lastActive: number;
  /** When `last_seen_at` was last written for this device. */
  touchedAt: number;
  /** Since when what we sent it has sat unread in our memory; unset while it reads. */
  stalledSince?: number;
}

type Socket = ServerWebSocket<SocketData>;
type Message = Record<string, unknown>;

const CLOSE_REPLACED = 4000;
const CLOSE_REVOKED = 4401;
const CLOSE_IDLE = 1001;
const CLOSE_UNSUPPORTED_DATA = 1003;
const CLOSE_POLICY = 1008;
const STALL_MS = 75 * 1000;

/**
 * Client → server message types. A new type is one entry here; a type not
 * listed is ignored, so a newer client keeps working against this server.
 * Phone ↔ computer kinds (dictate, text, agent.*) are not types: they travel
 * inside `relay` bodies, which the server never reads.
 */
const handlers = new Map<
  string,
  (hub: Hub, ws: Socket, message: Message) => void
>([
  [
    "hello",
    (hub, ws, message) => {
      const list = capabilities(message.capabilities);
      if (!list) return ws.close(CLOSE_POLICY, "invalid capabilities");
      ws.data.capabilities = list;
      hub.publishDevices(ws.data.userId);
    },
  ],
  [
    "relay",
    (hub, ws, { id, to, body }) => {
      if (
        typeof to !== "string" ||
        !isObject(body) ||
        (id != null && typeof id !== "string")
      ) {
        return ws.close(CLOSE_POLICY, "invalid relay");
      }
      if (!hub.relay(ws, to, body))
        send(ws, { type: "relay_failed", id, to, reason: "offline" });
    },
  ],
  ["ping", (_hub, ws) => send(ws, { type: "pong" })],
]);

function send(ws: Socket, message: Message): number {
  return ws.send(JSON.stringify(message));
}

/** Who is connected, and the routing between a user's devices (see the shortcut note in app.ts). */
export class Hub {
  private readonly byDevice = new Map<string, Socket>();
  private readonly byUser = new Map<string, Set<Socket>>();
  private open = 0;

  constructor(
    private readonly store: Store,
    private readonly now: () => number,
    private readonly log: (line: string) => void,
  ) {}

  onOpen(ws: Socket): void {
    this.open++;
    this.log(`ws connect (${this.open} open)`);
    const { deviceId, userId } = ws.data;
    const user = this.store.userById(userId);
    // Signed out between the upgrade and now: `revokeDevice` could not see this socket yet.
    if (!user || !this.store.deviceExists(deviceId))
      return ws.close(CLOSE_REVOKED, "signed out");
    const older = this.byDevice.get(deviceId);
    if (older) {
      this.forget(older);
      older.close(CLOSE_REPLACED, "replaced");
    }
    this.byDevice.set(deviceId, ws);
    const sockets = this.byUser.get(userId) ?? new Set();
    this.byUser.set(userId, sockets.add(ws));
    send(ws, { type: "ready", deviceId, user: userJson(user) });
    this.publishDevices(userId);
  }

  onMessage(ws: Socket, raw: string | Buffer): void {
    if (this.byDevice.get(ws.data.deviceId) !== ws) return;
    if (typeof raw !== "string")
      return ws.close(CLOSE_UNSUPPORTED_DATA, "text frames only");
    let message: unknown;
    try {
      message = JSON.parse(raw);
    } catch {
      message = null;
    }
    if (!isObject(message) || typeof message.type !== "string") {
      return ws.close(CLOSE_POLICY, "invalid message");
    }
    const now = this.now();
    ws.data.lastActive = now;
    if (now - ws.data.touchedAt >= TOUCH_EVERY_MS) {
      this.store.touchDevice(ws.data.deviceId, now);
      ws.data.touchedAt = now;
    }
    handlers.get(message.type)?.(this, ws, message);
  }

  onClose(ws: Socket, code: number): void {
    this.open--;
    this.log(`ws disconnect ${code} (${this.open} open)`);
    this.disconnect(ws, this.now());
  }

  /** The socket took some of what was queued for it. */
  onDrain(ws: Socket): void {
    ws.data.stalledSince = undefined;
  }

  /**
   * Closes every socket that has read nothing we sent it for 75 s: until it
   * reads, its share of the relay sits in our memory. A socket that sends
   * nothing is dropped by Bun's own idle timeout (see app.ts).
   */
  closeIdle(): void {
    const now = this.now();
    for (const ws of [...this.byDevice.values()]) {
      ws.data.stalledSince =
        ws.getBufferedAmount() > 0 ? (ws.data.stalledSince ?? now) : undefined;
      if ((ws.data.stalledSince ?? now) > now - STALL_MS) continue;
      this.disconnect(ws, ws.data.lastActive);
      ws.close(CLOSE_IDLE, "idle");
    }
  }

  /** Delivers `body` to another connected device of the sender's user. */
  relay(from: Socket, to: string, body: Message): boolean {
    const target = this.byDevice.get(to);
    if (!target || target.data.userId !== from.data.userId) return false;
    // 0 means dropped: the socket is closing or over its backpressure limit.
    return (
      send(target, { type: "relay", from: from.data.deviceId, body }) !== 0
    );
  }

  /** Closes a signed-out device's socket; the caller publishes the new device list. */
  revokeDevice(deviceId: string): void {
    const ws = this.byDevice.get(deviceId);
    if (!ws) return;
    this.forget(ws);
    ws.close(CLOSE_REVOKED, "signed out");
  }

  revokeUser(userId: string): void {
    for (const ws of [...(this.byUser.get(userId) ?? [])]) {
      this.forget(ws);
      ws.close(CLOSE_REVOKED, "signed out");
    }
  }

  /** Sends the user's device list to each of their sockets, with `current` set per recipient. */
  publishDevices(userId: string): void {
    const sockets = this.byUser.get(userId);
    if (!sockets) return;
    const rows = this.store.listDevices(userId, this.now());
    for (const ws of sockets)
      send(ws, {
        type: "devices",
        devices: this.devices(rows, ws.data.deviceId),
      });
  }

  /** Whether a device has a socket open right now (the admin dashboard). */
  isOnline(deviceId: string): boolean {
    return this.byDevice.has(deviceId);
  }

  /** How many devices have a socket open right now. */
  onlineCount(): number {
    return this.byDevice.size;
  }

  /** Current device first, then by name. */
  devices(rows: DeviceRow[], currentId: string) {
    return rows
      .map((row) => this.device(row, currentId))
      .sort(
        (a, b) =>
          Number(b.current) - Number(a.current) ||
          a.name.localeCompare(b.name) ||
          a.createdAt - b.createdAt,
      );
  }

  device(row: DeviceRow, currentId: string) {
    const ws = this.byDevice.get(row.id);
    return {
      id: row.id,
      name: row.name,
      platform: row.platform,
      kind: row.kind,
      appVersion: row.app_version,
      online: ws !== undefined,
      lastSeenAt: row.last_seen_at,
      createdAt: row.created_at,
      current: row.id === currentId,
      capabilities: ws?.data.capabilities ?? [],
    };
  }

  private disconnect(ws: Socket, seenAt: number): void {
    if (!this.forget(ws)) return;
    this.store.touchDevice(ws.data.deviceId, seenAt);
    this.publishDevices(ws.data.userId);
  }

  /** Drops the socket from presence; false when it was already replaced or revoked. */
  private forget(ws: Socket): boolean {
    const { deviceId, userId } = ws.data;
    if (this.byDevice.get(deviceId) !== ws) return false;
    this.byDevice.delete(deviceId);
    const sockets = this.byUser.get(userId);
    sockets?.delete(ws);
    if (sockets?.size === 0) this.byUser.delete(userId);
    return true;
  }
}
