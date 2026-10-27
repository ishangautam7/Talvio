import type { Server, ServerWebSocket } from "bun";
import {
  authenticate,
  deleteDevice,
  deleteMe,
  listDevices,
  login,
  logout,
  me,
  signup,
} from "./account";
import {
  adminGoogleExchange,
  adminGoogleStart,
  adminLogin,
  adminLogout,
  adminPageRoute,
  AdminSessions,
  listUsers,
  overview,
  userDetail,
} from "./admin";
import { Store } from "./db";
import { Free } from "./free";
import type { FreeConfig } from "./free/config";
import {
  deleteFeedback,
  type FeedbackGithub,
  type FeedbackLimits,
  feedbackScreenshot,
  listFeedback,
  sendFeedback,
} from "./feedback";
import {
  callback,
  exchange,
  GOOGLE_TOKEN_URL,
  type GoogleConfig,
  start,
} from "./google";
import {
  ApiError,
  errorResponse,
  invalidRequest,
  json,
  notFound,
  rateLimited,
} from "./http";
import { Hub, type SocketData } from "./hub";
import { addressKey, RateLimit } from "./rateLimit";
import { deleteSecret, listSecrets, putSecret, SecretBox } from "./secrets";
import { resolveSecretsKey } from "./secretsKey";
import { turn, type TurnConfig } from "./turn";

export interface Options {
  port: number;
  hostname: string;
  /** A file path, or ":memory:". */
  databasePath: string;
  /** Where browsers reach this server, without a trailing slash; Google redirects back here. */
  publicUrl: string;
  /** Null turns Google sign-in off. `tokenUrl` defaults to Google's. */
  google: { clientId: string; clientSecret: string; tokenUrl?: string } | null;
  /** Take the client address from X-Forwarded-For (behind exactly one reverse proxy). */
  trustProxy: boolean;
  /** Base64 of the 32-byte key for stored API keys (`TALVIO_SECRETS_KEY`); unset uses `secrets.key` beside the database. */
  secretsKey?: string;
  /** Where live-view TURN credentials come from (PROTOCOL.md, "TURN"); null offers STUN only. */
  turn?: TurnConfig | null;
  /** Unlocks the owner's admin routes for scripts (`ADMIN_TOKEN`). */
  adminToken?: string | null;
  /** Accounts that may sign in to `/admin` (`ADMIN_EMAILS`). With neither this nor `adminToken`, every admin route answers `not_found`. */
  adminEmails?: string[];
  /** Also file each feedback entry as a GitHub issue; null files nothing. */
  feedbackGithub?: FeedbackGithub | null;
  /** Free credit upstreams and prices (PROTOCOL.md, "Free credit"); null or absent turns it off. */
  free?: FreeConfig | null;
  /** Seconds without a byte from a client before its socket is dropped (75; tests shorten it). */
  idleSeconds?: number;
  now?: () => number;
  log?: (line: string) => void;
}

export interface Ctx {
  store: Store;
  hub: Hub;
  now: () => number;
  log: (line: string) => void;
  publicUrl: string;
  google: GoogleConfig | null;
  loginFailures: RateLimit;
  secrets: SecretBox;
  turn: TurnConfig | null;
  adminToken: string | null;
  adminEmails: Set<string>;
  adminSessions: AdminSessions;
  feedbackGithub: FeedbackGithub | null;
  feedbackLimits: FeedbackLimits;
  free: Free;
}

type Route = (
  req: Request,
  ctx: Ctx,
  url: URL,
  server: Server<SocketData>,
) => Response | undefined | Promise<Response | undefined>;

const routes = new Map<string, Route>([
  ["GET /v1/health", () => json({ ok: true })],
  [
    "GET /v1/config",
    (_req, ctx) => json({ protocol: 1, google: ctx.google !== null }),
  ],
  ["POST /v1/auth/signup", (req, ctx) => signup(req, ctx)],
  ["POST /v1/auth/login", (req, ctx) => login(req, ctx)],
  ["GET /v1/auth/google/start", (_req, ctx, url) => start(url, ctx)],
  ["GET /v1/auth/google/callback", (_req, ctx, url) => callback(url, ctx)],
  ["POST /v1/auth/exchange", (req, ctx) => exchange(req, ctx)],
  ["POST /v1/auth/logout", (req, ctx) => logout(req, ctx)],
  ["GET /v1/me", (req, ctx) => me(req, ctx)],
  ["DELETE /v1/me", (req, ctx) => deleteMe(req, ctx)],
  ["GET /v1/devices", (req, ctx) => listDevices(req, ctx)],
  ["GET /v1/secrets", (req, ctx) => listSecrets(req, ctx)],
  ["GET /v1/turn", (req, ctx) => turn(req, ctx)],
  ["POST /v1/admin/login", (req, ctx) => adminLogin(req, ctx)],
  [
    "GET /v1/admin/google/start",
    (_req, ctx, url) => adminGoogleStart(url, ctx),
  ],
  [
    "POST /v1/admin/google/exchange",
    (req, ctx) => adminGoogleExchange(req, ctx),
  ],
  ["POST /v1/admin/logout", (req, ctx) => adminLogout(req, ctx)],
  [
    "GET /v1/ws",
    (req, ctx, _url, server) => {
      const device = authenticate(req, ctx);
      const data: SocketData = {
        deviceId: device.id,
        userId: device.user_id,
        capabilities: [],
        lastActive: ctx.now(),
        touchedAt: device.last_seen_at,
      };
      if (server.upgrade(req, { data })) return undefined;
      throw invalidRequest("Expected a WebSocket upgrade.");
    },
  ],
]);

/**
 * What "20 auth requests per minute per IP" counts: the routes that sign in or
 * start to. Not Google's callback: it needs a state from a counted start, and a
 * 429 there would leave the browser on JSON instead of sending it back to the app.
 */
const PER_IP_LIMITED = new Set([
  "POST /v1/auth/signup",
  "POST /v1/auth/login",
  "GET /v1/auth/google/start",
  "POST /v1/auth/exchange",
  "POST /v1/admin/login",
  "GET /v1/admin/google/start",
  "POST /v1/admin/google/exchange",
]);

const DEVICE_PATH = /^\/v1\/devices\/([^/]+)$/;
const FEEDBACK_PATH = /^\/v1\/admin\/feedback\/([^/]+)$/;
const SCREENSHOT_PATH =
  /^\/v1\/admin\/feedback\/([^/]+)\/screenshots\/(\d{1,2})$/;
const SECRET_PATH = /^\/v1\/secrets\/([^/]+)$/;
const ADMIN_USER_PATH = /^\/v1\/admin\/users\/([^/]+)$/;

export function startServer(options: Options) {
  const now = options.now ?? Date.now;
  const log = options.log ?? console.log;
  // Resolved before the database opens, so a bad key stops the start with nothing touched.
  const secretsKey = resolveSecretsKey(
    options.databasePath,
    options.secretsKey,
  );
  log(`Secrets key: ${secretsKey.source}`);
  const store = new Store(options.databasePath);
  // shortcut: presence (in Hub) and both rate limits live in this process's
  // memory, so run a single instance; move them to a shared store such as
  // Redis before running more than one.
  const hub = new Hub(store, now, log);
  const perIp = new RateLimit(20, 60 * 1000, now);
  const loginFailures = new RateLimit(10, 15 * 60 * 1000, now);
  const feedbackLimits: FeedbackLimits = {
    perIp: new RateLimit(10, 60 * 60 * 1000, now),
    global: new RateLimit(200, 60 * 60 * 1000, now),
    adminFailures: new RateLimit(10, 15 * 60 * 1000, now),
  };
  const ctx: Ctx = {
    store,
    hub,
    now,
    log,
    publicUrl: options.publicUrl,
    google: options.google && { tokenUrl: GOOGLE_TOKEN_URL, ...options.google },
    loginFailures,
    secrets: new SecretBox(secretsKey.key),
    turn: options.turn ?? null,
    adminToken: options.adminToken || null,
    adminEmails: new Set(
      (options.adminEmails ?? []).map((address) =>
        address.trim().toLowerCase(),
      ),
    ),
    adminSessions: new AdminSessions(now),
    feedbackGithub: options.feedbackGithub ?? null,
    feedbackLimits,
    free: new Free(
      options.free ?? {
        limitUsd: 1,
        perMinute: 60,
        chat: null,
        stt: null,
        tts: null,
      },
      store.database,
      { store, now, log },
    ),
  };

  for (const address of ctx.adminEmails) {
    // A password sign-up never proves its address, so until the owner has the
    // account, whoever registers the address first would be an admin.
    if (!store.userByEmail(address)) {
      log(
        `Warning: an ADMIN_EMAILS address has no account yet; sign up with it before anyone else can.`,
      );
    }
  }

  function clientIp(req: Request, server: Server<SocketData>): string {
    if (options.trustProxy) {
      // The rightmost entry is the one our proxy appended; anything left of it came from the client.
      const forwarded = req.headers
        .get("x-forwarded-for")
        ?.split(",")
        .at(-1)
        ?.trim();
      if (forwarded) return forwarded;
    }
    return server.requestIP(req)?.address ?? "unknown";
  }

  const logInternal = (error: unknown) =>
    log(
      `internal error: ${error instanceof Error ? error.stack : String(error)}`,
    );

  /** Bun ends the process on an error thrown in a socket handler or a timer, so each runs through here. */
  function guard(run: () => void, ws?: ServerWebSocket<SocketData>): void {
    try {
      run();
    } catch (error) {
      logInternal(error);
      ws?.close(1011, "internal error");
    }
  }

  async function route(req: Request, url: URL, server: Server<SocketData>) {
    const key = `${req.method} ${url.pathname}`;
    if (PER_IP_LIMITED.has(key)) {
      const wait = perIp.take(addressKey(clientIp(req, server)));
      if (wait > 0) throw rateLimited(wait);
    }
    const handler = routes.get(key);
    if (handler) return handler(req, ctx, url, server);
    // Feedback is limited per address, so these routes need the client's.
    const ip = () => addressKey(clientIp(req, server));
    if (key === "POST /v1/feedback") return sendFeedback(req, ctx, ip());
    if (key === "GET /v1/admin/feedback")
      return listFeedback(req, ctx, url, ip());
    if (key === "GET /v1/admin/overview") return overview(req, ctx, ip());
    if (key === "GET /v1/admin/users") return listUsers(req, ctx, url, ip());
    const adminUser =
      req.method === "GET"
        ? ADMIN_USER_PATH.exec(url.pathname)?.[1]
        : undefined;
    if (adminUser) return userDetail(req, ctx, ip(), adminUser);
    // The page holds no data, but a server with no way in does not offer it.
    if (key === "GET /admin" || key === "GET /admin/")
      return adminPageRoute(ctx);
    const shot =
      req.method === "GET" ? SCREENSHOT_PATH.exec(url.pathname) : null;
    if (shot)
      return feedbackScreenshot(req, ctx, ip(), shot[1]!, Number(shot[2]));
    const feedbackId =
      req.method === "DELETE"
        ? FEEDBACK_PATH.exec(url.pathname)?.[1]
        : undefined;
    if (feedbackId) return deleteFeedback(req, ctx, ip(), feedbackId);
    const deviceId =
      req.method === "DELETE" ? DEVICE_PATH.exec(url.pathname)?.[1] : undefined;
    if (deviceId) return deleteDevice(req, ctx, deviceId);
    const secretName = SECRET_PATH.exec(url.pathname)?.[1];
    if (secretName && req.method === "PUT")
      return putSecret(req, ctx, secretName);
    if (secretName && req.method === "DELETE")
      return deleteSecret(req, ctx, url, secretName);
    if (url.pathname.startsWith("/v1/free/"))
      return ctx.free.route(req, url, server);
    throw notFound();
  }

  const server = Bun.serve<SocketData>({
    port: options.port,
    hostname: options.hostname,
    async fetch(req, server) {
      const started = performance.now();
      const url = new URL(req.url);
      let response: Response | undefined;
      try {
        response = await route(req, url, server);
      } catch (error) {
        if (!(error instanceof ApiError)) logInternal(error);
        response = errorResponse(
          error instanceof ApiError
            ? error
            : new ApiError(
                500,
                "internal",
                "Something went wrong on the server.",
              ),
        );
      }
      const ms = Math.round(performance.now() - started);
      log(`${req.method} ${url.pathname} ${response?.status ?? 101} ${ms}ms`);
      return response;
    },
    websocket: {
      maxPayloadLength: 16 * 1024 * 1024,
      // Counted on bytes, not messages, so a dictation still arriving keeps
      // its socket; a quiet client's own pings keep it too. A socket that
      // stops reading is closeIdle's (what we send restarts this clock).
      idleTimeout: options.idleSeconds ?? 75,
      sendPings: false,
      open: (ws) => guard(() => hub.onOpen(ws), ws),
      message: (ws, message) => guard(() => hub.onMessage(ws, message), ws),
      drain: (ws) => hub.onDrain(ws),
      close: (ws, code) => guard(() => hub.onClose(ws, code)),
    },
  });

  const idleTimer = setInterval(() => guard(() => hub.closeIdle()), 1000);
  const cleanupTimer = setInterval(
    () =>
      guard(() => {
        store.deleteExpired(now());
        perIp.prune();
        loginFailures.prune();
        feedbackLimits.perIp.prune();
        feedbackLimits.global.prune();
        feedbackLimits.adminFailures.prune();
        ctx.free.prune();
        ctx.adminSessions.prune();
      }),
    10 * 60 * 1000,
  );

  return {
    port: server.port!,
    url: server.url.href,
    async stop() {
      clearInterval(idleTimer);
      clearInterval(cleanupTimer);
      await server.stop(true);
      store.close();
    },
  };
}
