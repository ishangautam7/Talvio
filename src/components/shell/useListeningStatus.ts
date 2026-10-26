import { useTranslation } from "react-i18next";
import { useModelSlots } from "./useModelSlots";
import { useSttLoadState } from "./useSttStatus";

export type ListeningTone = "ready" | "setup" | "loading" | "error" | "idle";

/**
 * Which speech model is listening and whether it is up, in words and as a
 * tone, for the rail's status dot and Home's status card, so the two can never
 * say different things.
 */
export const useListeningStatus = (): {
  name: string;
  statusText: string;
  tone: ListeningTone;
  isCloud: boolean;
  label: string;
  ready: boolean;
} => {
  const { t } = useTranslation();
  const slots = useModelSlots();
  const stt = slots.stt;
  const { state } = useSttLoadState();
  const isCloud = stt.where === "cloud";

  const tone: ListeningTone = !stt.ready
    ? "setup"
    : isCloud || state === "ready"
      ? "ready"
      : state === "loading"
        ? "loading"
        : state === "error"
          ? "error"
          : "idle";

  const statusText = !stt.ready
    ? stt.issue === "needs_update"
      ? t("nav.status.needsUpdate")
      : t("nav.status.setup")
    : isCloud
      ? t("nav.status.cloud", { provider: stt.providerLabel ?? "" })
      : state === "loading"
        ? t("nav.status.loading")
        : state === "error"
          ? t("nav.status.error")
          : state === "ready"
            ? t("nav.status.ready")
            : t("nav.status.idle");

  const name = stt.modelLabel ?? t("nav.status.noModel");
  return {
    name,
    statusText,
    tone,
    isCloud,
    label: t("nav.status.label", { model: name, status: statusText }),
    ready: stt.ready,
  };
};

/** The dot's colour for each tone. */
export const TONE_DOT: Record<ListeningTone, string> = {
  ready: "bg-emerald-400",
  setup: "bg-amber-400",
  loading: "bg-sky-400 animate-pulse",
  error: "bg-error",
  idle: "bg-muted",
};
