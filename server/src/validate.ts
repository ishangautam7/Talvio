import { ApiError, invalidRequest, isObject } from "./http";

export interface DeviceInfo {
  name: string;
  platform: string;
  kind: string;
  appVersion: string | null;
}

const PLATFORMS = new Set<unknown>([
  "windows",
  "macos",
  "linux",
  "android",
  "ios",
]);
const KINDS = new Set<unknown>(["desktop", "mobile"]);
const CAPABILITY = /^[a-z0-9._-]{1,64}$/;
const EMAIL = /^[^\s@]+@[^\s@.]+(\.[^\s@.]+)+$/;

/** Length in characters (code points), not UTF-16 units. */
const length = (text: string) => [...text].length;

export function deviceInfo(value: unknown): DeviceInfo {
  if (!isObject(value)) throw invalidRequest("device is required.");
  const name = typeof value.name === "string" ? value.name.trim() : "";
  if (length(name) < 1 || length(name) > 64) {
    throw invalidRequest("device.name must be 1 to 64 characters.");
  }
  if (!PLATFORMS.has(value.platform))
    throw invalidRequest("device.platform is not valid.");
  if (!KINDS.has(value.kind)) throw invalidRequest("device.kind is not valid.");
  const appVersion = value.appVersion ?? null;
  if (
    appVersion !== null &&
    (typeof appVersion !== "string" || length(appVersion) > 32)
  ) {
    throw invalidRequest("device.appVersion must be at most 32 characters.");
  }
  return {
    name,
    platform: value.platform as string,
    kind: value.kind as string,
    appVersion,
  };
}

/** Trimmed and lower-cased; `invalid_email` when it is not an address. */
export function email(value: unknown): string {
  if (typeof value !== "string") throw invalidRequest("email is required.");
  const address = value.trim().toLowerCase();
  if (address.length > 254 || !EMAIL.test(address)) {
    throw new ApiError(400, "invalid_email", "That is not an email address.");
  }
  return address;
}

export function newPassword(value: unknown): string {
  if (typeof value !== "string") throw invalidRequest("password is required.");
  if (length(value) < 8 || length(value) > 256) {
    throw new ApiError(
      400,
      "weak_password",
      "Passwords must be 8 to 256 characters.",
    );
  }
  return value;
}

/** An optional display name: absent, null or blank all mean none. */
export function displayName(value: unknown): string | null {
  if (value === undefined || value === null) return null;
  if (typeof value !== "string" || length(value.trim()) > 64) {
    throw invalidRequest("name must be at most 64 characters.");
  }
  return value.trim() || null;
}

/** A `hello` capability list, or null when it breaks the protocol's limits. */
export function capabilities(value: unknown): string[] | null {
  return Array.isArray(value) &&
    value.length <= 64 &&
    value.every((item) => typeof item === "string" && CAPABILITY.test(item))
    ? value
    : null;
}
