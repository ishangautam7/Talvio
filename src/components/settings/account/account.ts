import type { AccountDevice, AccountUser } from "@/bindings";

/**
 * The account commands fail with a code, never a sentence (see
 * `server/PROTOCOL.md` and `src-tauri/src/account/`), and each code the user
 * can do something about has its own text. Anything else, including a code a
 * newer server adds, reads as the generic message.
 */
const ERROR_CODES = new Set([
  "invalid_credentials",
  "email_taken",
  "invalid_email",
  "weak_password",
  "rate_limited",
  "invalid_code",
  "google_unavailable",
  "account_conflict",
  "unauthorized",
  "not_found",
  "invalid_request",
  "network",
  "no_server",
  "server",
  "timeout",
  "signed_in",
  "invalid_url",
  "keychain",
]);

/** The translation key for an account error code, or null when it needs no message. */
export function accountErrorKey(code: string): string | null {
  // The user stopped the Google flow themselves: nothing to report.
  if (code === "cancelled") return null;
  return `settings.account.errors.${ERROR_CODES.has(code) ? code : "generic"}`;
}

/**
 * {@link accountErrorKey} for the sign-in routes, where `not_found` is the
 * server answering 404 for the route itself: the address is not an account
 * server (one ending in `/v1`, say), not a device that was removed.
 */
export function signInErrorKey(code: string): string | null {
  return code === "not_found"
    ? "settings.account.errors.wrongServer"
    : accountErrorKey(code);
}

/** Same rule as the server's: 8 to 256 characters. */
export const PASSWORD_MIN = 8;
export const PASSWORD_MAX = 256;

/**
 * The server address as `set_account_server_url` stores it, by the same rule
 * as `normalize_server_url` in `src-tauri/src/account/mod.rs`: http or https,
 * a host, nothing after the path, no trailing slash, and in a release build
 * http only to this computer, so a password never crosses a network in the
 * clear. Null when it is not such an address, so the field can say so before
 * anything is saved.
 */
export function normalizeServerUrl(
  input: string,
  release = !import.meta.env?.DEV,
): string | null {
  let url: URL;
  try {
    url = new URL(input.trim());
  } catch {
    return null;
  }
  const plain =
    (url.protocol === "https:" ||
      (url.protocol === "http:" && (!release || isLoopback(url.hostname)))) &&
    url.hostname !== "" &&
    url.username === "" &&
    url.password === "" &&
    // A literal ? or # in the serialized form is a query or fragment, even an empty one.
    !/[?#]/.test(url.href);
  return plain ? url.href.replace(/\/+$/, "") : null;
}

/** `localhost`, 127.0.0.0/8 or [::1], as `new URL` writes them. */
const isLoopback = (hostname: string): boolean =>
  hostname === "localhost" ||
  hostname === "[::1]" ||
  /^127\.\d+\.\d+\.\d+$/.test(hostname);

/** The name to greet someone by: their own, else their email. */
export const displayName = (user: AccountUser): string =>
  user.name?.trim() || user.email;

/** One letter for the avatar, whole even when it is an emoji or accented. */
export const avatarLetter = (user: AccountUser): string =>
  (Array.from(displayName(user))[0] ?? "").toLocaleUpperCase();

const PLATFORM_NAMES: Record<string, string> = {
  windows: "Windows",
  macos: "macOS",
  linux: "Linux",
  android: "Android",
  ios: "iOS",
};

/** Product names, the same in every language. */
export const platformName = (platform: string): string =>
  PLATFORM_NAMES[platform] ?? platform;

export const isPhone = (device: AccountDevice): boolean =>
  device.kind === "mobile";
