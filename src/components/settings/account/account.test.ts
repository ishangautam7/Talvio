import { describe, expect, test } from "bun:test";
import type { AccountUser } from "@/bindings";
import en from "@/i18n/locales/en/translation.json";
import {
  accountErrorKey,
  avatarLetter,
  displayName,
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
  test("a 404 on sign-in is the server not being found, not a device", () => {
    const key = signInErrorKey("not_found");
    expect(key).toBe("settings.account.errors.no_server");
    expect(hasKey(key!)).toBe(true);
    expect(signInErrorKey("invalid_credentials")).toBe(
      accountErrorKey("invalid_credentials"),
    );
    expect(signInErrorKey("cancelled")).toBeNull();
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
