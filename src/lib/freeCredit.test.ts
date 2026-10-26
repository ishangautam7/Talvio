import { describe, expect, test } from "bun:test";
import type { FreeCreditStatus } from "@/bindings";
import {
  formatUsd,
  formatUsdAmount,
  freeCreditView,
  offersFreeCredit,
  remainingFraction,
  servesAnyJob,
  withoutTalvioFree,
} from "./freeCredit";

const NOTHING = {
  assistant: false,
  cleanup: false,
  command: false,
  stt: false,
  voice: false,
};

const status = (
  overrides: Partial<FreeCreditStatus> = {},
): FreeCreditStatus => ({
  signed_in: true,
  balance: {
    limit_usd: 1,
    used_usd: 0.27,
    remaining_usd: 0.73,
    enabled: true,
  },
  exhausted: false,
  serving: { ...NOTHING },
  ...overrides,
});

describe("formatUsd", () => {
  test("shows whole cents in dollars", () => {
    expect(formatUsd(0.73, "en-US")).toBe("$0.73");
    expect(formatUsd(1, "en-US")).toBe("$1.00");
    expect(formatUsd(0.004, "en-US")).toBe("$0.00");
  });

  test("names a whole allowance without cents", () => {
    expect(formatUsdAmount(1, "en-US")).toBe("$1");
    expect(formatUsdAmount(0.5, "en-US")).toBe("$0.50");
  });

  test("never shows a negative or broken amount", () => {
    expect(formatUsd(-0.5, "en-US")).toBe("$0.00");
    expect(formatUsd(Number.NaN, "en-US")).toBe("$0.00");
  });
});

describe("freeCreditView", () => {
  test("signed out shows nothing", () => {
    expect(freeCreditView(null).kind).toBe("signed_out");
    expect(freeCreditView(status({ signed_in: false })).kind).toBe(
      "signed_out",
    );
  });

  test("waits for the server's balance", () => {
    expect(freeCreditView(status({ balance: null })).kind).toBe("loading");
  });

  test("reports what is left", () => {
    expect(freeCreditView(status())).toEqual({
      kind: "available",
      remaining: 0.73,
      limit: 1,
      used: 0.27,
    });
    expect(remainingFraction(freeCreditView(status()))).toBeCloseTo(0.73);
  });

  test("a 402 or an empty balance reads as used up", () => {
    expect(freeCreditView(status({ exhausted: true })).kind).toBe("exhausted");
    expect(
      freeCreditView(
        status({
          balance: {
            limit_usd: 1,
            used_usd: 1,
            remaining_usd: 0,
            enabled: true,
          },
        }),
      ).kind,
    ).toBe("exhausted");
  });

  test("switched off on the server", () => {
    expect(
      freeCreditView(
        status({
          balance: {
            limit_usd: 1,
            used_usd: 0,
            remaining_usd: 1,
            enabled: false,
          },
        }),
      ).kind,
    ).toBe("disabled");
  });
});

describe("offersFreeCredit", () => {
  test("offers it for a job that does not use it yet", () => {
    expect(offersFreeCredit(status(), "assistant")).toBe(true);
    expect(
      offersFreeCredit(
        status({ serving: { ...NOTHING, assistant: true } }),
        "assistant",
      ),
    ).toBe(false);
  });

  test("never offers it signed out, used up, switched off or unknown", () => {
    expect(offersFreeCredit(null, "stt")).toBe(false);
    expect(offersFreeCredit(status({ signed_in: false }), "stt")).toBe(false);
    expect(offersFreeCredit(status({ exhausted: true }), "stt")).toBe(false);
    expect(offersFreeCredit(status({ balance: null }), "stt")).toBe(false);
  });

  test("knows when a job runs on it", () => {
    expect(servesAnyJob(status())).toBe(false);
    expect(servesAnyJob(status({ serving: { ...NOTHING, voice: true } }))).toBe(
      true,
    );
    expect(
      servesAnyJob(
        status({ signed_in: false, serving: { ...NOTHING, voice: true } }),
      ),
    ).toBe(false);
  });
});

test("provider lists leave it out", () => {
  expect(
    withoutTalvioFree([{ id: "openai" }, { id: "talvio_free" }]).map(
      (p) => p.id,
    ),
  ).toEqual(["openai"]);
});
