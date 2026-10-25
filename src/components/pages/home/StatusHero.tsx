import React from "react";
import { useTranslation } from "react-i18next";
import { ArrowRight } from "lucide-react";
import { useSettings } from "@/hooks/useSettings";
import { Keycaps } from "@/components/ui/Keycaps";
import { useNavigation } from "@/components/shell/navigation";
import {
  TONE_DOT,
  useListeningStatus,
} from "@/components/shell/useListeningStatus";
import { useInsights } from "../insights/useInsights";
import { formatCount, formatDuration, timeSavedSeconds } from "./usageMath";

/** Five bars that move like a voice, the logo's waveform, while the speech
 *  model is ready. Still under reduced motion. */
const LiveBars: React.FC<{ live: boolean }> = ({ live }) => (
  <span
    aria-hidden="true"
    className={`live-bars ${live ? "is-live" : ""}`}
    data-live={live}
  >
    <span />
    <span />
    <span />
    <span />
    <span />
  </span>
);

const Figure: React.FC<{ value: string; label: string }> = ({
  value,
  label,
}) => (
  <span className="flex min-w-0 flex-col gap-1">
    <span className="font-display text-[1.75rem] leading-none text-ink tabular-nums">
      {value}
    </span>
    <span className="eyebrow truncate text-muted">{label}</span>
  </span>
);

/**
 * The top of Home: whether Talvio is ready to listen, with which model, the
 * keys that start a dictation, and three numbers that open Insights. The
 * first thing on the page answers "can I talk now?".
 */
export const StatusHero: React.FC = () => {
  const { t, i18n } = useTranslation();
  const { getSetting } = useSettings();
  const { openModelSlot, navigate } = useNavigation();
  const status = useListeningStatus();
  const { insights } = useInsights();
  const binding = getSetting("bindings")?.transcribe?.current_binding;
  const holdToTalk = getSetting("push_to_talk") ?? true;
  const stats = insights?.dictation;
  const streak = insights?.current_streak_days ?? 0;

  return (
    <section className="status-hero @container relative isolate overflow-hidden rounded-[1.375rem] border border-hairline bg-surface elev-card">
      <div className="grid gap-x-8 gap-y-6 p-6 @3xl:grid-cols-[minmax(0,1fr)_auto] @3xl:items-center">
        <div className="min-w-0">
          <button
            type="button"
            onClick={() => openModelSlot("stt")}
            title={status.label}
            className="group -m-1.5 flex max-w-full cursor-pointer items-center gap-3 rounded-xl p-1.5 text-start transition-colors hover:bg-ink/[0.035] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40"
          >
            <span className="status-orb grid h-12 w-12 shrink-0 place-items-center rounded-2xl">
              <LiveBars live={status.tone === "ready"} />
            </span>
            <span className="min-w-0">
              <span className="eyebrow flex items-center gap-1.5 text-muted">
                <span
                  aria-hidden="true"
                  className={`h-1.5 w-1.5 shrink-0 rounded-full ${TONE_DOT[status.tone]}`}
                />
                {t("modelsHub.slots.stt.name")}
              </span>
              <span className="mt-0.5 block truncate font-display text-[1.375rem] leading-tight text-ink">
                {status.name}
              </span>
              <span className="block truncate text-[0.8125rem] text-muted">
                {status.statusText}
              </span>
            </span>
          </button>

          {binding && (
            <div className="mt-5 flex flex-wrap items-center gap-x-4 gap-y-2">
              <Keycaps binding={binding} size="lg" />
              <p className="max-w-sm text-sm leading-relaxed text-body text-pretty">
                {holdToTalk ? t("home.subtitleHold") : t("home.subtitleTap")}
              </p>
            </div>
          )}
        </div>

        {stats && stats.total_words > 0 && (
          <button
            type="button"
            onClick={() => navigate("insights")}
            aria-label={`${t("home.stats.label")}. ${t("home.stats.seeInsights")}`}
            className="group grid cursor-pointer grid-cols-3 gap-6 rounded-2xl border border-hairline bg-surface-muted px-5 py-4 text-start transition-colors hover:border-hairline-strong focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40"
          >
            <Figure
              value={formatCount(stats.total_words, i18n.language)}
              label={t("home.stats.wordsLabel")}
            />
            <Figure
              value={formatDuration(timeSavedSeconds(stats), t)}
              label={t("home.stats.savedLabel")}
            />
            <Figure
              value={String(streak)}
              label={t("home.stats.streakLabel", { count: streak })}
            />
            <span className="col-span-3 -mt-1 inline-flex items-center gap-1 text-[0.8125rem] font-medium text-muted transition-colors group-hover:text-ink">
              {t("home.stats.seeInsights")}
              <ArrowRight
                className="h-3.5 w-3.5 transition-transform group-hover:translate-x-0.5 rtl:rotate-180 rtl:group-hover:-translate-x-0.5"
                aria-hidden="true"
              />
            </span>
          </button>
        )}
      </div>
    </section>
  );
};
