import React, { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "@/hooks/useSettings";
import { Keycaps } from "@/components/ui/Keycaps";
import { LevelBars, restingVoice, Stage } from "@/components/ui/Stage";
import { useNavigation } from "@/components/shell/navigation";
import { Button } from "@/components/ui/Button";
import { ArrowRight } from "@/components/icons/glyphs";
import {
  TONE_DOT,
  useListeningStatus,
} from "@/components/shell/useListeningStatus";
import { useInsights } from "../insights/useInsights";
import { formatCount, formatDuration, timeSavedSeconds } from "./usageMath";

/** One number from Insights: a quiet label over the figure. The whole cell
 *  opens the page. */
const Figure: React.FC<{
  value: string;
  label: string;
  onClick: () => void;
}> = ({ value, label, onClick }) => (
  <button
    type="button"
    onClick={onClick}
    className="flex min-w-0 cursor-pointer flex-col items-start gap-0.5 px-6 py-4 text-start transition-colors hover:bg-[var(--fill-hover)] focus-visible:relative focus-visible:shadow-[var(--focus-ring)] focus-visible:outline-none"
  >
    <span className="block max-w-full truncate text-[0.8125rem] text-muted">
      {label}
    </span>
    <span className="block max-w-full truncate font-display text-[1.25rem] leading-7 text-ink tabular-nums">
      {value}
    </span>
  </button>
);

/**
 * The top of Home, in one card: whether Talvio can listen right now and with
 * which model, the keys that start a dictation, and a voice line that
 * breathes while the speech model is ready (flat and grey when it is not).
 * Along its foot, three numbers that open Insights. The first thing on the
 * page answers "can I talk now?". When it cannot (`needsModel`), the card
 * says so and carries the one action that fixes it.
 */
export const StatusHero: React.FC<{ needsModel?: boolean }> = ({
  needsModel = false,
}) => {
  const { t, i18n } = useTranslation();
  const { getSetting } = useSettings();
  const { openModelSlot, navigate } = useNavigation();
  const status = useListeningStatus();
  const { insights } = useInsights();
  const binding = getSetting("bindings")?.transcribe?.current_binding;
  const holdToTalk = getSetting("push_to_talk") ?? true;
  const stats = insights?.dictation;
  const streak = insights?.current_streak_days ?? 0;
  const live = status.tone === "ready";
  const voice = useMemo(() => restingVoice(48, 1.3), []);

  const figures = stats && stats.total_words > 0 && (
    <div
      className="grid grid-cols-1 divide-y divide-hairline @2xl:grid-cols-3 @2xl:divide-x @2xl:divide-y-0"
      role="group"
      aria-label={t("home.stats.label")}
    >
      <Figure
        value={formatCount(stats.total_words, i18n.language)}
        label={t("home.stats.wordsLabel")}
        onClick={() => navigate("insights")}
      />
      <Figure
        value={formatDuration(timeSavedSeconds(stats), t)}
        label={t("home.stats.savedLabel")}
        onClick={() => navigate("insights")}
      />
      <Figure
        value={String(streak)}
        label={t("home.stats.streakLabel", { count: streak })}
        onClick={() => navigate("insights")}
      />
    </div>
  );

  return (
    <Stage
      kicker={
        <span className="inline-flex items-center gap-2">
          <span
            aria-hidden="true"
            className={`h-1.5 w-1.5 shrink-0 rounded-full ${TONE_DOT[status.tone]}`}
          />
          {t("modelsHub.slots.stt.name")}
        </span>
      }
      title={
        <button
          type="button"
          onClick={() => openModelSlot("stt")}
          title={status.label}
          className="-mx-1 max-w-full cursor-pointer truncate rounded-md px-1 text-start transition-colors hover:bg-[var(--fill-hover)] focus-visible:outline-none focus-visible:shadow-[var(--focus-ring)]"
        >
          {status.name}
        </button>
      }
      subtitle={needsModel ? t("home.needsModel.title") : status.statusText}
      actions={
        needsModel ? (
          <Button size="md" onClick={() => openModelSlot("stt")}>
            {t("home.needsModel.action")}
            <ArrowRight
              className="h-3.5 w-3.5 rtl:rotate-180"
              aria-hidden="true"
            />
          </Button>
        ) : (
          binding && (
            <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
              <Keycaps binding={binding} size="md" />
              <p className="max-w-[18rem] text-[0.8125rem] leading-snug text-muted text-pretty">
                {holdToTalk ? t("home.subtitleHold") : t("home.subtitleTap")}
              </p>
            </div>
          )
        )
      }
      picture={
        <div
          className="voice-line flex w-full items-center px-6 py-6"
          data-live={live}
        >
          <LevelBars
            levels={live ? voice : voice.map((level) => level * 0.18 + 0.04)}
            idle={live}
            className="h-14 w-full justify-center"
          />
        </div>
      }
      footer={figures || undefined}
    />
  );
};
