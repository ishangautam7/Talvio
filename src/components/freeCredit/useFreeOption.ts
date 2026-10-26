import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useFreeCredit } from "@/stores/freeCreditStore";
import { formatUsdAmount, freeCreditView } from "@/lib/freeCredit";

/**
 * Talvio free as a choice in a model or voice picker: whether to offer it
 * (signed in, with credit left) and what to call it ("Talvio free ($1
 * included)").
 */
export const useFreeOption = () => {
  const { t, i18n } = useTranslation();
  const { status } = useFreeCredit();
  return useMemo(() => {
    const view = freeCreditView(status);
    const limit =
      view.kind === "available" || view.kind === "exhausted" ? view.limit : 1;
    return {
      available: view.kind === "available",
      label: t("freeCredit.option", {
        amount: formatUsdAmount(limit, i18n.language),
      }),
    };
  }, [status, t, i18n.language]);
};
