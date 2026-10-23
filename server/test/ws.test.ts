import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { connect } from "node:net";
import {
  DESKTOP,
  type Harness,
  harness,
  type Json,
  PHONE,
  settle,
} from "./helpers";

let h: Harness;
beforeEach(async () => {
  h = await harness();
});
afterEach(() => h.close());

const ids = (message: Json) => message.devices.map((d: { id: string }) => d.id);
const find = (message: Json, id: string) =>
  message.devices.find((d: { id: string }) => d.id === id);

/** A client frame: text, masked as every client frame must be. */
function maskedFrame(text: string): Buffer {
  const payload = Buffer.from(text);
  const mask = [1, 2, 3, 4];
  const length =
    payload.length < 126
      ? [payload.length]
      : payload.length < 65536
        ? [126, payload.length >> 8, payload.length & 255]
        : [
            127,
            0,
            0,
            0,
            0,
            ...[24, 16, 8, 0].map((s) => (payload.length >> s) & 255),
          ];
  return Buffer.from([
    0x81,
    0x80 | length[0]!,
    ...length.slice(1),
    ...mask,
    ...payload.map((byte, i) => byte ^ mask[i % 4]!),
  ]);
}

/** A WebSocket on a plain TCP socket, past its upgrade, that the test writes frames to. */
async function rawSocket(base: string, token: string) {
  const socket = connect(Number(new URL(base).port), "127.0.0.1");
  socket.write(
    "GET /v1/ws HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n" +
      "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n" +
      `Authorization: Bearer ${token}\r\n\r\n`,
  );
  await new Promise((resolve) => socket.once("data", resolve));
  const closed = new Promise<boolean>((resolve) =>
    socket.once("close", () => resolve(true)),
  );
  return {
    socket,
    closed,
    isClosed: () => socket.destroyed || socket.closed,
    write: (bytes: Buffer) => socket.write(bytes),
    ping: () => socket.write(maskedFrame(JSON.stringify({ type: "ping" }))),
    destroy: () => socket.destroy(),
  };
}

/** A WebSocket that stops reading after its upgrade, but can still send a ping. */
async function stalledSocket(token: string) {
  const raw = await rawSocket(h.base, token);
  raw.socket.pause();
  return raw;
}

/** A signed-in desktop and phone of one account, both connected and past their first messages. */
async function pair(email = "a@example.com") {
  const desktop = await h.signup(email, DESKTOP);
  const phone = await h.login(email, PHONE);
  const desktopSocket = await h.connect(desktop.token);
  await desktopSocket.next("devices");
  const phoneSocket = await h.connect(phone.token);
  await phoneSocket.next("devices");
  await desktopSocket.next("devices", (m) => find(m, phone.device.id)?.online);
  return { desktop, phone, desktopSocket, phoneSocket };
}

describe("upgrade", () => {
  test("without a valid token answers 401 and does not upgrade", async () => {
    const reply = await h.request("GET", "/v1/ws");
    expect(reply.status).toBe(401);
    expect(reply.body.error.code).toBe("unauthorized");
    expect(
      h.connect("zv1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
    ).rejects.toThrow();
  });

  test("a plain request with a valid token is invalid_request", async () => {
    const { token } = await h.signup();
    const reply = await h.request("GET", "/v1/ws", { token });
    expect(reply.status).toBe(400);
    expect(reply.body.error.code).toBe("invalid_request");
  });

  test("sends ready, then the device list with this device online", async () => {
    const { token, user, device } = await h.signup();
    const socket = await h.connect(token);
    expect(await socket.next("ready")).toEqual({
      type: "ready",
      deviceId: device.id,
      user,
    });
    const devices = await socket.next("devices");
    expect(devices.devices).toEqual([
      { ...device, online: true, current: true, capabilities: [] },
    ]);
  });
});

describe("devices", () => {
  test("a device connecting and disconnecting updates the others", async () => {
    const { phone, desktopSocket, phoneSocket } = await pair();
    phoneSocket.close();
    const after = await desktopSocket.next("devices");
    expect(find(after, phone.device.id)).toMatchObject({
      online: false,
      capabilities: [],
    });
  });

  test("current is set per recipient", async () => {
    const { desktop, phone, desktopSocket, phoneSocket } = await pair();
    phoneSocket.send({ type: "hello", capabilities: [] });
    const forDesktop = await desktopSocket.next("devices");
    const forPhone = await phoneSocket.next("devices");
    expect(ids(forDesktop)[0]).toBe(desktop.device.id);
    expect(ids(forPhone)[0]).toBe(phone.device.id);
    expect(find(forDesktop, desktop.device.id).current).toBe(true);
    expect(find(forPhone, desktop.device.id).current).toBe(false);
  });

  test("hello capabilities reach the other device, and can change", async () => {
    const { desktop, desktopSocket, phoneSocket } = await pair();
    desktopSocket.send({
      type: "hello",
      capabilities: ["dictate", "text", "agent.run_1"],
    });
    const seen = await phoneSocket.next("devices");
    expect(find(seen, desktop.device.id).capabilities).toEqual([
      "dictate",
      "text",
      "agent.run_1",
    ]);
    desktopSocket.send({ type: "hello", capabilities: ["text"] });
    expect(
      find(await phoneSocket.next("devices"), desktop.device.id).capabilities,
    ).toEqual(["text"]);
  });

  test.each([
    ["not a list", "dictate"],
    ["too many", Array.from({ length: 65 }, (_, i) => `c${i}`)],
    ["too long", ["x".repeat(65)]],
    ["bad characters", ["Dictate"]],
    ["empty name", [""]],
    ["not strings", [1]],
  ])(
    "an invalid hello (%s) closes the socket",
    async (_label, capabilities) => {
      const { token } = await h.signup();
      const socket = await h.connect(token);
      socket.send({ type: "hello", capabilities });
      expect((await socket.closed).code).toBe(1008);
    },
  );

  test("a new sign-in on the account updates connected devices", async () => {
    const desktop = await h.signup();
    const socket = await h.connect(desktop.token);
    await socket.next("devices");
    const phone = await h.login();
    const update = await socket.next("devices");
    expect(find(update, phone.device.id)).toMatchObject({
      online: false,
      current: false,
    });
  });

  test("another account's devices are never listed or announced", async () => {
    const { desktopSocket } = await pair("a@example.com");
    const other = await h.signup("b@example.com");
    const otherSocket = await h.connect(other.token);
    const otherDevices = await otherSocket.next("devices");
    expect(ids(otherDevices)).toEqual([other.device.id]);
    await settle();
    expect(desktopSocket.queued("devices")).toEqual([]);
  });
});

describe("relay", () => {
  test("routes a body to another device of the same user, untouched", async () => {
    const { desktop, phone, desktopSocket, phoneSocket } = await pair();
    const body = {
      kind: "text",
      requestId: "6f1c",
      text: "héllo",
      nested: { a: [1, null] },
    };
    phoneSocket.send({ type: "relay", id: "r1", to: desktop.device.id, body });
    expect(await desktopSocket.next("relay")).toEqual({
      type: "relay",
      from: phone.device.id,
      body,
    });
    const result = {
      kind: "result",
      requestId: "6f1c",
      ok: true,
      text: "héllo",
      pasted: true,
    };
    desktopSocket.send({ type: "relay", to: phone.device.id, body: result });
    expect((await phoneSocket.next("relay")).body).toEqual(result);
  });

  test("carries a dictation-sized body", async () => {
    const { desktop, desktopSocket, phoneSocket } = await pair();
    // About five minutes of 16 kHz 16-bit mono audio, base64-encoded.
    const audio = "A".repeat(13 * 1024 * 1024);
    phoneSocket.send({
      type: "relay",
      to: desktop.device.id,
      body: { kind: "dictate", audio },
    });
    expect((await desktopSocket.next("relay")).body.audio.length).toBe(
      audio.length,
    );
  });

  test("a frame over 16 MiB closes the socket", async () => {
    const { phoneSocket } = await pair();
    phoneSocket.send("x".repeat(16 * 1024 * 1024 + 1));
    // Bun drops the connection without a close frame, so the client sees 1006 rather than 1009.
    expect([1006, 1009]).toContain((await phoneSocket.closed).code);
  });

  test("a body too deeply nested to forward closes only its sender's socket", async () => {
    const { desktop, desktopSocket, phoneSocket } = await pair();
    const depth = 1_000_000;
    phoneSocket.send(
      `{"type":"relay","to":"${desktop.device.id}","body":${'{"a":'.repeat(depth)}1${"}".repeat(depth)}}`,
    );
    expect((await phoneSocket.closed).code).toBe(1011);
    expect((await h.request("GET", "/v1/health")).status).toBe(200);
    desktopSocket.send({ type: "ping" });
    await desktopSocket.next("pong");
    expect(desktopSocket.queued("relay")).toEqual([]);
  });

  test("relay_failed for a signed-in device that is not connected", async () => {
    const desktop = await h.signup();
    const phone = await h.login();
    const socket = await h.connect(desktop.token);
    socket.send({ type: "relay", id: "r2", to: phone.device.id, body: {} });
    expect(await socket.next("relay_failed")).toEqual({
      type: "relay_failed",
      id: "r2",
      to: phone.device.id,
      reason: "offline",
    });
  });

  test("relay_failed for an unknown device, without an id when none was given", async () => {
    const { token } = await h.signup();
    const socket = await h.connect(token);
    socket.send({ type: "relay", to: "nobody", body: {} });
    expect(await socket.next("relay_failed")).toEqual({
      type: "relay_failed",
      to: "nobody",
      reason: "offline",
    });
  });

  test("relay_failed for another user's connected device, which receives nothing", async () => {
    const { phoneSocket } = await pair("a@example.com");
    const other = await h.signup("b@example.com");
    const otherSocket = await h.connect(other.token);
    await otherSocket.next("devices");
    phoneSocket.send({
      type: "relay",
      id: "r3",
      to: other.device.id,
      body: { kind: "text" },
    });
    expect(await phoneSocket.next("relay_failed")).toMatchObject({
      id: "r3",
      reason: "offline",
    });
    await settle();
    expect(otherSocket.queued("relay")).toEqual([]);
  });

  test.each([
    ["no target", { body: {} }],
    ["a list body", { to: "x", body: [] }],
    ["no body", { to: "x" }],
    ["a numeric id", { id: 7, to: "x", body: {} }],
  ])("a malformed relay (%s) closes the socket", async (_label, fields) => {
    const { token } = await h.signup();
    const socket = await h.connect(token);
    socket.send({ type: "relay", ...fields });
    expect((await socket.closed).code).toBe(1008);
  });
});

describe("connection rules", () => {
  test("ping answers pong; unknown message types are ignored", async () => {
    const { token } = await h.signup();
    const socket = await h.connect(token);
    socket.send({ type: "from-the-future" });
    socket.send({ type: "ping" });
    expect(await socket.next("pong")).toEqual({ type: "pong" });
  });

  test("non-JSON and binary frames close the socket", async () => {
    const { token } = await h.signup();
    const text = await h.connect(token);
    text.send("not json");
    expect((await text.closed).code).toBe(1008);
    const binary = await h.connect(token);
    binary.ws.send(new Uint8Array([1, 2, 3]));
    expect((await binary.closed).code).toBe(1003);
  });

  test("a second socket for the same device closes the first with 4000", async () => {
    const { token, device } = await h.signup();
    const first = await h.connect(token);
    await first.next("devices");
    const second = await h.connect(token);
    expect((await first.closed).code).toBe(4000);
    expect((await second.next("ready")).deviceId).toBe(device.id);
    second.send({ type: "ping" });
    expect(await second.next("pong")).toEqual({ type: "pong" });
  });

  test("removing a device closes its socket with 4401", async () => {
    const { phone, desktop, desktopSocket, phoneSocket } = await pair();
    await h.request("DELETE", `/v1/devices/${phone.device.id}`, {
      token: desktop.token,
    });
    expect((await phoneSocket.closed).code).toBe(4401);
    expect(ids(await desktopSocket.next("devices"))).toEqual([
      desktop.device.id,
    ]);
  });

  test("logging out closes this device's socket with 4401", async () => {
    const { phone, desktop, desktopSocket, phoneSocket } = await pair();
    await h.request("POST", "/v1/auth/logout", { token: phone.token });
    expect((await phoneSocket.closed).code).toBe(4401);
    expect(ids(await desktopSocket.next("devices"))).toEqual([
      desktop.device.id,
    ]);
  });

  test("deleting the account closes every socket with 4401", async () => {
    const { phone, desktopSocket, phoneSocket } = await pair();
    await h.request("DELETE", "/v1/me", { token: phone.token });
    expect((await desktopSocket.closed).code).toBe(4401);
    expect((await phoneSocket.closed).code).toBe(4401);
  });

  test("a socket nothing arrives from is dropped; one still sending a frame stays", async () => {
    // Bun's idle clock ticks in 4 s steps, so 8 s is the shortest that holds.
    const quick = await harness({ idleSeconds: 8 });
    try {
      const desktop = await quick.signup("a@example.com", DESKTOP);
      const phone = await quick.login("a@example.com", PHONE);
      // Another account's, so nothing it does is sent to the uploading phone:
      // what the server sends restarts a socket's idle clock too.
      const other = await quick.signup("b@example.com", PHONE);
      const desktopSocket = await quick.connect(desktop.token);
      const talking = setInterval(
        () => desktopSocket.send({ type: "ping" }),
        1000,
      );
      const uploading = await rawSocket(quick.base, phone.token);
      await desktopSocket.next(
        "devices",
        (m) => find(m, phone.device.id)?.online,
      );
      const quiet = await rawSocket(quick.base, other.token);

      // One relay, sent a slice a second for half again the idle limit, as a
      // long dictation goes up a slow link.
      const body = { kind: "text", requestId: "r1", text: "x".repeat(12_000) };
      const frame = maskedFrame(
        JSON.stringify({ type: "relay", to: desktop.device.id, body }),
      );
      const slices = 13;
      const size = Math.ceil(frame.length / slices);
      for (let i = 0; i < slices; i++) {
        uploading.write(frame.subarray(i * size, (i + 1) * size));
        await Bun.sleep(1000);
      }

      expect(quiet.isClosed()).toBe(true);
      expect(uploading.isClosed()).toBe(false);
      expect(await desktopSocket.next("relay")).toEqual({
        type: "relay",
        from: phone.device.id,
        body,
      });
      expect(desktopSocket.queued("devices")).toEqual([]);
      clearInterval(talking);
      uploading.destroy();
    } finally {
      await quick.close();
    }
  });

  test("a socket that reads nothing for 75 s is closed, even while it pings", async () => {
    const desktop = await h.signup("a@example.com", DESKTOP);
    const phone = await h.login("a@example.com", PHONE);
    const phoneSocket = await h.connect(phone.token);
    await phoneSocket.next("devices");
    const stalled = await stalledSocket(desktop.token);
    await phoneSocket.next(
      "devices",
      (m) => find(m, desktop.device.id)?.online,
    );
    const text = "x".repeat(8 * 1024 * 1024);
    for (let i = 0; i < 2; i++) {
      phoneSocket.send({
        type: "relay",
        to: desktop.device.id,
        body: { kind: "text", text },
      });
    }
    // Long enough for a once-a-second idle check to see what waits for it.
    await Bun.sleep(1500);
    h.advance(60 * 1000);
    stalled.ping();
    phoneSocket.send({ type: "ping" });
    await phoneSocket.next("pong");
    await settle();
    h.advance(15 * 1000);
    const update = await phoneSocket.next(
      "devices",
      (m) => find(m, desktop.device.id)?.online === false,
    );
    expect(find(update, phone.device.id).online).toBe(true);
    stalled.destroy();
  });

  test("activity on the socket keeps the session alive and lastSeenAt current", async () => {
    const { token, device } = await h.signup();
    const socket = await h.connect(token);
    await socket.next("devices");
    h.advance(61 * 1000);
    socket.send({ type: "ping" });
    await socket.next("pong");
    socket.close();
    await socket.closed;
    await settle();
    const me = await h.request("GET", "/v1/me", { token });
    expect(me.body.device.id).toBe(device.id);
    expect(me.body.device.lastSeenAt).toBe(h.now());
  });
});
