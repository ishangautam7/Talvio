import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import {
  commands,
  type AccountStatus,
  type FreeCreditJob,
  type FreeCreditStatus,
} from "@/bindings";

/** How soon focusing the window asks the server for the balance again. */
const FOCUS_REFRESH_MS = 30_000;

interface FreeCreditStore {
  status: FreeCreditStatus | null;
  /** A "Use Talvio free" request is running for this job. */
  choosing: FreeCreditJob | null;
  /** Point one job at Talvio free. Resolves to an error code, or null. */
  choose: (job: FreeCreditJob) => Promise<string | null>;
}

export const useFreeCreditStore = create<FreeCreditStore>((set) => ({
  status: null,
  choosing: null,
  choose: async (job) => {
    set({ choosing: job });
    try {
      const result = await commands.chooseFreeCredit(job);
      if (result.status === "ok") {
        set({ status: result.data });
        return null;
      }
      return result.error;
    } catch (error) {
      console.error("Failed to choose Talvio free:", error);
      return "server";
    } finally {
      set({ choosing: null });
    }
  },
}));

let started = false;
let lastRefresh = 0;

/**
 * Keep the status current for the window: read it once, take every
 * `free-credit-changed` (Rust sends one after each metered answer and every
 * 402), read it again when the account changes, and ask the server for the
 * balance when the window regains focus.
 */
const start = () => {
  if (started || typeof window === "undefined") return;
  started = true;
  const put = (status: FreeCreditStatus) =>
    useFreeCreditStore.setState({ status });
  const load = () =>
    commands
      .getFreeCreditStatus()
      .then(put)
      .catch((error) =>
        console.error("Failed to load the free credit status:", error),
      );
  void listen<FreeCreditStatus>("free-credit-changed", (event) =>
    put(event.payload),
  ).catch(() => {});
  void listen<AccountStatus>("account-status-changed", () => void load()).catch(
    () => {},
  );
  // A setting changed elsewhere can move a job on or off Talvio free.
  void listen("assistant-settings-changed", () => void load()).catch(() => {});
  window.addEventListener("focus", () => {
    const now = Date.now();
    if (now - lastRefresh < FOCUS_REFRESH_MS) return;
    lastRefresh = now;
    void commands
      .refreshFreeCredit()
      .then(put)
      .catch(() => {});
  });
  void load();
};

/** The free credit's status, kept current. */
export const useFreeCredit = () => {
  start();
  return useFreeCreditStore();
};
