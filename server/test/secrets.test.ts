import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { Database } from "bun:sqlite";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Store } from "../src/db";
import { SecretBox } from "../src/secrets";
import { resolveSecretsKey, SECRETS_KEY_FILE } from "../src/secretsKey";
import { type Harness, harness, type Json, PHONE } from "./helpers";

const NAME = "post_process:openai";
const VALUE = "sk-test-0123456789abcdef";

let h: Harness;
let token: string;
beforeEach(async () => {
  h = await harness();
  token = (await h.signup()).token;
});
afterEach(() => h.close());

const put = (name: string, body: unknown, as = token) =>
  h.request("PUT", `/v1/secrets/${name}`, { token: as, body });
const del = (name: string, updatedAt: number | string, as = token) =>
  h.request("DELETE", `/v1/secrets/${name}?updatedAt=${updatedAt}`, {
    token: as,
  });
const list = async (as = token): Promise<Json[]> => {
  const reply = await h.request("GET", "/v1/secrets", { token: as });
  expect(reply.status).toBe(200);
  return reply.body.secrets;
};

describe("secrets", () => {
  test("round trip: PUT then GET from another device of the account", async () => {
    const at = h.now() - 1000;
    const reply = await put(NAME, { value: VALUE, updatedAt: at });
    expect(reply.status).toBe(200);
    expect(reply.body).toEqual({
      secret: { name: NAME, value: VALUE, updatedAt: at, deleted: false },
    });
    const phone = await h.login("a@example.com", PHONE);
    expect(await list(phone.token)).toEqual([
      { name: NAME, value: VALUE, updatedAt: at, deleted: false },
    ]);
  });

  test("a percent-encoded colon in the path is the same name", async () => {
    const at = h.now();
    expect(
      (await put("cloud_stt%3Aelevenlabs", { value: "x", updatedAt: at }))
        .status,
    ).toBe(200);
    expect((await list()).map((s) => s.name)).toEqual(["cloud_stt:elevenlabs"]);
  });

  test("an empty account lists nothing", async () => {
    expect(await list()).toEqual([]);
  });

  test("last writer wins by updatedAt", async () => {
    const at = h.now();
    await put(NAME, { value: "v1", updatedAt: at });

    const older = await put(NAME, { value: "old", updatedAt: at - 1 });
    expect(older.status).toBe(409);
    expect(older.body.error.code).toBe("stale");
    expect(older.body.secret).toEqual({
      name: NAME,
      value: "v1",
      updatedAt: at,
      deleted: false,
    });

    const equal = await put(NAME, { value: "v1-retry", updatedAt: at });
    expect(equal.status).toBe(200);
    expect(equal.body.secret.value).toBe("v1-retry");

    const newer = await put(NAME, { value: "v2", updatedAt: at + 5 });
    expect(newer.status).toBe(200);
    expect(await list()).toEqual([
      { name: NAME, value: "v2", updatedAt: at + 5, deleted: false },
    ]);
  });

  test("DELETE leaves a tombstone a stale PUT cannot overwrite", async () => {
    const at = h.now();
    await put(NAME, { value: VALUE, updatedAt: at });
    const gone = await del(NAME, at + 10);
    expect(gone.status).toBe(200);
    expect(gone.body.secret).toEqual({
      name: NAME,
      value: "",
      updatedAt: at + 10,
      deleted: true,
    });
    expect(await list()).toEqual([gone.body.secret]);

    const stale = await put(NAME, { value: "offline", updatedAt: at + 5 });
    expect(stale.status).toBe(409);
    expect(stale.body.secret).toEqual(gone.body.secret);

    const staleDelete = await del(NAME, at + 1);
    expect(staleDelete.status).toBe(409);
    expect(staleDelete.body.error.code).toBe("stale");

    const revived = await put(NAME, { value: "new", updatedAt: at + 20 });
    expect(revived.status).toBe(200);
    expect(revived.body.secret.deleted).toBe(false);
  });

  test("deleting a name that never existed creates a tombstone", async () => {
    const at = h.now();
    const reply = await del("web_search:serper", at);
    expect(reply.status).toBe(200);
    expect(reply.body.secret).toEqual({
      name: "web_search:serper",
      value: "",
      updatedAt: at,
      deleted: true,
    });
    expect(
      (await put("web_search:serper", { value: "k", updatedAt: at - 1 }))
        .status,
    ).toBe(409);
  });

  test("accounts never see each other's secrets", async () => {
    const at = h.now();
    await put(NAME, { value: VALUE, updatedAt: at });
    const other = (await h.signup("b@example.com")).token;
    expect(await list(other)).toEqual([]);
    // B writing the same name makes B's own record and leaves A's alone.
    expect(
      (await put(NAME, { value: "b-key", updatedAt: at - 100 }, other)).status,
    ).toBe(200);
    expect((await del(NAME, at + 1, other)).status).toBe(200);
    expect(await list()).toEqual([
      { name: NAME, value: VALUE, updatedAt: at, deleted: false },
    ]);
  });

  test("every route needs a valid session", async () => {
    for (const reply of [
      await h.request("GET", "/v1/secrets"),
      await h.request("GET", "/v1/secrets", { token: "zv1_nope" }),
      await h.request("PUT", `/v1/secrets/${NAME}`, {
        body: { value: "x", updatedAt: h.now() },
      }),
      await h.request("DELETE", `/v1/secrets/${NAME}?updatedAt=${h.now()}`),
    ]) {
      expect(reply.status).toBe(401);
      expect(reply.body.error.code).toBe("unauthorized");
    }
  });

  test.each([
    ["no prefix", "openai"],
    ["unknown prefix", "memory:openai"],
    ["upper case", "post_process:OpenAI"],
    ["empty key part", "post_process:"],
    ["long key part", `post_process:${"a".repeat(65)}`],
    ["bare legacy name", "assistant_tts"],
    ["space", "post_process:open%20ai"],
  ])("invalid name: %s", async (_label, name) => {
    const reply = await put(name, { value: "x", updatedAt: h.now() });
    expect(reply.status).toBe(400);
    expect(reply.body.error.code).toBe("invalid_name");
    expect((await del(name, h.now())).body.error.code).toBe("invalid_name");
  });

  test("the longest valid key part is accepted", async () => {
    const name = `assistant_tts:${"a.b_c-9".repeat(9).slice(0, 64)}`;
    expect((await put(name, { value: "x", updatedAt: h.now() })).status).toBe(
      200,
    );
  });

  test.each([
    ["missing", undefined],
    ["empty", ""],
    ["a number", 42],
    ["too long", "x".repeat(8193)],
  ])("invalid value: %s", async (_label, value) => {
    const reply = await put(NAME, { value, updatedAt: h.now() });
    expect(reply.status).toBe(400);
    expect(reply.body.error.code).toBe("invalid_value");
  });

  test("a value of exactly 8192 characters is accepted", async () => {
    const value = "é".repeat(8192);
    const reply = await put(NAME, { value, updatedAt: h.now() });
    expect(reply.status).toBe(200);
    expect((await list())[0]!.value).toBe(value);
  });

  test("updatedAt must be integer ms, at most 5 minutes ahead", async () => {
    const ahead = h.now() + 5 * 60 * 1000;
    expect((await put(NAME, { value: "x", updatedAt: ahead })).status).toBe(
      200,
    );
    for (const updatedAt of [undefined, "1", 1.5, -1, ahead + 1]) {
      const reply = await put(NAME, { value: "x", updatedAt });
      expect(reply.status).toBe(400);
      expect(reply.body.error.code).toBe("invalid_time");
    }
    for (const updatedAt of ["", "abc", "1.5", "-1", String(ahead + 1)]) {
      const reply = await del(NAME, updatedAt);
      expect(reply.status).toBe(400);
      expect(reply.body.error.code).toBe("invalid_time");
    }
    const none = await h.request("DELETE", `/v1/secrets/${NAME}`, { token });
    expect(none.body.error.code).toBe("invalid_time");
  });

  test("malformed and oversized bodies are refused", async () => {
    const bad = await h.request("PUT", `/v1/secrets/${NAME}`, {
      token,
      body: "{not json",
    });
    expect(bad.body.error.code).toBe("invalid_request");
    const big = await put(NAME, { value: "x", pad: "y".repeat(70_000) });
    expect(big.status).toBe(413);
  });

  test("at most 100 records per account, tombstones included", async () => {
    const at = h.now();
    for (let i = 0; i < 99; i++) {
      expect(
        (await put(`cloud_stt:k${i}`, { value: "v", updatedAt: at })).status,
      ).toBe(200);
    }
    expect((await del("web_search:gone", at)).status).toBe(200);
    const full = await put("cloud_stt:one-more", { value: "v", updatedAt: at });
    expect(full.status).toBe(400);
    expect(full.body.error.code).toBe("too_many");
    expect((await del("web_search:another", at)).body.error.code).toBe(
      "too_many",
    );
    // Rewriting an existing record is still allowed.
    expect(
      (await put("cloud_stt:k0", { value: "w", updatedAt: at })).status,
    ).toBe(200);
    expect(
      (await put("web_search:gone", { value: "w", updatedAt: at })).status,
    ).toBe(200);
    // Another account has its own allowance.
    const other = (await h.signup("b@example.com")).token;
    expect((await put(NAME, { value: "v", updatedAt: at }, other)).status).toBe(
      200,
    );
  });

  test("deleting the account removes its secrets", async () => {
    const dir = mkdtempSync(join(tmpdir(), "talvio-secrets-"));
    const databasePath = join(dir, "talvio.db");
    const own = await harness({ databasePath });
    try {
      const a = (await own.signup("a@example.com")).token;
      const b = (await own.signup("b@example.com")).token;
      await own.request("PUT", `/v1/secrets/${NAME}`, {
        token: a,
        body: { value: VALUE, updatedAt: own.now() },
      });
      await own.request("PUT", `/v1/secrets/${NAME}`, {
        token: b,
        body: { value: "b", updatedAt: own.now() },
      });
      expect((await own.request("DELETE", "/v1/me", { token: a })).status).toBe(
        204,
      );
    } finally {
      await own.close();
    }
    const db = new Database(databasePath);
    const rows = db.query("SELECT user_id FROM secrets").all();
    db.close();
    expect(rows.length).toBe(1);
    rmSync(dir, { recursive: true, force: true });
  });
});

describe("secrets at rest", () => {
  let dir: string;
  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "talvio-secrets-"));
  });
  afterEach(() => rmSync(dir, { recursive: true, force: true }));

  test("the key file is created once and reused across restarts", async () => {
    const databasePath = join(dir, "talvio.db");
    const keyPath = join(dir, SECRETS_KEY_FILE);
    expect(existsSync(keyPath)).toBe(false);

    const first = await harness({ databasePath });
    const a = (await first.signup()).token;
    await first.request("PUT", `/v1/secrets/${NAME}`, {
      token: a,
      body: { value: VALUE, updatedAt: first.now() },
    });
    await first.close();
    const key = readFileSync(keyPath, "utf8");
    expect(Buffer.from(key.trim(), "base64").length).toBe(32);

    const second = await harness({ databasePath });
    const listed = await second.request("GET", "/v1/secrets", { token: a });
    await second.close();
    expect(listed.body.secrets[0].value).toBe(VALUE);
    expect(readFileSync(keyPath, "utf8")).toBe(key);
  });

  test("the database holds ciphertext bound to its key, account and name", async () => {
    const databasePath = join(dir, "talvio.db");
    const envKey = Buffer.alloc(32, 7).toString("base64");
    const app = await harness({ databasePath, secretsKey: envKey });
    const signed = await app.signup();
    await app.request("PUT", `/v1/secrets/${NAME}`, {
      token: signed.token,
      body: { value: VALUE, updatedAt: app.now() },
    });
    await app.close();
    // The env key wins: no key file was written.
    expect(existsSync(join(dir, SECRETS_KEY_FILE))).toBe(false);

    const db = new Database(databasePath);
    const row = db
      .query<
        { user_id: string; nonce: Uint8Array; ciphertext: Uint8Array },
        []
      >("SELECT user_id, nonce, ciphertext FROM secrets")
      .get()!;
    const raw = readFileSync(databasePath);
    db.close();
    expect(row.user_id).toBe(signed.user.id);
    expect(row.nonce.length).toBe(12);
    expect(Buffer.from(row.ciphertext).toString("utf8")).not.toContain(VALUE);
    expect(raw.includes(Buffer.from(VALUE))).toBe(false);

    const right = new SecretBox(Buffer.from(envKey, "base64"));
    expect(
      await right.open(signed.user.id, NAME, row.nonce, row.ciphertext),
    ).toBe(VALUE);
    const wrongKey = new SecretBox(Buffer.alloc(32, 8));
    await expect(
      wrongKey.open(signed.user.id, NAME, row.nonce, row.ciphertext),
    ).rejects.toThrow();
    await expect(
      right.open("another-account", NAME, row.nonce, row.ciphertext),
    ).rejects.toThrow();
    await expect(
      right.open(signed.user.id, "cloud_stt:openai", row.nonce, row.ciphertext),
    ).rejects.toThrow();
  });

  test("every write gets a fresh nonce", async () => {
    const box = new SecretBox(Buffer.alloc(32, 1));
    const one = await box.seal("acct", NAME, VALUE);
    const two = await box.seal("acct", NAME, VALUE);
    expect(Buffer.from(one.nonce).equals(Buffer.from(two.nonce))).toBe(false);
    expect(
      Buffer.from(one.ciphertext).equals(Buffer.from(two.ciphertext)),
    ).toBe(false);
  });

  test("a malformed key is refused at start", () => {
    expect(() => resolveSecretsKey(join(dir, "talvio.db"), "c2hvcnQ=")).toThrow(
      /32 bytes/,
    );
  });
});

describe("migration", () => {
  test("a version 1 database gains the secrets table and keeps its accounts", () => {
    const dir = mkdtempSync(join(tmpdir(), "talvio-migrate-"));
    const path = join(dir, "talvio.db");
    const v1 = new Store(path);
    v1.insertUser(
      {
        email: "a@example.com",
        name: null,
        password_hash: null,
        google_sub: null,
      },
      1,
    );
    v1.close();
    const raw = new Database(path);
    raw.run("DROP TABLE secrets");
    raw.run("PRAGMA user_version = 1");
    raw.close();

    const migrated = new Store(path);
    const user = migrated.userByEmail("a@example.com")!;
    expect(user).not.toBeNull();
    expect(migrated.listSecrets(user.id)).toEqual([]);
    migrated.close();
    const check = new Database(path);
    const { user_version } = check
      .query<{ user_version: number }, []>("PRAGMA user_version")
      .get()!;
    check.close();
    expect(user_version).toBe(2);
    rmSync(dir, { recursive: true, force: true });
  });
});
