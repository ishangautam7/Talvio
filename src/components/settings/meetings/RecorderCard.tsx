import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Eye, EyeOff, Pause, Play, Square } from "@/components/icons/glyphs";
import { Stage } from "@/components/ui/Stage";
import { Button } from "@/components/ui/Button";
import { SectionTitle } from "@/components/ui/Page";
import { type MeetingSpeaker, type MeetingState } from "./api";
import { MeetingLanes } from "./MeetingLanes";
import { useMeetingIndicator } from "./CallDetectionToggle";
import { formatClock, type TranscriptItem } from "./speakers";
import { SystemAudioNotice } from "./SystemAudioNotice";
import { TranscriptView } from "./TranscriptView";

interface RecorderCardProps {
  state: MeetingState;
  /** Which long-running call is in flight, so the buttons can say so. Stopping
   *  drains the whole transcription queue. */
  busy: "starting" | "stopping" | null;
  liveItems: readonly TranscriptItem[];
  speakers: readonly MeetingSpeaker[];
  onStop: () => void;
  onTogglePause: () => void;
}

/**
 * A meeting that is recording right now, in the main window.
 *
 * The page's stage becomes the recorder: the clock in place of the headline,
 * the two lanes now fed by the real levels of each side, and the two things
 * you can do (stop, pause). Starting a meeting used to replace the banner with a
 * plain white card, so the moment the page mattered most it looked least like
 * the rest of the app. The live transcript sits underneath as an ordinary
 * section.
 *
 * The elapsed time ticks locally rather than waiting for the backend. State
 * events only arrive when something changes — a segment lands, the user pauses —
 * so a clock driven by them alone would sit still through a silence and look
 * frozen. `elapsed_ms` from the last event is the anchor; wall time since then is
 * added on top, and paused time is excluded because the backend's counter is the
 * microphone accumulator, which drops paused frames rather than buffering them.
 */
export const RecorderCard: React.FC<RecorderCardProps> = ({
  state,
  busy,
  liveItems,
  speakers,
  onStop,
  onTogglePause,
}) => {
  const { t } = useTranslation();
  const indicator = useMeetingIndicator();
  const paused = state.paused;

  const anchor = useRef({ elapsed: state.elapsed_ms, at: Date.now() });
  const [tick, setTick] = useState(() => Date.now());
  useEffect(() => {
    anchor.current = { elapsed: state.elapsed_ms, at: Date.now() };
    setTick(Date.now());
  }, [state.elapsed_ms, paused]);
  useEffect(() => {
    if (paused) return;
    const id = window.setInterval(() => setTick(Date.now()), 500);
    return () => window.clearInterval(id);
  }, [paused]);

  const elapsedMs = paused
    ? anchor.current.elapsed
    : anchor.current.elapsed + Math.max(0, tick - anchor.current.at);
  const clock = formatClock(elapsedMs);
  const statusLabel = paused
    ? t("meetings.recorder.paused")
    : t("meetings.recorder.recording");

  return (
    <div className="w-full">
      <Stage
        wide
        label={`${statusLabel} ${clock}`}
        kicker={
          <span className="flex items-center gap-2" role="status">
            <span
              className="rec-dot"
              data-paused={String(paused)}
              aria-hidden="true"
            />
            {statusLabel}
          </span>
        }
        title={
          <span
            className="rec-clock"
            data-paused={String(paused)}
            aria-hidden="true"
          >
            {clock}
          </span>
        }
        actions={
          <div className="flex flex-wrap items-center gap-2.5">
            <button
              type="button"
              onClick={onStop}
              disabled={busy !== null}
              className="stage-cta inline-flex h-11 cursor-pointer items-center justify-center gap-2 rounded-full ps-5 pe-6 text-[0.9375rem] font-semibold disabled:cursor-wait disabled:opacity-70"
            >
              <Square className="h-3.5 w-3.5 fill-current" aria-hidden="true" />
              {busy === "stopping"
                ? t("meetings.recorder.stopping")
                : t("meetings.pill.stop")}
            </button>
            <Button
              variant="secondary"
              size="lg"
              onClick={onTogglePause}
              disabled={busy !== null}
              className="rounded-full"
            >
              {paused ? (
                <Play className="h-4 w-4" aria-hidden="true" />
              ) : (
                <Pause className="h-4 w-4" aria-hidden="true" />
              )}
              {paused
                ? t("meetings.recorder.resume")
                : t("meetings.recorder.pause")}
            </Button>
          </div>
        }
        picture={
          <MeetingLanes
            live
            paused={paused}
            systemCaptured={state.system_audio}
          />
        }
        corner={
          indicator.enabled !== null && (
            // The floating pill, from the one place that is still on screen
            // when it has been switched off.
            <button
              type="button"
              onClick={() => indicator.set(!indicator.enabled)}
              disabled={indicator.saving}
              title={
                indicator.enabled
                  ? t("meetings.indicator.hide")
                  : t("meetings.indicator.show")
              }
              aria-label={
                indicator.enabled
                  ? t("meetings.indicator.hide")
                  : t("meetings.indicator.show")
              }
              aria-pressed={indicator.enabled}
              className="grid h-8 w-8 cursor-pointer place-items-center rounded-full bg-surface text-muted shadow-[var(--shadow-raised)] transition-colors hover:text-ink focus-visible:outline-none focus-visible:shadow-[var(--focus-ring)] disabled:cursor-wait"
            >
              {indicator.enabled ? (
                <Eye className="h-4 w-4" aria-hidden="true" />
              ) : (
                <EyeOff className="h-4 w-4" aria-hidden="true" />
              )}
            </button>
          )
        }
      />

      {busy === "stopping" && (
        <p className="mt-3 px-1 text-sm text-muted" role="status">
          {t("meetings.recorder.stoppingHint")}
        </p>
      )}

      {/* During a recording the session's actual failure is the warning worth
          showing, right under the thing it is about. */}
      {!state.system_audio && (
        <div className="mt-4">
          <SystemAudioNotice live detail={state.system_audio_error} />
        </div>
      )}

      <section className="mt-8">
        <SectionTitle title={t("meetingsPage.transcript")} />
        <div className="overflow-hidden rounded-2xl border border-hairline bg-surface elev-card">
          <TranscriptView
            items={liveItems}
            speakers={speakers}
            stickToBottom
            heightClassName="max-h-[26rem]"
            emptyLabel={t("meetings.recorder.transcriptEmpty")}
            resetKey={state.meeting_id ?? "idle"}
            // Copy during the call, for pasting what was just said into a chat
            // or a doc without waiting for the meeting to end.
            copyable
          />
        </div>
      </section>
    </div>
  );
};
