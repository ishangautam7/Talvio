import React, { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "@/hooks/useSettings";
import { Keycaps } from "@/components/ui/Keycaps";
import { LevelBars, restingVoice, Stage } from "@/components/ui/Stage";
import {
  ArrowRight,
  Clock,
  DictateGlyph,
  Flame,
} from "@/components/icons/glyphs";
import { useNavigation } from "@/components/shell/navigation";
import {
  TONE_DOT,
  useListeningStatus,
} from "@/components/shell/useListeningStatus";
import { useInsights } from "../insights/useInsights";
import { formatCount, formatDuration, timeSavedSeconds } from "./usageMath";

/** One number from Insights, in a small card that opens the page. */
const Figure: React.FC<{
  hue: string;
  icon: React.ReactNode;
  value: string;
  label: string;
  onClick: () => void;
}> = ({ hue, icon, value, label, onClick }) => (
  <button
    type="button"
    onClick={onClick}
    data-hue={hue}
    className="group elev-card elev-hover flex min-w-0 cursor-pointer items-center gap-3.5 rounded-2xl border border-hairline bg-surface px-4 py-3.5 text-start focus-visible:outline-none focus-visible:shadow-[var(--focus-ring)]"
  >
    <span className="hue-tile grid h-10 w-10 shrink-0 place-items-center rounded-xl">
      {icon}
    </span>
    <span className="min-w-0 flex-1">
      <span className="block truncate font-display text-[1.375rem] leading-tight text-ink tabular-nums">
        {value}
      </span>
      <span className="block truncate text-[0.8125rem] text-muted">
        {label}
      </span>
    </span>
    <ArrowRight
      className="h-4 w-4 shrink-0 text-muted-soft opacity-0 transition-[opacity,translate] group-hover:translate-x-0.5 group-hover:opacity-100 rtl:rotate-180 rtl:group-hover:-translate-x-0.5"
      aria-hidden="true"
    />
  </button>
);

/**
 * The top of Home: whether Talvio can listen right now and with which model,
 * the keys that start a dictation, and a voice line that breathes while the
 * speech model is ready (flat and grey when it is not). Under it, three
 * numbers that open Insights. The first thing on the page answers "can I
 * talk now?".
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
  const live = status.tone === "ready";
  const voice = useMemo(() => restingVoice(44, 1.3), []);

  return (
    <>
      <Stage
        kicker={
          <span className="inline-flex items-center gap-2">
            <span
              aria-hidden="true"
              className={`status-pip h-2 w-2 shrink-0 rounded-full ${TONE_DOT[status.tone]}`}
            />
            {t("modelsHub.slots.stt.name")}
          </span>
        }
        title={
          <button
            type="button"
            onClick={() => openModelSlot("stt")}
            title={status.label}
            className="-mx-1 max-w-full cursor-pointer truncate rounded-lg px-1 text-start transition-colors hover:bg-ink/[0.04] focus-visible:outline-none focus-visible:shadow-[var(--focus-ring)]"
          >
            {status.name}
          </button>
        }
        subtitle={status.statusText}
        actions={
          binding && (
            <div className="flex flex-wrap items-center gap-x-4 gap-y-2.5">
              <Keycaps binding={binding} size="lg" />
              <p className="max-w-[17rem] text-sm leading-snug text-body text-pretty">
                {holdToTalk ? t("home.subtitleHold") : t("home.subtitleTap")}
              </p>
            </div>
          )
        }
        picture={
          <div
            className="voice-line flex w-full items-center px-5 py-6"
            data-live={live}
          >
            <LevelBars
              levels={live ? voice : voice.map((level) => level * 0.18 + 0.04)}
              idle={live}
              className="h-[4.5rem] w-full justify-center"
            />
          </div>
        }
      />

      {stats && stats.total_words > 0 && (
        <div
          className="mt-4 grid grid-cols-1 gap-3 @2xl:grid-cols-3"
          role="group"
          aria-label={t("home.stats.label")}
        >
          <Figure
            hue="home"
            icon={<DictateGlyph className="h-5 w-5" />}
            value={formatCount(stats.total_words, i18n.language)}
            label={t("home.stats.wordsLabel")}
            onClick={() => navigate("insights")}
          />
          <Figure
            hue="history"
            icon={<Clock className="h-5 w-5" />}
            value={formatDuration(timeSavedSeconds(stats), t)}
            label={t("home.stats.savedLabel")}
            onClick={() => navigate("insights")}
          />
          <Figure
            hue="commands"
            icon={<Flame className="h-5 w-5" />}
            value={String(streak)}
            label={t("home.stats.streakLabel", { count: streak })}
            onClick={() => navigate("insights")}
          />
        </div>
      )}
    </>
  );
};
