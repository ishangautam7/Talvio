import { authenticate } from "./account";
import type { Ctx } from "./app";
import type { SecretRow, SecretWrite } from "./db";
import { ApiError, json, readJson } from "./http";

/**
 * Provider API keys that follow an account across its signed-in computers.
 * Stored AES-256-GCM encrypted, never in plaintext, and never logged.
 */

const NAME =
  /^(post_process|cloud_stt|assistant_tts|web_search):[a-z0-9_.-]{1,64}$/;
const MAX_VALUE_CHARS = 8192;
/** Live records and tombstones together, per account. */
export const MAX_SECRETS = 100;
/** How far ahead of the server's clock a client's `updatedAt` may be. */
const MAX_CLOCK_AHEAD_MS = 5 * 60 * 1000;
const NONCE_BYTES = 12;

/** AES-256-GCM with a fresh nonce per write, bound to the account and the name. */
export class SecretBox {
  private readonly key: Promise<CryptoKey>;

  constructor(raw: Uint8Array) {
    this.key = crypto.subtle.importKey(
      "raw",
      new Uint8Array(raw),
      "AES-GCM",
      false,
      ["encrypt", "decrypt"],
    );
  }

  /** What the ciphertext is bound to, so it cannot be moved to another account or name. */
  static associatedData(accountId: string, name: string): Uint8Array {
    return new TextEncoder().encode(`${accountId}\n${name}`);
  }

  async seal(
    accountId: string,
    name: string,
    value: string,
  ): Promise<{ nonce: Uint8Array; ciphertext: Uint8Array }> {
    const nonce = crypto.getRandomValues(new Uint8Array(NONCE_BYTES));
    const ciphertext = await crypto.subtle.encrypt(
      {
        name: "AES-GCM",
        iv: nonce,
        additionalData: SecretBox.associatedData(accountId, name),
      },
      await this.key,
      new TextEncoder().encode(value),
    );
    return { nonce, ciphertext: new Uint8Array(ciphertext) };
  }

  /** Throws when the key, the nonce, the account or the name does not match. */
  async open(
    accountId: string,
    name: string,
    nonce: Uint8Array,
    ciphertext: Uint8Array,
  ): Promise<string> {
    const plain = await crypto.subtle.decrypt(
      {
        name: "AES-GCM",
        iv: new Uint8Array(nonce),
        additionalData: SecretBox.associatedData(accountId, name),
      },
      await this.key,
      new Uint8Array(ciphertext),
    );
    return new TextDecoder().decode(plain);
  }
}

async function secretJson(ctx: Ctx, accountId: string, row: SecretRow) {
  let value = "";
  if (!row.deleted && row.nonce && row.ciphertext) {
    try {
      value = await ctx.secrets.open(
        accountId,
        row.name,
        row.nonce,
        row.ciphertext,
      );
    } catch (error) {
      // The name only: never the value, the key or the ciphertext.
      ctx.log(`secret ${row.name} could not be decrypted (wrong key?)`);
      throw error;
    }
  }
  return {
    name: row.name,
    value,
    updatedAt: row.updated_at,
    deleted: row.deleted === 1,
  };
}

function secretName(raw: string): string {
  let name: string;
  try {
    name = decodeURIComponent(raw);
  } catch {
    name = "";
  }
  if (!NAME.test(name)) {
    throw new ApiError(400, "invalid_name", "That is not a secret name.");
  }
  return name;
}

function updatedAt(value: unknown, ctx: Ctx): number {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value < 0 ||
    value > ctx.now() + MAX_CLOCK_AHEAD_MS
  ) {
    throw new ApiError(
      400,
      "invalid_time",
      "updatedAt must be a time in ms, at most 5 minutes ahead of the server.",
    );
  }
  return value;
}

function secretValue(value: unknown): string {
  if (typeof value !== "string") throw invalidValue();
  const chars = [...value].length;
  if (chars < 1 || chars > MAX_VALUE_CHARS) throw invalidValue();
  return value;
}

const invalidValue = () =>
  new ApiError(
    400,
    "invalid_value",
    `value must be a string of 1 to ${MAX_VALUE_CHARS} characters.`,
  );

async function write(
  ctx: Ctx,
  accountId: string,
  name: string,
  change: SecretWrite,
): Promise<Response> {
  const result = ctx.store.writeSecret(accountId, name, change, MAX_SECRETS);
  if (result.kind === "too_many") {
    throw new ApiError(
      400,
      "too_many",
      `An account can hold at most ${MAX_SECRETS} secrets.`,
    );
  }
  const secret = await secretJson(ctx, accountId, result.row);
  if (result.kind === "stale") {
    throw new ApiError(
      409,
      "stale",
      "A newer version of this secret is stored.",
      {},
      { secret },
    );
  }
  return json({ secret });
}

export async function listSecrets(req: Request, ctx: Ctx): Promise<Response> {
  const device = authenticate(req, ctx);
  const rows = ctx.store.listSecrets(device.user_id);
  const secrets = [];
  for (const row of rows)
    secrets.push(await secretJson(ctx, device.user_id, row));
  return json({ secrets });
}

export async function putSecret(
  req: Request,
  ctx: Ctx,
  rawName: string,
): Promise<Response> {
  const device = authenticate(req, ctx);
  const name = secretName(rawName);
  const body = await readJson(req);
  const value = secretValue(body.value);
  const at = updatedAt(body.updatedAt, ctx);
  // Encrypted before the write's transaction, which is synchronous, so the
  // staleness check and the write cannot be split by another request.
  const sealed = await ctx.secrets.seal(device.user_id, name, value);
  return write(ctx, device.user_id, name, {
    ...sealed,
    deleted: false,
    updatedAt: at,
  });
}

export async function deleteSecret(
  req: Request,
  ctx: Ctx,
  url: URL,
  rawName: string,
): Promise<Response> {
  const device = authenticate(req, ctx);
  const name = secretName(rawName);
  const raw = url.searchParams.get("updatedAt");
  const at = updatedAt(
    raw !== null && /^\d{1,16}$/.test(raw) ? Number(raw) : undefined,
    ctx,
  );
  return write(ctx, device.user_id, name, {
    nonce: null,
    ciphertext: null,
    deleted: true,
    updatedAt: at,
  });
}
