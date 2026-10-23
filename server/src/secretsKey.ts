import { chmodSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { randomBytes } from "node:crypto";
import { dirname, join } from "node:path";

/** The key file's name, in the database's folder. */
export const SECRETS_KEY_FILE = "secrets.key";

const KEY_BYTES = 32;

export interface SecretsKey {
  key: Uint8Array;
  /** Where it came from, for the one log line; never the key itself. */
  source: string;
}

function decode(text: string, where: string): Uint8Array {
  const bytes = Buffer.from(text.trim(), "base64");
  if (bytes.length !== KEY_BYTES) {
    throw new Error(
      `${where} must be the base64 of exactly ${KEY_BYTES} bytes.`,
    );
  }
  return new Uint8Array(bytes);
}

/**
 * The AES-256 key that encrypts stored API keys: `TALVIO_SECRETS_KEY` (base64
 * of 32 bytes) when set; otherwise `secrets.key` beside the database, created
 * with 32 random bytes on first start (mode 0600 where the OS has modes). An
 * in-memory database gets a key that lives as long as the process.
 */
export function resolveSecretsKey(
  databasePath: string,
  fromEnv: string | undefined,
): SecretsKey {
  if (fromEnv !== undefined && fromEnv.trim() !== "") {
    return {
      key: decode(fromEnv, "TALVIO_SECRETS_KEY"),
      source: "TALVIO_SECRETS_KEY",
    };
  }
  if (databasePath === ":memory:") {
    return {
      key: new Uint8Array(randomBytes(KEY_BYTES)),
      source: "a temporary key (in-memory database)",
    };
  }
  const path = join(dirname(databasePath), SECRETS_KEY_FILE);
  if (existsSync(path)) {
    return { key: decode(readFileSync(path, "utf8"), path), source: path };
  }
  const key = new Uint8Array(randomBytes(KEY_BYTES));
  // "wx": never overwrite a key file that appeared meanwhile; losing it loses every stored secret.
  writeFileSync(path, `${Buffer.from(key).toString("base64")}\n`, {
    flag: "wx",
    mode: 0o600,
  });
  try {
    chmodSync(path, 0o600);
  } catch {
    // Windows has no Unix modes; the folder's ACL protects it there.
  }
  return { key, source: `${path} (created)` };
}
