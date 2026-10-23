import { createHash, randomBytes, timingSafeEqual } from "node:crypto";

/** 32 random bytes, base64url without padding. */
export const randomToken = (): string => randomBytes(32).toString("base64url");

/** SHA-256, base64url without padding: the storage form of every secret, and PKCE's S256. */
export const sha256 = (value: string): string =>
  createHash("sha256").update(value).digest("base64url");

export function pkceMatches(verifier: string, challenge: string): boolean {
  const expected = Buffer.from(sha256(verifier));
  const given = Buffer.from(challenge);
  return expected.length === given.length && timingSafeEqual(expected, given);
}
