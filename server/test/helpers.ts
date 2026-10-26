import { startServer } from "../src/app";
import { randomToken } from "../src/crypto";
import type { FeedbackGithub } from "../src/feedback";
import type { TurnConfig } from "../src/turn";

// Test messages are loosely typed on purpose: each assertion states the shape it expects.
export type Json = Record<string, any>;

export const DESKTOP = {
  name: "ISHAN-PC",
  platform: "windows",
  kind: "desktop",
  appVersion: "2.2.0",
};
export const PHONE = {
  name: "Android phone",
  platform: "android",
  kind: "mobile",
};
export const PASSWORD = "correct horse";
export const CLIENT_ID = "client-123.apps.googleusercontent.com";
export const PUBLIC_URL = "https://zv.test";

export interface Reply {
  status: number;
  headers: Headers;
  body: Json;
  text: string;
}

export type Harness = Awaited<ReturnType<typeof harness>>;

export async function harness(
  options: {
    google?: boolean;
    trustProxy?: boolean;
    idleSeconds?: number;
    databasePath?: string;
    secretsKey?: string;
    turn?: TurnConfig | null;
    adminToken?: string | null;
    feedbackGithub?: FeedbackGithub | null;
  } = {},
) {
  let time = Date.UTC(2026, 0, 1);
  const google = options.google === false ? null : fakeGoogle();
  const app = startServer({
    port: 0,
    hostname: "127.0.0.1",
    databasePath: options.databasePath ?? ":memory:",
    secretsKey: options.secretsKey,
    turn: options.turn ?? null,
    adminToken: options.adminToken ?? null,
    feedbackGithub: options.feedbackGithub ?? null,
    publicUrl: PUBLIC_URL,
    google: google && {
      clientId: CLIENT_ID,
      clientSecret: "shh",
      tokenUrl: google.url,
    },
    trustProxy: options.trustProxy ?? false,
    idleSeconds: options.idleSeconds,
    now: () => time,
    log: () => {},
  });
  const base = `http://127.0.0.1:${app.port}`;

  async function request(
    method: string,
    path: string,
    init: {
      body?: unknown;
      token?: string;
      headers?: Record<string, string>;
    } = {},
  ): Promise<Reply> {
    const headers: Record<string, string> = { ...init.headers };
    if (init.token) headers.authorization = `Bearer ${init.token}`;
    const response = await fetch(base + path, {
      method,
      redirect: "manual",
      headers,
      body:
        init.body === undefined || typeof init.body === "string"
          ? (init.body as string | undefined)
          : JSON.stringify(init.body),
    });
    const text = await response.text();
    const isJson = response.headers
      .get("content-type")
      ?.includes("application/json");
    return {
      status: response.status,
      headers: response.headers,
      body: isJson ? JSON.parse(text) : {},
      text,
    };
  }

  async function signup(
    email = "a@example.com",
    device: object = DESKTOP,
  ): Promise<Json> {
    const reply = await request("POST", "/v1/auth/signup", {
      body: { email, password: PASSWORD, device },
    });
    if (reply.status !== 201) throw new Error(`signup failed: ${reply.text}`);
    return reply.body;
  }

  async function login(
    email = "a@example.com",
    device: object = PHONE,
  ): Promise<Json> {
    const reply = await request("POST", "/v1/auth/login", {
      body: { email, password: PASSWORD, device },
    });
    if (reply.status !== 200) throw new Error(`login failed: ${reply.text}`);
    return reply.body;
  }

  return {
    base,
    google,
    request,
    signup,
    login,
    now: () => time,
    advance(ms: number) {
      time += ms;
    },
    connect: (token: string) => Client.connect(base, token),
    async close() {
      await app.stop();
      await google?.stop();
    },
  };
}

/** A stand-in for Google's token endpoint: each code it issued answers with an id_token for its claims. */
export function fakeGoogle() {
  const codes = new Map<string, Json | number>();
  const requests: URLSearchParams[] = [];
  const server = Bun.serve({
    port: 0,
    hostname: "127.0.0.1",
    async fetch(req) {
      const form = new URLSearchParams(await req.text());
      requests.push(form);
      const entry = codes.get(form.get("code") ?? "");
      if (entry === undefined)
        return Response.json({ error: "invalid_grant" }, { status: 400 });
      if (typeof entry === "number")
        return new Response(null, { status: entry });
      const idToken = [{ alg: "RS256" }, entry]
        .map((part) => Buffer.from(JSON.stringify(part)).toString("base64url"))
        .join(".");
      return Response.json({
        access_token: "ya29",
        id_token: `${idToken}.signature`,
      });
    },
  });
  return {
    url: `http://127.0.0.1:${server.port}/token`,
    requests,
    /** A Google authorization code for these claims, or for an HTTP error status. */
    issue(claims: Json | number): string {
      const code = `4/${randomToken()}`;
      codes.set(code, claims);
      return code;
    },
    stop: () => server.stop(true),
  };
}

export function googleClaims(nowMs: number, overrides: Json = {}): Json {
  return {
    iss: "https://accounts.google.com",
    aud: CLIENT_ID,
    sub: "google-sub-1",
    email: "a@example.com",
    email_verified: true,
    name: "Ishan Gautam",
    exp: Math.floor(nowMs / 1000) + 3600,
    ...overrides,
  };
}

/** A WebSocket test client that queues what the server sends. */
export class Client {
  private readonly queue: Json[] = [];
  private wake: () => void = () => {};
  readonly closed: Promise<{ code: number; reason: string }>;

  private constructor(readonly ws: WebSocket) {
    ws.onmessage = (event) => {
      this.queue.push(JSON.parse(String(event.data)));
      this.wake();
    };
    this.closed = new Promise((resolve) => {
      ws.onclose = (event) =>
        resolve({ code: event.code, reason: event.reason });
    });
  }

  static connect(base: string, token: string): Promise<Client> {
    const ws = new WebSocket(`${base.replace("http", "ws")}/v1/ws`, {
      headers: { authorization: `Bearer ${token}` },
    });
    const client = new Client(ws);
    return new Promise((resolve, reject) => {
      ws.onopen = () => resolve(client);
      ws.onerror = () => reject(new Error("WebSocket did not open"));
    });
  }

  send(message: unknown): void {
    this.ws.send(
      typeof message === "string" ? message : JSON.stringify(message),
    );
  }

  /** The next message of `type` that also passes `match`, dropping everything queued before it. */
  async next(
    type: string,
    match: (message: Json) => boolean = () => true,
  ): Promise<Json> {
    const deadline = Date.now() + 5000;
    for (;;) {
      while (this.queue.length > 0) {
        const message = this.queue.shift()!;
        if (message.type === type && match(message)) return message;
      }
      const remaining = deadline - Date.now();
      if (remaining <= 0) throw new Error(`no "${type}" message arrived`);
      await new Promise<void>((resolve) => {
        const timer = setTimeout(resolve, remaining);
        this.wake = () => {
          clearTimeout(timer);
          resolve();
        };
      });
    }
  }

  /** Messages of `type` queued so far, without consuming them. */
  queued(type: string): Json[] {
    return this.queue.filter((message) => message.type === type);
  }

  close(): void {
    this.ws.close();
  }
}

export const settle = () => new Promise((resolve) => setTimeout(resolve, 100));
