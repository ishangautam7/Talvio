import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { Mic, Volume2 } from "@/components/icons/glyphs";
import { LevelBars, restingVoice } from "@/components/ui/Stage";
import { MEETING_LEVEL_EVENT, type MeetingLevels } from "./api";

/** How many bars each lane shows. */
const BARS = 34;
/** How often a lane takes the newest level, in ms. */
const STEP_MS = 90;
/** Below this a stream counts as silent. Matches the pill. */
export const SILENT_LEVEL = 0.02;

/** A level as a bar height: quiet speech is a fraction of full scale, so the
 *  curve lifts the low end where most of a voice lives. */
const shape = (level: number) => Math.min(1, Math.sqrt(Math.max(0, level)) * 1.6);

/**
 * The Meetings picture: two lanes, one per side of a call — you on the
 * microphone, everyone else on the computer's sound — because a meeting is
 * two streams and the most common failure is one of them staying silent.
 *
 * At rest each lane shows the shape a sentence makes, breathing slowly. While
 * recording the lanes are the real levels (`meeting-level`), scrolling from
 * the end edge, so the user can watch both sides arrive. A side that is not
 * being captured at all is drawn flat and marked, because quiet and missing
 * must not look alike.
 */
export const MeetingLanes: React.FC<{
  live?: boolean;
  paused?: boolean;
  systemCaptured?: boolean;
}> = ({ live = false, paused = false, systemCaptured = true }) => {
  const { t } = useTranslation();
  const latest = useRef<MeetingLevels>({ mic: 0, system: 0 });
  const [history, setHistory] = useState(() => ({
    mic: new Array<number>(BARS).fill(0.04),
    system: new Array<number>(BARS).fill(0.04),
  }));

  useEffect(() => {
    if (!live) return;
    const unlisten = listen<MeetingLevels>(MEETING_LEVEL_EVENT, (event) => {
      latest.current = event.payload;
    });
    const tick = window.setInterval(() => {
      const now = latest.current;
      // Levels stop arriving the instant capture stops, which would freeze
      // a lane mid-word, so the held value decays between events.
      latest.current = { mic: now.mic * 0.7, system: now.system * 0.7 };
      if (paused) return;
      setHistory((current) => ({
        mic: [...current.mic.slice(1), Math.max(0.04, shape(now.mic))],
        system: [...current.system.slice(1), Math.max(0.04, shape(now.system))],
      }));
    }, STEP_MS);
    return () => {
      window.clearInterval(tick);
      void unlisten.then((off) => off());
    };
  }, [live, paused]);

  const lane = (
    key: "mic" | "system",
    label: string,
    icon: React.ReactNode,
    captured: boolean,
    seed: number,
  ) => {
    const levels = live
      ? captured
        ? history[key]
        : new Array<number>(BARS).fill(0.03)
      : restingVoice(BARS, seed);
    const speaking =
      live && captured && !paused && latest.current[key] >= SILENT_LEVEL;
    return (
      <div className="flex min-w-0 items-center gap-3">
        <span
          className={`inline-flex w-[5.5rem] shrink-0 items-center gap-1.5 text-[0.8125rem] font-medium ${
            captured ? "text-ink" : "text-warning line-through"
          }`}
        >
          <span
            className={`grid h-6 w-6 shrink-0 place-items-center rounded-full ${
              speaking ? "bg-hue text-white" : "hue-tile"
            } transition-colors`}
          >
            {icon}
          </span>
          <span className="truncate">{label}</span>
        </span>
        <LevelBars
          levels={levels}
          idle={!live}
          className={`h-10 min-w-0 flex-1 ${
            key === "mic" ? "text-hue" : "text-[var(--hue-assistant)]"
          } ${paused ? "opacity-40" : ""} ${!captured ? "text-warning" : ""}`}
        />
      </div>
    );
  };

  return (
    <div
      className="flex w-full flex-col justify-center gap-4 px-4 py-5"
      aria-hidden="true"
    >
      {lane(
        "mic",
        t("meetings.speakers.me"),
        <Mic className="h-3.5 w-3.5" />,
        true,
        1,
      )}
      <div className="ms-[6.25rem] h-px bg-hairline" />
      {lane(
        "system",
        t("meetings.speakers.others"),
        <Volume2 className="h-3.5 w-3.5" />,
        systemCaptured,
        2.3,
      )}
    </div>
  );
};
