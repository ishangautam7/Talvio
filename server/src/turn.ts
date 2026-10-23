import { authenticate } from "./account";
import type { Ctx } from "./app";
import { json } from "./http";

/**
 * ICE servers for a live view (PROTOCOL.md, "TURN"). Phone and computer
 * connect directly whenever they can; a TURN server relays their encrypted
 * media when neither can reach the other (strict NATs, most mobile networks).
 */

/** Where TURN credentials come from, if anywhere. */
export type TurnConfig =
  /** A coturn-style server with `use-auth-secret`: the TURN REST scheme. */
  | { kind: "secret"; urls: string[]; secret: string }
  /** Cloudflare's TURN service, which issues short-lived credentials per call. */
  | {
      kind: "cloudflare";
      keyId: string;
      apiToken: string;
      /** Defaults to Cloudflare's API; tests point it elsewhere. */
      baseUrl?: string;
    };

export interface IceServer {
  urls: string[];
  username?: string;
  credential?: string;
}

/** Always offered: answers "what is my public address" and relays nothing. */
export const STUN: IceServer = {
  urls: ["stun:stun.cloudflare.com:3478", "stun:stun.l.google.com:19302"],
};

/** How long issued credentials last. A live view longer than this keeps its connection. */
export const TURN_TTL_SECONDS = 24 * 60 * 60;

const CLOUDFLARE_API = "https://rtc.live.cloudflare.com/v1";
const CLOUDFLARE_TIMEOUT_MS = 5000;

/** The TURN settings from the environment, or null for STUN only. */
export function turnFromEnv(
  env: Record<string, string | undefined>,
): TurnConfig | null {
  if (env.CLOUDFLARE_TURN_KEY_ID && env.CLOUDFLARE_TURN_API_TOKEN) {
    return {
      kind: "cloudflare",
      keyId: env.CLOUDFLARE_TURN_KEY_ID,
      apiToken: env.CLOUDFLARE_TURN_API_TOKEN,
    };
  }
  const urls = (env.TURN_URLS ?? "")
    .split(",")
    .map((url) => url.trim())
    .filter(Boolean);
  if (urls.length > 0 && env.TURN_SECRET) {
    return { kind: "secret", urls, secret: env.TURN_SECRET };
  }
  return null;
}

/** `GET /v1/turn`. A TURN service that fails leaves STUN only, never an error. */
export async function turn(req: Request, ctx: Ctx): Promise<Response> {
  const device = authenticate(req, ctx);
  const iceServers: IceServer[] = [STUN];
  const relay = await turnServers(ctx, device.user_id);
  iceServers.push(...relay);
  return json({ iceServers, ttl: TURN_TTL_SECONDS });
}

async function turnServers(ctx: Ctx, userId: string): Promise<IceServer[]> {
  const config = ctx.turn;
  if (!config) return [];
  try {
    return config.kind === "secret"
      ? [await sharedSecretServer(config, userId, ctx.now())]
      : await cloudflareServers(config);
  } catch (error) {
    ctx.log(
      `TURN credentials unavailable, offering STUN only: ${error instanceof Error ? error.message : String(error)}`,
    );
    return [];
  }
}

/**
 * The TURN REST scheme coturn's `use-auth-secret` checks: the username is
 * `<expiry unix seconds>:<user id>` and the password the base64 HMAC-SHA1 of
 * the username under the shared secret.
 */
export async function sharedSecretServer(
  config: { urls: string[]; secret: string },
  userId: string,
  nowMs: number,
): Promise<IceServer> {
  const expires = Math.floor(nowMs / 1000) + TURN_TTL_SECONDS;
  const username = `${expires}:${userId}`;
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(config.secret),
    { name: "HMAC", hash: "SHA-1" },
    false,
    ["sign"],
  );
  const mac = await crypto.subtle.sign(
    "HMAC",
    key,
    new TextEncoder().encode(username),
  );
  return {
    urls: config.urls,
    username,
    credential: Buffer.from(mac).toString("base64"),
  };
}

/** Cloudflare issues credentials per call; its answer is used as it comes. */
async function cloudflareServers(config: {
  keyId: string;
  apiToken: string;
  baseUrl?: string;
}): Promise<IceServer[]> {
  const base = config.baseUrl ?? CLOUDFLARE_API;
  const response = await fetch(
    `${base}/turn/keys/${encodeURIComponent(config.keyId)}/credentials/generate-ice-servers`,
    {
      method: "POST",
      headers: {
        authorization: `Bearer ${config.apiToken}`,
        "content-type": "application/json",
      },
      body: JSON.stringify({ ttl: TURN_TTL_SECONDS }),
      signal: AbortSignal.timeout(CLOUDFLARE_TIMEOUT_MS),
    },
  );
  if (!response.ok) throw new Error(`Cloudflare answered ${response.status}`);
  const body = (await response.json()) as { iceServers?: unknown };
  // An array today; a single object in the older `generate` answer.
  const listed = Array.isArray(body.iceServers)
    ? body.iceServers
    : [body.iceServers];
  const servers = listed.flatMap(iceServer);
  if (servers.length === 0) throw new Error("Cloudflare sent no ICE servers");
  return servers;
}

/** One ICE server from an untrusted answer, or none if it is not one. */
function iceServer(value: unknown): IceServer[] {
  if (typeof value !== "object" || value === null) return [];
  const { urls, username, credential } = value as Record<string, unknown>;
  const list = (Array.isArray(urls) ? urls : [urls]).filter(
    (url): url is string =>
      typeof url === "string" && /^(stun|stuns|turn|turns):/.test(url),
  );
  if (list.length === 0) return [];
  const server: IceServer = { urls: list };
  if (typeof username === "string") server.username = username;
  if (typeof credential === "string") server.credential = credential;
  return [server];
}
