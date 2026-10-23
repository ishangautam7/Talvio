import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Zap } from "lucide-react";
import { commands, type CommandCard } from "@/bindings";
import { feedbackOptions } from "@/components/settings/debug/commandReport";
import {
  AFTER_HOVER_MS,
  AUTO_CLOSE_MS,
  answerChoices,
  cardHeading,
  closesItself,
  commandCardKind,
} from "./commandCard";

/**
 * A spoken command's card (`src-tauri/src/command_mode/voice.rs`): what was
 * heard and what happened. A command that waits for "send" shows what will be
 * sent, with Send and Cancel; one that asks a question shows its choices. A
 * card that only reports closes itself after a few seconds, and stays while
 * the pointer is on it.
 *
 * `armed` is the popup's guard against a click aimed at something else
 * landing on a card that just appeared or moved.
 */
export const CommandCardView: React.FC<{
  card: CommandCard;
  armed: () => boolean;
}> = ({ card, armed }) => {
  const { t } = useTranslation();
  const { seq, report } = card;
  const kind = commandCardKind(report);
  const [busy, setBusy] = useState(false);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // A new card is a new start: nothing pressed on it yet.
  useEffect(() => setBusy(false), [seq]);

  const stopTimer = () => {
    if (closeTimer.current !== null) clearTimeout(closeTimer.current);
    closeTimer.current = null;
  };
  const startTimer = (ms: number) => {
    stopTimer();
    if (!closesItself(report)) return;
    closeTimer.current = setTimeout(
      () => void commands.commandCardDismiss(seq),
      ms,
    );
  };

  useEffect(() => {
    startTimer(AUTO_CLOSE_MS);
    return stopTimer;
    // The timer belongs to this card: restarted only when the card changes.
  }, [seq]);

  const act = (action: () => Promise<void>) => {
    if (busy || !armed()) return;
    setBusy(true);
    void action().finally(() => setBusy(false));
  };

  const choices = answerChoices(report);

  return (
    <section
      className="ask-card"
      aria-labelledby={`command-${seq}-heard`}
      aria-describedby={`command-${seq}-body`}
      onMouseEnter={stopTimer}
      onMouseLeave={() => startTimer(AFTER_HOVER_MS)}
    >
      <header className="ask-head" data-tauri-drag-region>
        <Zap className="ask-head-icon" size={12} />
        <p className="ask-question-text" id={`command-${seq}-heard`}>
          {cardHeading(report, t)}
        </p>
      </header>
      <div
        className="ask-answer-body"
        id={`command-${seq}-body`}
        aria-live="polite"
      >
        {report.feedback.map((feedback, index) => (
          <p key={index}>{t(feedback.key, feedbackOptions(feedback))}</p>
        ))}
        {kind === "confirm" && report.pending_summary && (
          <p className="command-summary">{report.pending_summary}</p>
        )}
        {kind === "question" && report.question && (
          <p className="approval-phone">{report.question}</p>
        )}
        {kind === "question" && choices.length === 0 && (
          <p className="approval-platform">{t("commandCard.sayAgain")}</p>
        )}
        {report.dry_run && (
          <p className="approval-platform">{t("commandCard.dryRun")}</p>
        )}
        {busy && (
          <p className="approval-platform">{t("commandCard.working")}</p>
        )}
      </div>
      {kind === "confirm" && (
        <div className="approval-actions">
          <button
            type="button"
            className="qa-btn"
            disabled={busy}
            onClick={() => act(() => commands.commandCardConfirm(seq, false))}
          >
            {t("commandCard.cancel")}
          </button>
          <button
            type="button"
            className="qa-btn primary"
            disabled={busy}
            onClick={() => act(() => commands.commandCardConfirm(seq, true))}
          >
            {t("commandCard.confirm")}
          </button>
        </div>
      )}
      {kind === "question" && (
        <div className="approval-actions command-choices">
          <button
            type="button"
            className="qa-btn"
            disabled={busy}
            onClick={() => act(() => commands.commandCardDismiss(seq))}
          >
            {t("commandCard.cancel")}
          </button>
          {choices.map((choice) => (
            <button
              key={choice}
              type="button"
              className="qa-btn"
              disabled={busy}
              onClick={() => act(() => commands.commandCardAnswer(seq, choice))}
            >
              {choice}
            </button>
          ))}
        </div>
      )}
    </section>
  );
};
