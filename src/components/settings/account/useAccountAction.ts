import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type { AccountStatus, Result } from "@/bindings";
import { accountErrorKey } from "./account";

/**
 * One account command at a time from a signed-in screen (sign out, remove a
 * device, delete the account), with a toast of the error's own sentence when
 * it fails. `busy` names the command still running. The new status arrives as
 * `account-status-changed`, which every command emits before it returns; its
 * own snapshot is not applied, because the two travel separately and an
 * older snapshot could land after a newer event.
 */
export function useAccountAction() {
  const { t } = useTranslation();
  const [busy, setBusy] = useState<string | null>(null);

  const run = async (
    kind: string,
    call: () => Promise<Result<AccountStatus, string>>,
  ) => {
    setBusy(kind);
    try {
      const result = await call();
      if (result.status === "ok") return;
      const key = accountErrorKey(result.error);
      if (key) toast.error(t(key));
    } catch (failure) {
      console.error(`Account action "${kind}" failed:`, failure);
      toast.error(t("settings.account.errors.generic"));
    } finally {
      setBusy(null);
    }
  };

  return { busy, run };
}
