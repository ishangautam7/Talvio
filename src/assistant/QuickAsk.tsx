import React from "react";
import { ArrowUp, Check, Copy, CornerDownLeft, Undo2 } from "lucide-react";
import { type Components } from "react-markdown";
import { useTranslation } from "react-i18next";
import { voiceEnergy } from "@/components/shared/waveformSignal";
import { LuminousOrb } from "./LuminousOrb";
import { orbModeFor, quickAskShape, type QuickAskPhase } from "./quickAskState";
import { copyAnswer } from "./richAnswer";

/**
 * The quick ask: a luminous orb that listens, thinks and answers aloud.
 *
 * There is no answer card. The orb swells with the user's voice while they
 * ask, turns slowly while the assistant works, and moves with the assistant's
 * own voice while the answer is spoken; then it settles and goes. The only
 * words it ever carries are one short line about a tool at work ("Searching
 * the web", "Open YouTube…") or a failure, and the only controls are small
 * chips for what an answer may still be for: Insert when the question was
 * about selected text, Undo for a reminder it just set, Copy when the answer
 * could not be heard.
 *
 * Typing is still possible (the tray opens the quick ask as a bar); a typed
 * question is answered by the same orb, aloud.
 *
 * Presentational: every action is a prop, so the settings preview, the tour
 * and the tests render exactly what the panel renders.
 */

/** The sphere's diameter. The element is twice this, for the halo. */
export const ORB_SIZE = 64;

export interface QuickAskProps {
  phase: QuickAskPhase;
  /** What the assistant is doing, in a few words: announced, never shown. */
  status: string;
  /** The one visible line: a tool at work, a failure, a heads-up. */
  caption?: string | null;
  /** Microphone levels while listening, when no `levelSource` is given. */
  levels?: number[];
  /** The live voice level for the orb (the user's, then the assistant's),
   *  read every frame. */
  levelSource?: () => number;
  /** The question as asked, without the selection it was about. */
  question: string;
  /** The answer, or as much of it as has streamed in. */
  answer: string;
  /** Kept for callers of the card this replaced; unused by the orb. */
  markdown?: Components;
  /** Characters of selected text this ask is about, 0 for none. */
  selectionChars: number;
  /** The screen was looked at for this ask. */
  screen: boolean;
  /** The reminder this ask set, by its time. The orb then offers Undo. */
  reminder?: { when: string } | null;
  /** Cancel the reminders this ask set. */
  onUndoReminder?: () => void;
  /** A failure to show in place of a caption. */
  error: string | null;
  /** A quiet heads-up about the answer, e.g. web search found nothing. */
  notice: string | null;
  /** Kept for callers of the card this replaced; the orb has no retry. */
  canRetry: boolean;
  /** The answer is complete (not still being written). */
  answerReady?: boolean;
  /** The answer was not heard (the voice failed, or there was nothing to say
   *  aloud), so it is offered as text to copy. */
  offerCopy?: boolean;
  /** The orb is on its way out: it shrinks into itself and fades. */
  leaving?: boolean;

  /** Typed input, for the prompt. */
  input: string;
  onInputChange: (value: string) => void;
  onSubmit: () => void;
  /** Focus the field when the prompt appears. */
  autoFocus?: boolean;
  /**
   * The field is about to be used. The panel does not take the keyboard when it
   * appears, so that a voice ask cannot steal focus from the app it is about;
   * this is where it asks for it.
   */
  onRequestKeyboard?: () => void;

  /** Put the surface away (nothing in flight). */
  onClose: () => void;
  /** Kept for callers of the card this replaced. */
  onNewConversation?: () => void;
  /** Stop whatever is in flight (the voice included) and put the orb away. */
  onCancel: () => void;
  /** Kept for callers of the card this replaced. */
  onStop: () => void;
  /** Kept for callers of the card this replaced. */
  onRetry: () => void;
  /** Write the answer where the user was. Resolves true on success. */
  onInsert: (text: string) => Promise<boolean>;
  /** Keeps a button press from starting a window drag. */
  stopDrag?: (event: React.MouseEvent) => void;
}

/** A brief tick after an action, then back to the resting icon. */
function useFlash(): [boolean, () => void] {
  const [on, setOn] = React.useState(false);
  const timer = React.useRef<number | null>(null);
  React.useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    [],
  );
  const flash = React.useCallback(() => {
    setOn(true);
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setOn(false), 1400);
  }, []);
  return [on, flash];
}

const QuickAsk: React.FC<QuickAskProps> = ({
  phase,
  status,
  caption = null,
  levels,
  levelSource,
  answer,
  selectionChars,
  reminder = null,
  onUndoReminder,
  error,
  notice,
  answerReady,
  offerCopy = false,
  leaving = false,
  input,
  onInputChange,
  onSubmit,
  autoFocus = false,
  onRequestKeyboard,
  onClose,
  onCancel,
  onInsert,
  stopDrag,
}) => {
  const { t } = useTranslation();
  const shape = quickAskShape(phase);
  const inputRef = React.useRef<HTMLInputElement>(null);
  const [copied, flashCopied] = useFlash();
  const [inserted, flashInserted] = useFlash();

  // Focused imperatively, and only on the way *into* the prompt, so a surface on
  // its way out cannot pull the caret back.
  React.useEffect(() => {
    if (phase === "prompt" && autoFocus) inputRef.current?.focus();
  }, [phase, autoFocus]);

  const inFlight =
    phase === "listening" ||
    phase === "transcribing" ||
    phase === "working" ||
    phase === "answering" ||
    phase === "speaking";
  // Without a live source (the settings preview, the tour), the levels a
  // caller passes still move the orb.
  const level =
    phase === "listening" || phase === "speaking"
      ? voiceEnergy(levels ?? [])
      : 0;

  if (shape === "bar") {
    return (
      <section
        className="qa-surface"
        data-shape="bar"
        data-phase={phase}
        aria-label={t("assistant.title")}
        data-tauri-drag-region
      >
        <div className="qa-row" data-tauri-drag-region>
          <input
            ref={inputRef}
            className="qa-input"
            type="text"
            value={input}
            placeholder={t("assistant.bar.placeholder")}
            aria-label={t("assistant.bar.placeholder")}
            onChange={(event) => onInputChange(event.target.value)}
            onPointerDown={onRequestKeyboard}
            onFocus={onRequestKeyboard}
            onMouseDown={stopDrag}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.repeat && !event.shiftKey)
                onSubmit();
            }}
          />
          <button
            type="button"
            className="qa-send"
            onClick={onSubmit}
            onMouseDown={stopDrag}
            disabled={!input.trim()}
            title={t("assistant.send")}
            aria-label={t("assistant.send")}
          >
            <ArrowUp size={15} strokeWidth={2.5} />
          </button>
        </div>
      </section>
    );
  }

  const settled = phase === "done" || phase === "speaking";
  const ready = answerReady ?? phase === "done";
  const canInsert =
    selectionChars > 0 && !reminder && ready && settled && !!answer.trim();
  const canCopy = offerCopy && !!answer.trim() && phase !== "speaking";
  const line =
    phase === "error" ? error : (caption ?? (settled ? notice : null));

  const insert = () => {
    void onInsert(answer).then((ok) => {
      if (ok) flashInserted();
    });
  };
  const copy = () => {
    void copyAnswer(answer).then(flashCopied, () => {});
  };

  const orbLabel = inFlight ? t("assistant.stop") : t("common.close");

  return (
    <section
      className="qa-surface"
      data-shape="orb"
      data-phase={phase}
      data-leaving={leaving ? "" : undefined}
      aria-label={t("assistant.title")}
      data-tauri-drag-region
    >
      <button
        type="button"
        className="qa-orb"
        onClick={inFlight ? onCancel : onClose}
        onMouseDown={stopDrag}
        title={orbLabel}
        aria-label={orbLabel}
      >
        <LuminousOrb
          mode={orbModeFor(phase)}
          size={ORB_SIZE}
          level={level}
          levelSource={levelSource}
        />
      </button>

      {(line || canInsert || reminder || canCopy) && (
        <div className="qa-orb-row">
          {line && (
            <span
              key={line}
              className="qa-caption"
              role={phase === "error" ? "alert" : undefined}
              title={line}
            >
              {line}
            </span>
          )}
          {canInsert && (
            <button
              type="button"
              className="qa-btn primary qa-chip-action"
              onClick={insert}
              onMouseDown={stopDrag}
              title={t("assistant.insertReplace")}
            >
              {inserted ? (
                <Check size={13} strokeWidth={2.4} aria-hidden="true" />
              ) : (
                <CornerDownLeft size={13} strokeWidth={2.2} aria-hidden="true" />
              )}
              <span>{t("assistant.insertReplaceShort")}</span>
            </button>
          )}
          {reminder && settled && (
            <button
              type="button"
              className="qa-btn qa-chip-action"
              onClick={onUndoReminder}
              onMouseDown={stopDrag}
              title={t("reminders.settings.cancel")}
            >
              <Undo2 size={13} strokeWidth={2.2} aria-hidden="true" />
              <span>{t("shortcutEditor.undo")}</span>
            </button>
          )}
          {canCopy && (
            <button
              type="button"
              className="qa-btn qa-chip-action"
              onClick={copy}
              onMouseDown={stopDrag}
              title={t("assistant.copy")}
            >
              {copied ? (
                <Check size={13} strokeWidth={2.4} aria-hidden="true" />
              ) : (
                <Copy size={13} strokeWidth={2.2} aria-hidden="true" />
              )}
              <span>
                {copied ? t("assistant.quick.copied") : t("assistant.copy")}
              </span>
            </button>
          )}
        </div>
      )}

      {/* What the orb shows without words, for a screen reader. */}
      <span className="qa-sr-only" role="status" aria-live="polite">
        {phase === "error" ? "" : status}
      </span>
    </section>
  );
};

export default QuickAsk;
