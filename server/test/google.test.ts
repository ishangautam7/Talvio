import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { randomToken, sha256 } from "../src/crypto";
import {
  CLIENT_ID,
  DESKTOP,
  type Harness,
  harness,
  googleClaims,
  type Json,
  PASSWORD,
  PHONE,
  PUBLIC_URL,
} from "./helpers";

const PHONE_REDIRECT = "zynveliq://auth/callback";

let h: Harness;
beforeEach(async () => {
  h = await harness();
});
afterEach(() => h.close());

function startPath(
  redirectUri: string,
  challenge: string,
  method = "S256",
): string {
  const query = new URLSearchParams({
    redirect_uri: redirectUri,
    code_challenge: challenge,
    code_challenge_method: method,
  });
  return `/v1/auth/google/start?${query}`;
}

/** Runs start → Google → callback and answers where the browser was sent back to. */
async function throughGoogle(
  claims: Json | number = googleClaims(h.now()),
  redirectUri = PHONE_REDIRECT,
) {
  const verifier = randomToken();
  const start = await h.request(
    "GET",
    startPath(redirectUri, sha256(verifier)),
  );
  expect(start.status).toBe(302);
  const state = new URL(start.headers.get("location")!).searchParams.get(
    "state",
  )!;
  const back = await h.request(
    "GET",
    `/v1/auth/google/callback?${new URLSearchParams({ state, code: h.google!.issue(claims) })}`,
  );
  expect(back.status).toBe(302);
  const location = back.headers.get("location")!;
  return {
    verifier,
    state,
    location,
    code: new URL(location).searchParams.get("code"),
  };
}

const exchange = (
  code: string | null,
  codeVerifier: string,
  device: object = PHONE,
) =>
  h.request("POST", "/v1/auth/exchange", {
    body: { code, codeVerifier, device },
  });

async function signInWithGoogle(
  claims: Json = googleClaims(h.now()),
): Promise<Json> {
  const { code, verifier } = await throughGoogle(claims);
  const reply = await exchange(code, verifier);
  expect(reply.status).toBe(200);
  return reply.body;
}

describe("start", () => {
  test("sends the browser to Google with this server's own callback", async () => {
    const reply = await h.request(
      "GET",
      startPath(PHONE_REDIRECT, sha256(randomToken())),
    );
    expect(reply.status).toBe(302);
    const google = new URL(reply.headers.get("location")!);
    expect(google.origin + google.pathname).toBe(
      "https://accounts.google.com/o/oauth2/v2/auth",
    );
    expect(Object.fromEntries(google.searchParams)).toEqual({
      client_id: CLIENT_ID,
      redirect_uri: `${PUBLIC_URL}/v1/auth/google/callback`,
      response_type: "code",
      scope: "openid email profile",
      prompt: "select_account",
      state: expect.stringMatching(/^[A-Za-z0-9_-]{43}$/),
    });
  });

  test("accepts a desktop loopback redirect", async () => {
    for (const port of [1024, 53682, 65535]) {
      const redirectUri = `http://127.0.0.1:${port}/callback`;
      const { location } = await throughGoogle(
        googleClaims(h.now()),
        redirectUri,
      );
      expect(location).toStartWith(`${redirectUri}?code=`);
    }
  });

  test.each([
    "",
    "http://127.0.0.1:1023/callback",
    "http://127.0.0.1:65536/callback",
    "http://127.0.0.1:05000/callback",
    "http://127.0.0.1/callback",
    "http://localhost:5000/callback",
    "https://127.0.0.1:5000/callback",
    "http://127.0.0.1:5000/callback/",
    "http://127.0.0.1:5000/callback?x=1",
    "http://127.0.0.1:5000@evil.example/callback",
    "zynveliq://auth/callback/",
    "zynveliq://auth/callback?next=x",
    "ZYNVELIQ://auth/callback",
    "https://evil.example/callback",
  ])(
    "refuses redirect_uri %p with a page and no redirect",
    async (redirectUri) => {
      const reply = await h.request(
        "GET",
        startPath(redirectUri, sha256(randomToken())),
      );
      expect(reply.status).toBe(400);
      expect(reply.headers.get("location")).toBeNull();
      expect(reply.headers.get("content-type")).toContain("text/html");
    },
  );

  test("refuses a missing or non-S256 challenge", async () => {
    for (const path of [
      startPath(PHONE_REDIRECT, ""),
      startPath(PHONE_REDIRECT, "short"),
      startPath(PHONE_REDIRECT, sha256("x"), "plain"),
    ]) {
      const reply = await h.request("GET", path);
      expect(reply.status).toBe(400);
      expect(reply.headers.get("location")).toBeNull();
    }
  });

  test("without Google credentials, redirects back with google_unavailable", async () => {
    const off = await harness({ google: false });
    const ok = await off.request("GET", startPath(PHONE_REDIRECT, sha256("v")));
    expect(ok.status).toBe(302);
    expect(ok.headers.get("location")).toBe(
      `${PHONE_REDIRECT}?error=google_unavailable`,
    );
    const bad = await off.request(
      "GET",
      startPath("https://evil.example/", sha256("v")),
    );
    expect(bad.status).toBe(400);
    await off.close();
  });
});

describe("callback and exchange", () => {
  test("a new Google user gets an account named by Google", async () => {
    const before = h.google!.requests.length;
    const auth = await signInWithGoogle();
    expect(auth.token).toMatch(/^zv1_/);
    expect(auth.user).toMatchObject({
      email: "a@example.com",
      name: "Ishan Gautam",
      hasPassword: false,
      googleLinked: true,
    });
    expect(auth.device).toMatchObject({ ...PHONE, current: true });
    expect(
      (await h.request("GET", "/v1/me", { token: auth.token })).status,
    ).toBe(200);

    const sent = h.google!.requests[before]!;
    expect(sent.get("client_secret")).toBe("shh");
    expect(sent.get("redirect_uri")).toBe(
      `${PUBLIC_URL}/v1/auth/google/callback`,
    );
    expect(sent.get("grant_type")).toBe("authorization_code");
  });

  test("a returning Google user is found by Google id, even after an email change", async () => {
    const first = await signInWithGoogle();
    const again = await signInWithGoogle(
      googleClaims(h.now(), { email: "new@example.com" }),
    );
    expect(again.user.id).toBe(first.user.id);
    expect(
      (await h.request("GET", "/v1/me", { token: first.token })).status,
    ).toBe(200);
  });

  test("cancelling at Google sends the user back with access_denied", async () => {
    const start = await h.request(
      "GET",
      startPath(PHONE_REDIRECT, sha256("v")),
    );
    const state = new URL(start.headers.get("location")!).searchParams.get(
      "state",
    )!;
    const back = await h.request(
      "GET",
      `/v1/auth/google/callback?state=${state}&error=access_denied`,
    );
    expect(back.headers.get("location")).toBe(
      `${PHONE_REDIRECT}?error=access_denied`,
    );
  });

  test.each([
    ["any other Google error", "error=server_error"],
    ["no code", ""],
    ["a code Google refuses", "code=unknown"],
  ])("%s is google_failed", async (_label, query) => {
    const start = await h.request(
      "GET",
      startPath(PHONE_REDIRECT, sha256("v")),
    );
    const state = new URL(start.headers.get("location")!).searchParams.get(
      "state",
    )!;
    const back = await h.request(
      "GET",
      `/v1/auth/google/callback?state=${state}&${query}`,
    );
    expect(back.headers.get("location")).toBe(
      `${PHONE_REDIRECT}?error=google_failed`,
    );
  });

  test.each([
    ["wrong audience", { aud: "someone-else" }],
    ["wrong issuer", { iss: "https://evil.example" }],
    ["expired", { exp: 1 }],
    ["unverified email", { email_verified: false }],
    ["no subject", { sub: undefined }],
  ])("an id_token with %s is google_failed", async (_label, change) => {
    const { location } = await throughGoogle(googleClaims(h.now(), change));
    expect(location).toBe(`${PHONE_REDIRECT}?error=google_failed`);
  });

  test("a Google token endpoint error is google_failed", async () => {
    const { location } = await throughGoogle(500);
    expect(location).toBe(`${PHONE_REDIRECT}?error=google_failed`);
  });

  test("accepts the bare issuer and an audience list", async () => {
    const { location } = await throughGoogle(
      googleClaims(h.now(), {
        iss: "accounts.google.com",
        aud: ["other", CLIENT_ID],
      }),
    );
    expect(location).toStartWith(`${PHONE_REDIRECT}?code=`);
  });

  test("the callback still sends the browser back when this IP is over its limit", async () => {
    const start = await h.request(
      "GET",
      startPath(PHONE_REDIRECT, sha256(randomToken())),
    );
    const state = new URL(start.headers.get("location")!).searchParams.get(
      "state",
    )!;
    for (let i = 0; i < 20; i++) {
      await h.request("POST", "/v1/auth/login", { body: {} });
    }
    const back = await h.request(
      "GET",
      `/v1/auth/google/callback?${new URLSearchParams({ state, code: h.google!.issue(googleClaims(h.now())) })}`,
    );
    expect(back.headers.get("location")).toStartWith(`${PHONE_REDIRECT}?code=`);
  });

  test("a state works once", async () => {
    const { state } = await throughGoogle();
    const reused = await h.request(
      "GET",
      `/v1/auth/google/callback?state=${state}&code=${h.google!.issue(googleClaims(h.now()))}`,
    );
    expect(reused.status).toBe(400);
    expect(reused.headers.get("location")).toBeNull();
  });

  test("a state expires after 10 minutes", async () => {
    const start = await h.request(
      "GET",
      startPath(PHONE_REDIRECT, sha256("v")),
    );
    const state = new URL(start.headers.get("location")!).searchParams.get(
      "state",
    )!;
    h.advance(10 * 60 * 1000);
    const late = await h.request(
      "GET",
      `/v1/auth/google/callback?state=${state}&code=${h.google!.issue(googleClaims(h.now()))}`,
    );
    expect(late.status).toBe(400);
  });

  test("an unknown state is a page, not a redirect", async () => {
    const reply = await h.request(
      "GET",
      "/v1/auth/google/callback?state=forged&code=x",
    );
    expect(reply.status).toBe(400);
    expect(reply.headers.get("location")).toBeNull();
  });

  test("a login code works once", async () => {
    const { code, verifier } = await throughGoogle();
    expect((await exchange(code, verifier)).status).toBe(200);
    const reused = await exchange(code, verifier);
    expect(reused.status).toBe(400);
    expect(reused.body.error.code).toBe("invalid_code");
  });

  test("a wrong verifier fails, and spends the code", async () => {
    const { code, verifier } = await throughGoogle();
    const wrong = await exchange(code, randomToken());
    expect(wrong.status).toBe(400);
    expect(wrong.body.error.code).toBe("invalid_code");
    expect((await exchange(code, verifier)).body.error.code).toBe(
      "invalid_code",
    );
  });

  test("a login code expires after 2 minutes", async () => {
    const { code, verifier } = await throughGoogle();
    h.advance(2 * 60 * 1000);
    expect((await exchange(code, verifier)).body.error.code).toBe(
      "invalid_code",
    );
  });

  test("an unknown code is invalid_code; missing fields are invalid_request", async () => {
    expect((await exchange("nope", "nope")).body.error.code).toBe(
      "invalid_code",
    );
    expect((await exchange(null, "nope")).body.error.code).toBe(
      "invalid_request",
    );
    const { code, verifier } = await throughGoogle();
    expect(
      (await exchange(code, verifier, { name: "x" })).body.error.code,
    ).toBe("invalid_request");
  });
});

describe("linking", () => {
  test("Google takes over an unverified password account", async () => {
    const squatter = await h.signup("a@example.com", DESKTOP);
    const socket = await h.connect(squatter.token);
    await socket.next("ready");

    const owner = await signInWithGoogle();
    expect(owner.user).toMatchObject({
      id: squatter.user.id,
      hasPassword: false,
      googleLinked: true,
    });
    expect((await socket.closed).code).toBe(4401);
    expect(
      (await h.request("GET", "/v1/me", { token: squatter.token })).status,
    ).toBe(401);
    const login = await h.request("POST", "/v1/auth/login", {
      body: { email: "a@example.com", password: PASSWORD, device: PHONE },
    });
    expect(login.status).toBe(401);
    expect(login.body.error.code).toBe("invalid_credentials");
    const devices = await h.request("GET", "/v1/devices", {
      token: owner.token,
    });
    expect(devices.body.devices.map((d: { id: string }) => d.id)).toEqual([
      owner.device.id,
    ]);
  });

  test("another Google account with a linked account's email is refused with account_conflict", async () => {
    const first = await signInWithGoogle();
    const { location } = await throughGoogle(
      googleClaims(h.now(), { sub: "google-sub-2" }),
    );
    expect(location).toBe(`${PHONE_REDIRECT}?error=account_conflict`);
    expect(
      (await h.request("GET", "/v1/me", { token: first.token })).status,
    ).toBe(200);
    expect((await signInWithGoogle()).user.id).toBe(first.user.id);
  });

  test("a password login still being checked when Google takes the account over gets no session", async () => {
    await h.signup("a@example.com", DESKTOP);
    const start = await h.request(
      "GET",
      startPath(PHONE_REDIRECT, sha256(randomToken())),
    );
    const state = new URL(start.headers.get("location")!).searchParams.get(
      "state",
    )!;
    const code = h.google!.issue(googleClaims(h.now()));
    const login = h.request("POST", "/v1/auth/login", {
      body: { email: "a@example.com", password: PASSWORD, device: PHONE },
    });
    const back = await h.request(
      "GET",
      `/v1/auth/google/callback?${new URLSearchParams({ state, code })}`,
    );
    expect(back.headers.get("location")).toStartWith(`${PHONE_REDIRECT}?code=`);
    const reply = await login;
    expect(reply.status).toBe(401);
    expect(reply.body.error.code).toBe("invalid_credentials");
  });

  test("signing up with a password for a Google account's email is email_taken", async () => {
    await signInWithGoogle();
    const reply = await h.request("POST", "/v1/auth/signup", {
      body: { email: "a@example.com", password: PASSWORD, device: DESKTOP },
    });
    expect(reply.body.error.code).toBe("email_taken");
  });
});
