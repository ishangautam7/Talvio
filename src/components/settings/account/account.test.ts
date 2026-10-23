import { describe, expect, test } from "bun:test";
import type { AccountUser } from "@/bindings";
import en from "@/i18n/locales/en/translation.json";
import {
  accountErrorKey,
  avatarLetter,
  displayName,
  normalizeServerUrl as normalize,
  platformName,
  signInErrorKey,
} from "./account";

const user = (name: string | null, email = "a@b.com"): AccountUser => ({
  id: "u1",
  email,
  name,
  has_password: true,
  google_linked: false,
});

const hasKey = (key: string) =>
  key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown>)?.[part],
      en,
    ) !== undefined;

describe("accountErrorKey", () => {
  test("every contract code has its own sentence", () => {
    for (const code of [
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
    ]) {
      const key = accountErrorKey(code);
      expect(key).toBe(`settings.account.errors.${code}`);
      expect(hasKey(key!)).toBe(true);
    }
  });

  test("a code it does not know reads as the generic message", () => {
    expect(accountErrorKey("internal")).toBe("settings.account.errors.generic");
    expect(accountErrorKey("")).toBe("settings.account.errors.generic");
    expect(hasKey("settings.account.errors.generic")).toBe(true);
  });

  test("stopping the Google flow yourself is not an error", () => {
    expect(accountErrorKey("cancelled")).toBeNull();
  });
});

describe("signInErrorKey", () => {
  test("a 404 on sign-in is the server address, not a device", () => {
    const key = signInErrorKey("not_found");
    expect(key).toBe("settings.account.errors.wrongServer");
    expect(hasKey(key!)).toBe(true);
    expect(signInErrorKey("invalid_credentials")).toBe(
      accountErrorKey("invalid_credentials"),
    );
    expect(signInErrorKey("cancelled")).toBeNull();
  });
});

// The same cases as `normalize_server_url` in src-tauri/src/account/mod.rs.
describe("normalizeServerUrl", () => {
  // Debug builds' rule; release builds are below.
  const normalizeServerUrl = (input: string) => normalize(input, false);

  test("keeps http and https, without trailing slashes or spaces", () => {
    expect(normalizeServerUrl(" https://sync.example.com/ ")).toBe(
      "https://sync.example.com",
    );
    expect(normalizeServerUrl("http://localhost:8787//")).toBe(
      "http://localhost:8787",
    );
    expect(normalizeServerUrl("https://example.com/zynveliq")).toBe(
      "https://example.com/zynveliq",
    );
    expect(normalizeServerUrl("HTTPS://Sync.Example.com")).toBe(
      "https://sync.example.com",
    );
  });

  test("refuses anything that is not a plain web address", () => {
    expect(normalizeServerUrl("")).toBeNull();
    expect(normalizeServerUrl("sync.example.com")).toBeNull();
    expect(normalizeServerUrl("ftp://example.com")).toBeNull();
    expect(normalizeServerUrl("javascript:alert(1)")).toBeNull();
    expect(normalizeServerUrl("file:///etc/passwd")).toBeNull();
    expect(normalizeServerUrl("https://")).toBeNull();
    expect(normalizeServerUrl("https://user:pw@example.com")).toBeNull();
    expect(normalizeServerUrl("https://example.com/?a=1")).toBeNull();
    expect(normalizeServerUrl("https://example.com/#top")).toBeNull();
    expect(normalizeServerUrl("https://example.com/?")).toBeNull();
    expect(normalizeServerUrl("https://example.com#")).toBeNull();
  });

  test("a release build takes http only for this computer", () => {
    for (const good of [
      "https://sync.example.com",
      "http://localhost:8787",
      "http://127.0.0.1:8787",
      "http://127.1.2.3",
      "http://[::1]:8787",
    ]) {
      expect(normalize(good, true)).not.toBeNull();
    }
    for (const bad of [
      "http://sync.example.com",
      "http://192.168.1.20:8787",
      "http://localhost.example.com",
      "http://[::2]",
    ]) {
      expect(normalize(bad, true)).toBeNull();
    }
  });
});

describe("names", () => {
  test("falls back to the email when there is no name", () => {
    expect(displayName(user("Ishan"))).toBe("Ishan");
    expect(displayName(user(null))).toBe("a@b.com");
    expect(displayName(user("   "))).toBe("a@b.com");
  });

  test("the avatar letter is one whole character", () => {
    expect(avatarLetter(user("ishan"))).toBe("I");
    expect(avatarLetter(user(null, "zoe@example.com"))).toBe("Z");
    expect(avatarLetter(user("😀 Sam"))).toBe("😀");
    expect(avatarLetter(user("élodie"))).toBe("É");
  });

  test("platforms read as their product names", () => {
    expect(platformName("macos")).toBe("macOS");
    expect(platformName("ios")).toBe("iOS");
    expect(platformName("haiku")).toBe("haiku");
  });
});
