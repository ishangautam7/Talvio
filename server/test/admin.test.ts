import { afterEach, describe, expect, test } from "bun:test";
import { Database } from "bun:sqlite";
import { createHash } from "node:crypto";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { adminEmailsFromEnv } from "../src/admin";
import { randomToken, sha256 } from "../src/crypto";
import { type Charge, FreeLedger, subjectFor } from "../src/free/ledger";
import {
  DESKTOP,
  googleClaims,
  type Harness,
  harness,
  type Json,
  PASSWORD,
  PHONE,
  PUBLIC_URL,
} from "./helpers";

test("ADMIN_EMAILS is a comma separated list, trimmed and lower-cased", () => {
  expect(adminEmailsFromEnv(" A@x.com, ,b@Y.org ")).toEqual([
    "a@x.com",
    "b@y.org",
  ]);
  expect(adminEmailsFromEnv(undefined)).toEqual([]);
});

const ADMIN = "admin-token-for-the-tests-0123456789";
const DAY = 24 * 60 * 60 * 1000;
const SECRET_VALUE = "sk-live-SECRET-VALUE-0123456789";
const OWNER = "owner@example.com";

let h: Harness | undefined;
let db: Database | undefined;
let dir: string | undefined;
afterEach(async () => {
  db?.close();
  db = undefined;
  await h?.close();
  h = undefined;
  if (dir) rmSync(dir, { recursive: true, force: true });
  dir = undefined;
});

/** A server on a database file, plus a second connection that writes the free credit ledger. */
async function setup(
  adminToken: string | null = ADMIN,
  adminEmails: string[] = [OWNER],
) {
  dir = mkdtempSync(join(tmpdir(), "talvio-admin-"));
  const databasePath = join(dir, "talvio.db");
  h = await harness({ adminToken, adminEmails, databasePath });
  db = new Database(databasePath);
  db.run("PRAGMA busy_timeout = 5000");
  const ledger = new FreeLedger(db, 1);
  const charge = (email: string, userId: string, c: Charge, at = h!.now()) =>
    ledger.charge(subjectFor(email), userId, c, at);
  return { charge };
}

const get = (path: string, token: string | null = ADMIN) =>
  h!.request("GET", path, { token: token ?? undefined });

const chat = (costUsd: number): Charge => ({
  kind: "chat",
  model: "upstream-chat-model",
  units: 1500,
  unit: "tokens",
  costUsd,
});
const stt = (costUsd: number): Charge => ({
  kind: "transcribe",
  model: "whisper-large-v3-turbo",
  units: 12.5,
  unit: "seconds",
  costUsd,
});

const ROUTES = [
  "/v1/admin/overview",
  "/v1/admin/users",
  "/v1/admin/users/00000000-0000-0000-0000-000000000000",
];

describe("guard", () => {
  test("every admin route and the page are not_found with neither ADMIN_TOKEN nor ADMIN_EMAILS", async () => {
    await setup(null, []);
    for (const path of [...ROUTES, "/admin", "/v1/admin/feedback"]) {
      const reply = await get(path);
      expect(reply.status).toBe(404);
    }
    for (const path of [
      "/v1/admin/login",
      "/v1/admin/google/exchange",
      "/v1/admin/logout",
    ]) {
      const reply = await h!.request("POST", path, {
        body: { email: OWNER, password: PASSWORD },
      });
      expect(reply.status).toBe(404);
    }
    const start = await get(
      `/v1/admin/google/start?code_challenge=${sha256("v")}&code_challenge_method=S256`,
    );
    expect(start.status).toBe(404);
  });

  test("ADMIN_EMAILS alone turns the dashboard on, and no token is built in", async () => {
    await setup(null);
    expect((await get("/admin", null)).status).toBe(200);
    expect((await get("/v1/admin/overview")).status).toBe(401);
  });

  test("a missing or wrong token is unauthorized, and repeated failures are rate limited", async () => {
    await setup();
    for (const path of ROUTES) {
      expect((await get(path, null)).status).toBe(401);
    }
    // A user's own session token is not the admin token.
    const { token } = await h!.signup();
    expect((await get("/v1/admin/overview", token)).status).toBe(401);
    for (let i = 0; i < 10; i++) await get("/v1/admin/overview", "wrong");
    const limited = await get("/v1/admin/overview");
    expect(limited.status).toBe(429);
    expect(Number(limited.headers.get("retry-after"))).toBeGreaterThan(0);
  });
});

describe("signing in to the dashboard", () => {
  const adminLogin = (email: string, password = PASSWORD) =>
    h!.request("POST", "/v1/admin/login", { body: { email, password } });

  test("an ADMIN_EMAILS account signs in with its own password", async () => {
    await setup(null);
    const owner = await h!.signup(OWNER, DESKTOP);
    const reply = await adminLogin("  Owner@Example.com ");
    expect(reply.status).toBe(200);
    expect(reply.body.token).toStartWith("zva_");
    expect(reply.body.expiresAt).toBe(h!.now() + 12 * 60 * 60 * 1000);
    expect(reply.body.user).toEqual({
      id: owner.user.id,
      email: OWNER,
      name: null,
    });
    const session = reply.body.token as string;
    expect((await get("/v1/admin/overview", session)).status).toBe(200);
    expect((await get("/v1/admin/users", session)).status).toBe(200);
    expect((await get("/v1/admin/feedback", session)).status).toBe(200);

    // Not a device: the apps' device list is unchanged, and it is no app session.
    const devices = await h!.request("GET", "/v1/devices", {
      token: owner.token,
    });
    expect(devices.body.devices).toHaveLength(1);
    expect((await h!.request("GET", "/v1/me", { token: session })).status).toBe(
      401,
    );
    // An app session of the admin account is not an admin sign-in either.
    expect((await get("/v1/admin/overview", owner.token)).status).toBe(401);

    const out = await h!.request("POST", "/v1/admin/logout", {
      token: session,
    });
    expect(out.status).toBe(204);
    expect((await get("/v1/admin/overview", session)).status).toBe(401);
  });

  test("a wrong password, an unknown address and a non-admin account are refused", async () => {
    await setup(null);
    await h!.signup(OWNER);
    await h!.signup("someone@example.com");
    const wrong = await adminLogin(OWNER, "not the password");
    expect(wrong.status).toBe(401);
    expect(wrong.body.error.code).toBe("invalid_credentials");
    const unknown = await adminLogin("nobody@example.com");
    expect(unknown.status).toBe(401);
    expect(unknown.body.error.code).toBe("invalid_credentials");
    const other = await adminLogin("someone@example.com");
    expect(other.status).toBe(403);
    expect(other.body.error.code).toBe("not_admin");
    expect(other.body.token).toBeUndefined();
  });

  test("repeated wrong passwords are limited like the apps' login", async () => {
    await setup(null);
    await h!.signup(OWNER);
    for (let i = 0; i < 10; i++) await adminLogin(OWNER, "wrong password");
    const limited = await adminLogin(OWNER);
    expect(limited.status).toBe(429);
  });

  test("a sign-in ends after 12 hours, and when the account goes", async () => {
    await setup(null);
    const owner = await h!.signup(OWNER);
    const first = (await adminLogin(OWNER)).body.token as string;
    h!.advance(12 * 60 * 60 * 1000);
    expect((await get("/v1/admin/overview", first)).status).toBe(401);

    const second = (await adminLogin(OWNER)).body.token as string;
    expect((await get("/v1/admin/overview", second)).status).toBe(200);
    await h!.request("DELETE", "/v1/me", { token: owner.token });
    expect((await get("/v1/admin/overview", second)).status).toBe(401);
  });

  test("an admin account signs in with Google, coming back to /admin", async () => {
    await setup(null);
    const verifier = randomToken();
    const start = await get(
      `/v1/admin/google/start?${new URLSearchParams({
        code_challenge: sha256(verifier),
        code_challenge_method: "S256",
      })}`,
      null,
    );
    expect(start.status).toBe(302);
    const google = new URL(start.headers.get("location")!);
    expect(google.searchParams.get("redirect_uri")).toBe(
      `${PUBLIC_URL}/v1/auth/google/callback`,
    );
    const back = await get(
      `/v1/auth/google/callback?${new URLSearchParams({
        state: google.searchParams.get("state")!,
        code: h!.google!.issue(googleClaims(h!.now(), { email: OWNER })),
      })}`,
      null,
    );
    expect(back.status).toBe(302);
    const landing = new URL(back.headers.get("location")!);
    expect(landing.origin + landing.pathname).toBe(`${PUBLIC_URL}/admin`);
    const code = landing.searchParams.get("code");

    const exchange = (codeVerifier: string) =>
      h!.request("POST", "/v1/admin/google/exchange", {
        body: { code, codeVerifier },
      });
    const reply = await exchange(verifier);
    expect(reply.status).toBe(200);
    expect(reply.body.user.email).toBe(OWNER);
    expect(
      (await get("/v1/admin/overview", reply.body.token as string)).status,
    ).toBe(200);
    // Used once.
    expect((await exchange(verifier)).body.error.code).toBe("invalid_code");
  });

  test("Google sign-in of an account that is not an admin is refused", async () => {
    await setup(null);
    const start = await get(
      `/v1/admin/google/start?code_challenge=${sha256("verifier")}&code_challenge_method=S256`,
      null,
    );
    const state = new URL(start.headers.get("location")!).searchParams.get(
      "state",
    )!;
    const back = await get(
      `/v1/auth/google/callback?${new URLSearchParams({
        state,
        code: h!.google!.issue(googleClaims(h!.now())),
      })}`,
      null,
    );
    const code = new URL(back.headers.get("location")!).searchParams.get(
      "code",
    );
    const reply = await h!.request("POST", "/v1/admin/google/exchange", {
      body: { code, codeVerifier: "verifier" },
    });
    expect(reply.status).toBe(403);
    expect(reply.body.error.code).toBe("not_admin");
  });

  test("a malformed Google start is refused", async () => {
    await setup(null);
    const reply = await get(
      "/v1/admin/google/start?code_challenge=short&code_challenge_method=S256",
      null,
    );
    expect(reply.status).toBe(400);
  });
});

describe("GET /admin", () => {
  test("serves a static page that runs only its own script and style", async () => {
    await setup();
    await h!.signup("someone@example.com");
    const reply = await get("/admin", null);
    expect(reply.status).toBe(200);
    expect(reply.headers.get("content-type")).toContain("text/html");
    expect(reply.headers.get("cache-control")).toBe("no-store");
    expect(reply.headers.get("x-frame-options")).toBe("DENY");
    // No data without the token.
    expect(reply.text).not.toContain("someone@example.com");

    const csp = reply.headers.get("content-security-policy")!;
    expect(csp).toContain("default-src 'none'");
    expect(csp).toContain("connect-src 'self'");
    expect(csp).not.toContain("unsafe-inline");
    const script = /<script>([\s\S]*)<\/script>/.exec(reply.text)![1]!;
    const style = /<style>([\s\S]*)<\/style>/.exec(reply.text)![1]!;
    const hash = (text: string) =>
      createHash("sha256").update(text, "utf8").digest("base64");
    expect(csp).toContain(`'sha256-${hash(script)}'`);
    expect(csp).toContain(`'sha256-${hash(style)}'`);
    // The inline script at least parses.
    expect(() => new Function(script)).not.toThrow();
    // The token is kept per tab, never in localStorage or a cookie.
    expect(script).toContain("sessionStorage");
    expect(script).not.toContain("localStorage");
    expect(script).not.toContain("document.cookie");
    // Text from other people goes in as text, never as markup.
    expect(script).not.toContain("innerHTML");

    expect((await get("/admin/", null)).status).toBe(200);
  });
});

describe("GET /v1/admin/overview", () => {
  test("counts users, devices, credit per service, days and feedback", async () => {
    const { charge } = await setup();
    const a = await h!.signup("a@example.com", DESKTOP);
    await h!.login("a@example.com", PHONE);
    h!.advance(3 * DAY);
    const b = await h!.signup("b@example.com", DESKTOP);
    h!.advance(5 * DAY);
    await h!.signup("c@example.com", PHONE);

    charge("a@example.com", a.user.id, chat(0.01), h!.now() - 2 * DAY);
    charge("a@example.com", a.user.id, stt(0.002));
    charge("b@example.com", b.user.id, chat(0.03));
    // Older than the 30-day window: counted in totals, not in the days.
    charge("b@example.com", b.user.id, chat(0.5), h!.now() - 40 * DAY);
    await h!.request("POST", "/v1/feedback", {
      body: { message: "Works well" },
    });

    const reply = await get("/v1/admin/overview");
    expect(reply.status).toBe(200);
    const o = reply.body;
    expect(o.generatedAt).toBe(h!.now());
    expect(o.users).toEqual({
      total: 3,
      new24h: 1,
      new7d: 2,
      active24h: 1,
      active7d: 2,
    });
    expect(o.devices).toEqual({ total: 4, desktop: 2, mobile: 2, online: 0 });
    expect(o.credit.limitUsd).toBe(1);
    expect(o.credit.enabled).toBe(false);
    expect(o.credit.spentUsd).toBeCloseTo(0.542, 9);
    expect(o.credit.requests).toBe(4);
    expect(o.credit.accountsWithUsage).toBe(2);
    expect(o.credit.accountsExhausted).toBe(0);
    expect(o.credit.byService.map((s: Json) => s.service)).toEqual([
      "chat",
      "transcribe",
    ]);
    expect(o.credit.byService[0]).toMatchObject({ requests: 3 });
    expect(o.credit.byService[0].costUsd).toBeCloseTo(0.54, 9);
    expect(o.credit.byModel[0]).toMatchObject({
      service: "chat",
      model: "upstream-chat-model",
      requests: 3,
    });

    expect(o.days).toHaveLength(30);
    const today = new Date(h!.now()).toISOString().slice(0, 10);
    expect(o.days.at(-1)).toMatchObject({
      day: today,
      requests: 2,
      signups: 1,
    });
    expect(o.days.at(-1).costUsd).toBeCloseTo(0.032, 9);
    expect(o.days.at(-3)).toMatchObject({ requests: 1, costUsd: 0.01 });
    expect(o.days.reduce((n: number, d: Json) => n + d.requests, 0)).toBe(3);
    expect(o.days.reduce((n: number, d: Json) => n + d.signups, 0)).toBe(3);

    expect(o.feedback).toEqual({
      total: 1,
      last7d: 1,
      url: "https://zv.test/v1/admin/feedback",
    });
  });

  test("an account that used its whole allowance counts as used up", async () => {
    const { charge } = await setup();
    const a = await h!.signup("a@example.com");
    charge("a@example.com", a.user.id, chat(1));
    const o = (await get("/v1/admin/overview")).body;
    expect(o.credit.accountsExhausted).toBe(1);
  });

  test("shows a device that is connected right now as online", async () => {
    await setup();
    const a = await h!.signup("a@example.com");
    const client = await h!.connect(a.token);
    await client.next("devices");
    const o = (await get("/v1/admin/overview")).body;
    expect(o.devices.online).toBe(1);
    const detail = (await get(`/v1/admin/users/${a.user.id}`)).body;
    expect(detail.devices[0].online).toBe(true);
    client.close();
  });
});

describe("GET /v1/admin/users", () => {
  test("lists users newest first with devices, credit and services", async () => {
    const { charge } = await setup();
    const a = await h!.signup("a@example.com", DESKTOP);
    await h!.login("a@example.com", PHONE);
    h!.advance(1000);
    await h!.signup("b@example.com", PHONE);
    charge("a@example.com", a.user.id, chat(0.25));
    charge("a@example.com", a.user.id, stt(0.05));

    const reply = await get("/v1/admin/users");
    expect(reply.status).toBe(200);
    expect(reply.body.total).toBe(2);
    expect(reply.body.next).toBeNull();
    const [b, first] = reply.body.users;
    expect(b.email).toBe("b@example.com");
    expect(b.credit).toMatchObject({
      usedUsd: 0,
      remainingUsd: 1,
      requests: 0,
    });
    expect(b.services).toEqual([]);
    expect(first).toMatchObject({
      id: a.user.id,
      email: "a@example.com",
      createdAt: Date.UTC(2026, 0, 1),
      lastSeenAt: Date.UTC(2026, 0, 1),
      devices: { total: 2, desktop: 1, mobile: 1 },
      credit: { limitUsd: 1, usedUsd: 0.3, remainingUsd: 0.7, requests: 2 },
      services: ["chat", "transcribe"],
    });
  });

  test("searches by email, name and id, with LIKE wildcards taken literally", async () => {
    await setup();
    await h!.request("POST", "/v1/auth/signup", {
      body: {
        email: "ram@example.com",
        password: "correct horse",
        name: "Ram Sharma",
        device: DESKTOP,
      },
    });
    const sita = await h!.signup("sita@other.org");
    await h!.signup("x_y@example.com");

    const emails = async (q: string) =>
      (await get(`/v1/admin/users?q=${encodeURIComponent(q)}`)).body.users.map(
        (u: Json) => u.email,
      );
    expect(await emails("other")).toEqual(["sita@other.org"]);
    expect(await emails("SHARMA")).toEqual(["ram@example.com"]);
    expect(await emails(sita.user.id)).toEqual(["sita@other.org"]);
    expect(await emails("_")).toEqual(["x_y@example.com"]);
    expect(await emails("%")).toEqual([]);
    expect((await get("/v1/admin/users?q=example")).body.total).toBe(2);
  });

  test("pages with limit and offset, and sorts", async () => {
    await setup();
    for (const name of ["c", "a", "b"]) {
      await h!.signup(`${name}@example.com`);
      h!.advance(1000);
    }
    const page1 = (await get("/v1/admin/users?limit=2")).body;
    expect(page1.users.map((u: Json) => u.email)).toEqual([
      "b@example.com",
      "a@example.com",
    ]);
    expect(page1.next).toBe(2);
    const page2 = (await get(`/v1/admin/users?limit=2&offset=${page1.next}`))
      .body;
    expect(page2.users.map((u: Json) => u.email)).toEqual(["c@example.com"]);
    expect(page2.next).toBeNull();
    const byEmail = (await get("/v1/admin/users?sort=email")).body;
    expect(byEmail.users.map((u: Json) => u.email)).toEqual([
      "a@example.com",
      "b@example.com",
      "c@example.com",
    ]);
    expect((await get("/v1/admin/users?sort=seen")).status).toBe(200);

    for (const bad of [
      "limit=0",
      "limit=101",
      "limit=x",
      "offset=-1",
      "sort=password_hash",
      `q=${"a".repeat(201)}`,
    ]) {
      const reply = await get(`/v1/admin/users?${bad}`);
      expect(reply.status).toBe(400);
      expect(reply.body.error.code).toBe("invalid_request");
    }
  });
});

describe("GET /v1/admin/users/{id}", () => {
  test("shows one account's devices, services and recent requests", async () => {
    const { charge } = await setup();
    const a = await h!.signup("a@example.com", DESKTOP);
    h!.advance(60_000);
    await h!.login("a@example.com", PHONE);
    charge("a@example.com", a.user.id, chat(0.01), h!.now() - 1000);
    charge("a@example.com", a.user.id, stt(0.002));
    await h!.request("POST", "/v1/feedback", {
      body: { message: "Signed-in feedback" },
      token: a.token,
    });

    const reply = await get(`/v1/admin/users/${a.user.id}`);
    expect(reply.status).toBe(200);
    const d = reply.body;
    expect(d.user).toEqual({
      id: a.user.id,
      email: "a@example.com",
      name: null,
      createdAt: Date.UTC(2026, 0, 1),
      emailVerified: false,
      hasPassword: true,
      googleLinked: false,
      lastSeenAt: h!.now(),
    });
    expect(d.devices.map((x: Json) => x.kind)).toEqual(["mobile", "desktop"]);
    expect(d.devices[1]).toMatchObject({
      name: "ISHAN-PC",
      platform: "windows",
      kind: "desktop",
      appVersion: "2.2.0",
      online: false,
    });
    expect(d.credit).toMatchObject({ usedUsd: 0.012, requests: 2 });
    expect(d.services.map((s: Json) => s.service)).toEqual([
      "chat",
      "transcribe",
    ]);
    expect(d.recentRequests[0]).toEqual({
      service: "transcribe",
      model: "whisper-large-v3-turbo",
      units: 12.5,
      unit: "seconds",
      costUsd: 0.002,
      at: h!.now(),
    });
    expect(d.feedback).toBe(1);
  });

  test("credit follows the email, as the ledger does, across a new sign-up", async () => {
    const { charge } = await setup();
    const first = await h!.signup("a@example.com");
    charge("a@example.com", first.user.id, chat(0.4));
    await h!.request("DELETE", "/v1/me", { token: first.token });
    const again = await h!.signup("a@example.com");
    const d = (await get(`/v1/admin/users/${again.user.id}`)).body;
    expect(d.credit).toMatchObject({ usedUsd: 0.4, remainingUsd: 0.6 });
  });

  test("an unknown id is not_found", async () => {
    await setup();
    const reply = await get("/v1/admin/users/nobody");
    expect(reply.status).toBe(404);
    expect(reply.body.error.code).toBe("not_found");
  });
});

test("no admin answer carries a session token, a hash or a stored API key", async () => {
  const { charge } = await setup();
  const a = await h!.signup("a@example.com");
  charge("a@example.com", a.user.id, chat(0.01));
  const put = await h!.request("PUT", "/v1/secrets/post_process:openai", {
    token: a.token,
    body: { value: SECRET_VALUE, updatedAt: h!.now() },
  });
  expect(put.status).toBe(200);
  const passwordHash = db!
    .query<{ password_hash: string }, [string]>(
      "SELECT password_hash FROM users WHERE id = ?",
    )
    .get(a.user.id)!.password_hash;
  const tokenHash = sha256(a.token);
  await h!.signup(OWNER);
  const session = (
    await h!.request("POST", "/v1/admin/login", {
      body: { email: OWNER, password: PASSWORD },
    })
  ).body.token as string;

  const texts = [
    (await get("/v1/admin/overview")).text,
    (await get("/v1/admin/users")).text,
    (await get(`/v1/admin/users/${a.user.id}`)).text,
    (await get("/v1/admin/users?q=a")).text,
    (await get("/v1/admin/users", session)).text,
  ];
  for (const text of texts) {
    expect(text).not.toContain(a.token);
    expect(text).not.toContain(session);
    expect(text).not.toContain(tokenHash);
    expect(text).not.toContain(passwordHash);
    expect(text).not.toContain(SECRET_VALUE);
    expect(text).not.toContain("post_process:openai");
    expect(text).not.toContain(subjectFor("a@example.com"));
    // No field is named after a token, hash, secret or ledger subject.
    expect(text).not.toMatch(
      /"[A-Za-z]*(token|hash|secret|subject)[A-Za-z]*":/i,
    );
  }
});
