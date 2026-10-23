import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { DESKTOP, type Harness, harness, PASSWORD, PHONE } from "./helpers";
import { addressKey } from "../src/rateLimit";

const DAY = 24 * 60 * 60 * 1000;
const TOKEN = /^zv1_[A-Za-z0-9_-]{43}$/;

let h: Harness;
beforeEach(async () => {
  h = await harness();
});
afterEach(() => h.close());

describe("public routes", () => {
  test("health and config", async () => {
    expect((await h.request("GET", "/v1/health")).body).toEqual({ ok: true });
    expect((await h.request("GET", "/v1/config")).body).toEqual({
      protocol: 1,
      google: true,
    });
  });

  test("config says google is off without credentials", async () => {
    const off = await harness({ google: false });
    expect((await off.request("GET", "/v1/config")).body).toEqual({
      protocol: 1,
      google: false,
    });
    await off.close();
  });

  test("unknown routes and wrong methods are not_found", async () => {
    for (const [method, path] of [
      ["GET", "/nope"],
      ["POST", "/v1/health"],
      ["GET", "/v1/devices/x/y"],
    ] as const) {
      const reply = await h.request(method, path);
      expect(reply.status).toBe(404);
      expect(reply.body.error.code).toBe("not_found");
    }
  });
});

describe("signup", () => {
  test("creates the account and signs this device in", async () => {
    const reply = await h.request("POST", "/v1/auth/signup", {
      body: {
        email: "  Ishan@Example.COM ",
        password: PASSWORD,
        name: " Ishan ",
        device: DESKTOP,
      },
    });
    expect(reply.status).toBe(201);
    expect(reply.body.token).toMatch(TOKEN);
    expect(reply.body.user).toEqual({
      id: expect.any(String),
      email: "ishan@example.com",
      name: "Ishan",
      hasPassword: true,
      googleLinked: false,
      createdAt: h.now(),
    });
    expect(reply.body.device).toEqual({
      id: expect.any(String),
      ...DESKTOP,
      online: false,
      lastSeenAt: h.now(),
      createdAt: h.now(),
      current: true,
      capabilities: [],
    });
  });

  test("rejects a taken email, whatever its case", async () => {
    await h.signup("a@example.com");
    const reply = await h.request("POST", "/v1/auth/signup", {
      body: { email: "A@EXAMPLE.com", password: PASSWORD, device: DESKTOP },
    });
    expect(reply.status).toBe(409);
    expect(reply.body.error.code).toBe("email_taken");
  });

  test.each([
    ["not an address", { email: "nope" }, "invalid_email"],
    ["no domain dot", { email: "a@b" }, "invalid_email"],
    ["short password", { password: "1234567" }, "weak_password"],
    ["long password", { password: "x".repeat(257) }, "weak_password"],
    ["missing email", { email: undefined }, "invalid_request"],
    ["missing device", { device: undefined }, "invalid_request"],
    [
      "blank device name",
      { device: { ...DESKTOP, name: "   " } },
      "invalid_request",
    ],
    [
      "long device name",
      { device: { ...DESKTOP, name: "x".repeat(65) } },
      "invalid_request",
    ],
    [
      "bad platform",
      { device: { ...DESKTOP, platform: "beos" } },
      "invalid_request",
    ],
    ["bad kind", { device: { ...DESKTOP, kind: "watch" } }, "invalid_request"],
    [
      "long appVersion",
      { device: { ...DESKTOP, appVersion: "1".repeat(33) } },
      "invalid_request",
    ],
    ["long name", { name: "x".repeat(65) }, "invalid_request"],
  ])("%s", async (_label, change, code) => {
    const body = {
      email: "a@example.com",
      password: PASSWORD,
      device: DESKTOP,
      ...change,
    };
    const reply = await h.request("POST", "/v1/auth/signup", { body });
    expect(reply.status).toBe(400);
    expect(reply.body.error.code).toBe(code);
    expect(reply.body.error.message).toEqual(expect.any(String));
  });

  test("accepts exactly 8 and 256 characters, counted as characters", async () => {
    for (const [email, password] of [
      ["eight@example.com", "😀".repeat(8)],
      ["max@example.com", "x".repeat(256)],
    ]) {
      const reply = await h.request("POST", "/v1/auth/signup", {
        body: { email, password, device: PHONE },
      });
      expect(reply.status).toBe(201);
    }
  });

  test("malformed JSON and non-object bodies are invalid_request", async () => {
    for (const body of ["{", "[]", "null", ""]) {
      const reply = await h.request("POST", "/v1/auth/signup", { body });
      expect(reply.status).toBe(400);
      expect(reply.body.error.code).toBe("invalid_request");
    }
  });

  test("bodies over 64 KiB are refused, with or without Content-Length", async () => {
    const big = JSON.stringify({
      email: "a@example.com",
      pad: "x".repeat(64 * 1024),
    });
    const sized = await h.request("POST", "/v1/auth/signup", { body: big });
    expect(sized.status).toBe(413);
    expect(sized.body.error.code).toBe("payload_too_large");

    const chunked = await fetch(`${h.base}/v1/auth/signup`, {
      method: "POST",
      body: new ReadableStream({
        start(controller) {
          controller.enqueue(new TextEncoder().encode(big));
          controller.close();
        },
      }),
    });
    expect(chunked.status).toBe(413);
    expect(
      ((await chunked.json()) as { error: { code: string } }).error.code,
    ).toBe("payload_too_large");
  });
});

describe("login", () => {
  test("signs in a new device with the right password", async () => {
    const first = await h.signup();
    const reply = await h.request("POST", "/v1/auth/login", {
      body: { email: " A@example.com", password: PASSWORD, device: PHONE },
    });
    expect(reply.status).toBe(200);
    expect(reply.body.token).toMatch(TOKEN);
    expect(reply.body.user.id).toBe(first.user.id);
    expect(reply.body.device.id).not.toBe(first.device.id);
    expect(reply.body.device).toMatchObject({
      ...PHONE,
      appVersion: null,
      current: true,
    });
  });

  test("wrong password and unknown email are both invalid_credentials", async () => {
    await h.signup();
    for (const body of [
      { email: "a@example.com", password: "wrong password", device: PHONE },
      { email: "nobody@example.com", password: PASSWORD, device: PHONE },
    ]) {
      const reply = await h.request("POST", "/v1/auth/login", { body });
      expect(reply.status).toBe(401);
      expect(reply.body.error.code).toBe("invalid_credentials");
    }
  });

  test("missing password is invalid_request, a malformed email invalid_email", async () => {
    const missing = await h.request("POST", "/v1/auth/login", {
      body: { email: "a@example.com", device: PHONE },
    });
    expect(missing.body.error.code).toBe("invalid_request");
    const malformed = await h.request("POST", "/v1/auth/login", {
      body: { email: "a", password: PASSWORD, device: PHONE },
    });
    expect(malformed.body.error.code).toBe("invalid_email");
  });
});

describe("sessions", () => {
  test("me answers the user and this device", async () => {
    const { token, user, device } = await h.signup();
    const reply = await h.request("GET", "/v1/me", { token });
    expect(reply.status).toBe(200);
    expect(reply.body).toEqual({ user, device });
  });

  test("a missing, malformed or unknown token is unauthorized", async () => {
    for (const headers of [
      {} as Record<string, string>,
      { authorization: "Basic abc" },
      {
        authorization: "Bearer zv1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
      },
    ]) {
      const reply = await h.request("GET", "/v1/me", { headers });
      expect(reply.status).toBe(401);
      expect(reply.body.error.code).toBe("unauthorized");
    }
  });

  test("logout deletes this device and its token", async () => {
    const desktop = await h.signup();
    const phone = await h.login();
    expect(
      (await h.request("POST", "/v1/auth/logout", { token: phone.token }))
        .status,
    ).toBe(204);
    expect(
      (await h.request("GET", "/v1/me", { token: phone.token })).status,
    ).toBe(401);
    const list = await h.request("GET", "/v1/devices", {
      token: desktop.token,
    });
    expect(list.body.devices.map((d: { id: string }) => d.id)).toEqual([
      desktop.device.id,
    ]);
  });

  test("a token unused for 180 days is gone; one in use stays", async () => {
    const idle = await h.signup("idle@example.com");
    const busy = await h.signup("busy@example.com");
    h.advance(179 * DAY);
    expect(
      (await h.request("GET", "/v1/me", { token: busy.token })).status,
    ).toBe(200);
    h.advance(DAY);
    expect(
      (await h.request("GET", "/v1/me", { token: idle.token })).status,
    ).toBe(401);
    expect(
      (await h.request("GET", "/v1/me", { token: busy.token })).status,
    ).toBe(200);
  });

  test("lastSeenAt is written at most once a minute", async () => {
    const { token } = await h.signup();
    const created = h.now();
    h.advance(30 * 1000);
    expect(
      (await h.request("GET", "/v1/me", { token })).body.device.lastSeenAt,
    ).toBe(created);
    h.advance(31 * 1000);
    expect(
      (await h.request("GET", "/v1/me", { token })).body.device.lastSeenAt,
    ).toBe(h.now());
  });

  test("an expired device drops out of the device list", async () => {
    const old = await h.signup();
    h.advance(100 * DAY);
    const fresh = await h.login();
    h.advance(81 * DAY);
    const list = await h.request("GET", "/v1/devices", { token: fresh.token });
    expect(list.body.devices.map((d: { id: string }) => d.id)).toEqual([
      fresh.device.id,
    ]);
    expect(
      (await h.request("GET", "/v1/me", { token: old.token })).status,
    ).toBe(401);
  });
});

describe("devices", () => {
  test("lists the current device first, then by name", async () => {
    const zed = await h.signup("a@example.com", { ...DESKTOP, name: "Zed" });
    await h.login("a@example.com", { ...PHONE, name: "beta" });
    await h.login("a@example.com", { ...PHONE, name: "Alpha" });
    const reply = await h.request("GET", "/v1/devices", { token: zed.token });
    expect(reply.status).toBe(200);
    expect(reply.body.devices.map((d: { name: string }) => d.name)).toEqual([
      "Zed",
      "Alpha",
      "beta",
    ]);
    expect(
      reply.body.devices.map((d: { current: boolean }) => d.current),
    ).toEqual([true, false, false]);
  });

  test("delete signs that device out", async () => {
    const desktop = await h.signup();
    const phone = await h.login();
    const reply = await h.request("DELETE", `/v1/devices/${phone.device.id}`, {
      token: desktop.token,
    });
    expect(reply.status).toBe(204);
    expect(
      (await h.request("GET", "/v1/me", { token: phone.token })).status,
    ).toBe(401);
    expect(
      (await h.request("GET", "/v1/me", { token: desktop.token })).status,
    ).toBe(200);
  });

  test("delete is not_found for an unknown device or another account's", async () => {
    const mine = await h.signup("a@example.com");
    const theirs = await h.signup("b@example.com");
    for (const id of ["no-such-device", theirs.device.id]) {
      const reply = await h.request("DELETE", `/v1/devices/${id}`, {
        token: mine.token,
      });
      expect(reply.status).toBe(404);
      expect(reply.body.error.code).toBe("not_found");
    }
    expect(
      (await h.request("GET", "/v1/me", { token: theirs.token })).status,
    ).toBe(200);
  });

  test("delete without a token is unauthorized", async () => {
    const { device } = await h.signup();
    expect((await h.request("DELETE", `/v1/devices/${device.id}`)).status).toBe(
      401,
    );
  });
});

describe("delete account", () => {
  test("removes the account and every session, freeing the email", async () => {
    const desktop = await h.signup();
    const phone = await h.login();
    expect(
      (await h.request("DELETE", "/v1/me", { token: phone.token })).status,
    ).toBe(204);
    for (const token of [desktop.token, phone.token]) {
      expect((await h.request("GET", "/v1/me", { token })).status).toBe(401);
    }
    const again = await h.signup();
    expect(again.user.id).not.toBe(desktop.user.id);
  });
});

describe("rate limits", () => {
  test("20 auth requests per minute per IP, then 429 with Retry-After", async () => {
    for (let i = 0; i < 20; i++) {
      expect(
        (await h.request("POST", "/v1/auth/login", { body: {} })).status,
      ).toBe(400);
    }
    const limited = await h.request("POST", "/v1/auth/signup", { body: {} });
    expect(limited.status).toBe(429);
    expect(limited.body.error.code).toBe("rate_limited");
    expect(limited.headers.get("retry-after")).toBe("60");
    h.advance(30 * 1000);
    expect((await h.request("GET", "/v1/auth/google/start")).status).toBe(429);
    h.advance(30 * 1000);
    expect(
      (await h.request("POST", "/v1/auth/login", { body: {} })).status,
    ).toBe(400);
  });

  test("authenticated and public routes are not counted", async () => {
    const { token } = await h.signup();
    for (let i = 0; i < 25; i++) {
      expect((await h.request("GET", "/v1/me", { token })).status).toBe(200);
    }
    expect((await h.request("GET", "/v1/health")).status).toBe(200);
  });

  test("X-Forwarded-For is ignored unless the proxy is trusted", async () => {
    for (let i = 0; i < 20; i++) {
      await h.request("POST", "/v1/auth/login", {
        body: {},
        headers: { "x-forwarded-for": `10.0.0.${i}` },
      });
    }
    const reply = await h.request("POST", "/v1/auth/login", {
      body: {},
      headers: { "x-forwarded-for": "10.0.1.1" },
    });
    expect(reply.status).toBe(429);
  });

  test("10 failed logins per 15 minutes per email, from any IP", async () => {
    const proxied = await harness({ trustProxy: true });
    await proxied.signup();
    const attempt = (password: string, ip: string) =>
      proxied.request("POST", "/v1/auth/login", {
        body: { email: "A@example.com", password, device: PHONE },
        headers: { "x-forwarded-for": `203.0.113.9, ${ip}` },
      });
    for (let i = 0; i < 10; i++) {
      expect((await attempt("wrong password", `10.0.0.${i}`)).status).toBe(401);
    }
    const locked = await attempt(PASSWORD, "10.0.1.1");
    expect(locked.status).toBe(429);
    expect(locked.body.error.code).toBe("rate_limited");
    expect(locked.headers.get("retry-after")).toBe(String(15 * 60));

    const other = await proxied.request("POST", "/v1/auth/signup", {
      body: { email: "b@example.com", password: PASSWORD, device: PHONE },
      headers: { "x-forwarded-for": "10.0.1.2" },
    });
    expect(other.status).toBe(201);

    proxied.advance(15 * 60 * 1000);
    expect((await attempt(PASSWORD, "10.0.1.3")).status).toBe(200);
    await proxied.close();
  });

  test("wrong logins sent at once still stop at 10", async () => {
    const proxied = await harness({ trustProxy: true });
    await proxied.signup();
    const replies = await Promise.all(
      Array.from({ length: 15 }, (_, i) =>
        proxied.request("POST", "/v1/auth/login", {
          body: { email: "a@example.com", password: "wrong", device: PHONE },
          headers: { "x-forwarded-for": `10.0.0.${i}` },
        }),
      ),
    );
    expect(replies.map((reply) => reply.status).sort()).toEqual([
      ...Array(10).fill(401),
      ...Array(5).fill(429),
    ]);
    await proxied.close();
  });

  test("a correct login does not count toward the failed-login limit", async () => {
    const proxied = await harness({ trustProxy: true });
    await proxied.signup();
    const attempt = (password: string, i: number) =>
      proxied.request("POST", "/v1/auth/login", {
        body: { email: "a@example.com", password, device: PHONE },
        headers: { "x-forwarded-for": `10.0.0.${i}` },
      });
    for (let i = 0; i < 9; i++) await attempt("wrong", i);
    expect((await attempt(PASSWORD, 9)).status).toBe(200);
    expect((await attempt("wrong", 10)).status).toBe(401);
    expect((await attempt(PASSWORD, 11)).status).toBe(429);
    await proxied.close();
  });

  test("an IPv6 /64 is one address", async () => {
    const proxied = await harness({ trustProxy: true });
    const from = (ip: string) =>
      proxied.request("POST", "/v1/auth/login", {
        body: {},
        headers: { "x-forwarded-for": ip },
      });
    for (let i = 0; i < 20; i++) {
      expect((await from(`2001:db8:0:1::${i.toString(16)}`)).status).toBe(400);
    }
    expect((await from("2001:db8:0:1:ffff:1:2:3")).status).toBe(429);
    expect((await from("2001:db8:0:2::1")).status).toBe(400);
    await proxied.close();
  });

  test.each([
    ["203.0.113.9", "203.0.113.9"],
    ["::ffff:203.0.113.9", "203.0.113.9"],
    ["2001:DB8:0:1:2:3:4:5", "2001:db8:0:1::/64"],
    ["2001:0db8::1", "2001:db8:0:0::/64"],
    ["::1", "0:0:0:0::/64"],
    ["::1:2:3:4:5:6:7", "0:1:2:3::/64"],
    ["unknown", "unknown"],
  ])("limits %p under %p", (address, key) => {
    expect(addressKey(address)).toBe(key);
  });
});
