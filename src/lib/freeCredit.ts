import type { FreeCreditJob, FreeCreditStatus } from "@/bindings";

/**
 * Talvio free: the $1 of AI usage every signed-in Talvio account gets,
 * served by the account server (`src-tauri/src/free_credit.rs`). Rust owns
 * the session, the balance and which jobs run on it; these are the pure
 * rules the UI draws from its `FreeCreditStatus`.
 */

/** The provider id in every job's registry. */
export const FREE_PROVIDER_ID = "talvio_free";
/** Its name, a product name and so the same in every language. */
export const FREE_PROVIDER_LABEL = "Talvio free";
/** The chat model the free API serves (`GET /v1/free/models`). */
export const FREE_CHAT_MODEL = "talvio-free-chat";

/** Every job Talvio free can do, in the order the UI lists them. */
export const FREE_CREDIT_JOBS: readonly FreeCreditJob[] = [
  "stt",
  "cleanup",
  "assistant",
  "command",
  "voice",
];

/** Whether a provider or voice engine id is Talvio free. */
export const isTalvioFree = (id: string | null | undefined): boolean =>
  id === FREE_PROVIDER_ID;

/** Lists of providers the user picks from never show Talvio free: it has
 *  its own "Use Talvio free" button and needs no key or address. */
export const withoutTalvioFree = <T extends { id: string }>(
  providers: readonly T[],
): T[] => providers.filter((provider) => !isTalvioFree(provider.id));

/** What the UI shows about the free credit. */
export type FreeCreditView =
  /** Nobody is signed in: there is no free credit to show. */
  | { kind: "signed_out" }
  /** Signed in, and the server has not answered yet. */
  | { kind: "loading" }
  /** The server has switched the free credit off. */
  | { kind: "disabled" }
  /** All of it is spent. */
  | { kind: "exhausted"; limit: number }
  | { kind: "available"; remaining: number; limit: number; used: number };

export const freeCreditView = (
  status: FreeCreditStatus | null | undefined,
): FreeCreditView => {
  if (!status?.signed_in) return { kind: "signed_out" };
  const balance = status.balance;
  if (status.exhausted) {
    return { kind: "exhausted", limit: balance?.limit_usd ?? 1 };
  }
  if (!balance) return { kind: "loading" };
  if (!balance.enabled) return { kind: "disabled" };
  if (balance.remaining_usd <= 0) {
    return { kind: "exhausted", limit: balance.limit_usd };
  }
  return {
    kind: "available",
    remaining: balance.remaining_usd,
    limit: balance.limit_usd,
    used: balance.used_usd,
  };
};

/** "$0.73". Whole cents, in the reader's own number format. */
export const formatUsd = (value: number, locale?: string): string => {
  const safe = Number.isFinite(value) ? Math.max(0, value) : 0;
  try {
    return new Intl.NumberFormat(locale, {
      style: "currency",
      currency: "USD",
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    }).format(safe);
  } catch {
    return `$${safe.toFixed(2)}`;
  }
};

/** "$1" for a whole amount, "$0.50" otherwise: how the allowance is named. */
export const formatUsdAmount = (value: number, locale?: string): string => {
  const safe = Number.isFinite(value) ? Math.max(0, value) : 0;
  if (Math.round(safe * 100) % 100 !== 0) return formatUsd(safe, locale);
  try {
    return new Intl.NumberFormat(locale, {
      style: "currency",
      currency: "USD",
      minimumFractionDigits: 0,
      maximumFractionDigits: 0,
    }).format(safe);
  } catch {
    return `$${Math.round(safe)}`;
  }
};

/** How much of the credit is left, 0–1, for the meter. */
export const remainingFraction = (view: FreeCreditView): number => {
  if (view.kind !== "available" || view.limit <= 0) return 0;
  return Math.min(1, Math.max(0, view.remaining / view.limit));
};

/**
 * Whether to offer "Use Talvio free" for `job`: someone is signed in, there is
 * credit left, and the job does not already run on it.
 */
export const offersFreeCredit = (
  status: FreeCreditStatus | null | undefined,
  job: FreeCreditJob,
): boolean =>
  freeCreditView(status).kind === "available" && !status?.serving[job];

/** Whether any job runs on Talvio free right now. */
export const servesAnyJob = (
  status: FreeCreditStatus | null | undefined,
): boolean =>
  !!status?.signed_in &&
  FREE_CREDIT_JOBS.some((job) => status.serving[job] === true);
