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
 * server answering 404 for the route itself: the discovered address is not
 * an account server (yet), which reads as the server not being found, not as
 * a device that was removed.
 */
export function signInErrorKey(code: string): string | null {
  return code === "not_found"
    ? "settings.account.errors.no_server"
    : accountErrorKey(code);
}

/** Same rule as the server's: 8 to 256 characters. */
export const PASSWORD_MIN = 8;
export const PASSWORD_MAX = 256;

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
