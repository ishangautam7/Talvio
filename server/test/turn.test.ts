import { afterEach, describe, expect, test } from "bun:test";
import {
  sharedSecretServer,
  STUN,
  TURN_TTL_SECONDS,
  turnFromEnv,
} from "../src/turn";
import { type Harness, harness } from "./helpers";

let h: Harness | undefined;
afterEach(() => h?.close());

describe("GET /v1/turn", () => {
  test("needs a signed-in device", async () => {
    h = await harness();
    const reply = await h.request("GET", "/v1/turn");
    expect(reply.status).toBe(401);
  });

  test("offers STUN only when no TURN is configured", async () => {
    h = await harness();
    const { token } = await h.signup();
    const reply = await h.request("GET", "/v1/turn", { token });
    expect(reply.status).toBe(200);
    expect(reply.body).toEqual({ iceServers: [STUN], ttl: TURN_TTL_SECONDS });
  });

  test("a shared secret gives TURN REST credentials for the account", async () => {
    h = await harness({
      turn: { kind: "secret", urls: ["turn:turn.test:3478"], secret: "s3cret" },
    });
    const { token, user } = await h.signup();
    const reply = await h.request("GET", "/v1/turn", { token });
    expect(reply.status).toBe(200);
    const [stun, relay] = reply.body.iceServers;
    expect(stun).toEqual(STUN);
    expect(relay.urls).toEqual(["turn:turn.test:3478"]);
    const expires = Math.floor(h.now() / 1000) + TURN_TTL_SECONDS;
    expect(relay.username).toBe(`${expires}:${user.id}`);
    expect(relay.credential).toBe(
      (
        await sharedSecretServer(
          { urls: [], secret: "s3cret" },
          user.id,
          h.now(),
        )
      ).credential,
    );
  });

  test("Cloudflare's answer is passed on, and its failure leaves STUN", async () => {
    let answer = () =>
      Response.json({
        iceServers: [
          {
            urls: [
              "stun:stun.cloudflare.com:3478",
              "turn:turn.cloudflare.com:3478?transport=udp",
              "javascript:alert(1)",
            ],
            username: "u",
            credential: "c",
          },
          { urls: "nonsense" },
        ],
      });
    const seen: { auth: string | null; path: string }[] = [];
    const fake = Bun.serve({
      port: 0,
      hostname: "127.0.0.1",
      fetch(req) {
        seen.push({
          auth: req.headers.get("authorization"),
          path: new URL(req.url).pathname,
        });
        return answer();
      },
    });
    try {
      h = await harness({
        turn: {
          kind: "cloudflare",
          keyId: "key1",
          apiToken: "tok",
          baseUrl: `http://127.0.0.1:${fake.port}/v1`,
        },
      });
      const { token } = await h.signup();
      const reply = await h.request("GET", "/v1/turn", { token });
      expect(seen[0]).toEqual({
        auth: "Bearer tok",
        path: "/v1/turn/keys/key1/credentials/generate-ice-servers",
      });
      expect(reply.body.iceServers).toEqual([
        STUN,
        {
          urls: [
            "stun:stun.cloudflare.com:3478",
            "turn:turn.cloudflare.com:3478?transport=udp",
          ],
          username: "u",
          credential: "c",
        },
      ]);

      answer = () => new Response("nope", { status: 500 });
      const failed = await h.request("GET", "/v1/turn", { token });
      expect(failed.status).toBe(200);
      expect(failed.body.iceServers).toEqual([STUN]);
    } finally {
      fake.stop(true);
    }
  });
});

describe("turnFromEnv", () => {
  test("reads Cloudflare first, then a shared secret, else nothing", () => {
    expect(turnFromEnv({})).toBeNull();
    expect(
      turnFromEnv({ TURN_URLS: "turn:a:3478", TURN_SECRET: "" }),
    ).toBeNull();
    expect(
      turnFromEnv({
        TURN_URLS: " turn:a:3478 , turns:a:5349 ",
        TURN_SECRET: "x",
      }),
    ).toEqual({
      kind: "secret",
      urls: ["turn:a:3478", "turns:a:5349"],
      secret: "x",
    });
    expect(
      turnFromEnv({
        CLOUDFLARE_TURN_KEY_ID: "k",
        CLOUDFLARE_TURN_API_TOKEN: "t",
        TURN_URLS: "turn:a",
        TURN_SECRET: "x",
      }),
    ).toEqual({ kind: "cloudflare", keyId: "k", apiToken: "t" });
  });
});
