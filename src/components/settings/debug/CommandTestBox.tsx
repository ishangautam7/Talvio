import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { Play, Square } from "@/components/icons/glyphs";
import { commands, type CommandReport } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { SettingContainer } from "@/components/ui/SettingContainer";
import {
  feedbackOptions,
  metaValues,
  prettyArgs,
  RUN_DELAY_SECONDS,
  secondsLeft,
  statusTone,
  waitsFor,
} from "./commandReport";

/**
 * The event a command run from outside this box reports on
 * (`signal_handle::COMMAND_REPORT_EVENT`): `--run-command`, and a spoken
 * command, so both show here too.
 */
const COMMAND_REPORT_EVENT = "command-mode-report";

const TONE_CLASSES = {
  ok: "bg-success/10 text-success",
  wait: "bg-warning/10 text-warning",
  bad: "bg-error/10 text-error",
} as const;

/** The last report: status, what was said, what ran, what it cost. */
const ReportView: React.FC<{
  report: CommandReport;
  busy: boolean;
  onConfirm: (accept: boolean) => void;
  onAnswer: (answer: string) => void;
}> = ({ report, busy, onConfirm, onAnswer }) => {
  const { t } = useTranslation();
  const [answer, setAnswer] = useState("");
  const waiting = waitsFor(report);

  // A new report starts with an empty answer field.
  useEffect(() => setAnswer(""), [report.id, report.status]);

  return (
    <div className="mt-2 space-y-3 rounded-lg border border-hairline bg-surface-strong p-3 text-[0.8125rem] text-ink">
      <div className="flex flex-wrap items-center gap-2">
        <span
          className={`rounded-md px-2 py-0.5 text-xs font-medium ${TONE_CLASSES[statusTone(report.status)]}`}
        >
          {t(`settings.debug.commandMode.status.${report.status}`)}
        </span>
        {report.dry_run && (
          <span className="rounded-md bg-ink/[0.06] px-2 py-0.5 text-xs text-muted">
            {t("settings.debug.commandMode.dryRunBadge")}
          </span>
        )}
        {report.origin === "cli" && (
          <span className="rounded-md bg-ink/[0.06] px-2 py-0.5 text-xs text-muted">
            {t("settings.debug.commandMode.fromCli")}
          </span>
        )}
        <span className="text-xs text-muted">
          {t("settings.debug.commandMode.meta", metaValues(report))}
        </span>
      </div>

      {report.transcript && (
        <p className="text-muted">
          {t("settings.debug.commandMode.transcript", {
            text: report.transcript,
          })}
        </p>
      )}

      {report.feedback.length > 0 && (
        <ul className="space-y-1">
          {report.feedback.map((feedback, index) => (
            <li key={index} className="font-medium">
              {t(feedback.key, feedbackOptions(feedback))}
            </li>
          ))}
        </ul>
      )}

      {waiting === "confirm" && (
        <div className="space-y-2">
          {report.pending_summary && (
            <p className="whitespace-pre-wrap text-muted">
              {report.pending_summary}
            </p>
          )}
          <div className="flex gap-2">
            <Button
              variant="primary"
              size="sm"
              disabled={busy}
              onClick={() => onConfirm(true)}
            >
              {t("settings.debug.commandMode.send")}
            </Button>
            <Button
              variant="secondary"
              size="sm"
              disabled={busy}
              onClick={() => onConfirm(false)}
            >
              {t("settings.debug.commandMode.cancel")}
            </Button>
          </div>
        </div>
      )}

      {waiting === "answer" && (
        <div className="space-y-2">
          {report.choices.length > 0 && (
            <div className="flex flex-wrap gap-2">
              {report.choices.map((choice) => (
                <Button
                  key={choice}
                  variant="secondary"
                  size="sm"
                  disabled={busy}
                  onClick={() => onAnswer(choice)}
                >
                  {choice}
                </Button>
              ))}
            </div>
          )}
          <div className="flex gap-2">
            <Input
              type="text"
              variant="compact"
              className="flex-1"
              value={answer}
              placeholder={t("settings.debug.commandMode.answer.placeholder")}
              onChange={(e) => setAnswer(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && answer.trim()) onAnswer(answer);
              }}
            />
            <Button
              variant="secondary"
              size="sm"
              disabled={busy || !answer.trim()}
              onClick={() => onAnswer(answer)}
            >
              {t("settings.debug.commandMode.answer.submit")}
            </Button>
          </div>
        </div>
      )}

      {report.escalation && (
        <p className="text-muted">
          {t("settings.debug.commandMode.escalation", {
            task: report.escalation,
          })}
        </p>
      )}

      {report.error && (
        <p className="text-error">
          {t("settings.debug.commandMode.error", { error: report.error })}
        </p>
      )}

      {report.steps.length > 0 && (
        <div className="space-y-1.5">
          <h4 className="text-xs font-medium text-muted">
            {t("settings.debug.commandMode.steps")}
          </h4>
          <ol className="space-y-1.5">
            {report.steps.map((step, index) => (
              <li
                key={index}
                className="rounded-md border border-hairline bg-surface p-2"
              >
                <div className="flex flex-wrap items-baseline gap-2">
                  <code className="font-medium">{step.tool}</code>
                  {step.result && (
                    <span className="text-xs text-muted">{step.result}</span>
                  )}
                </div>
                {step.detail && (
                  <p className="mt-1 whitespace-pre-wrap text-muted">
                    {step.detail}
                  </p>
                )}
                <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-words text-xs text-muted">
                  {prettyArgs(step.args_json)}
                </pre>
              </li>
            ))}
          </ol>
        </div>
      )}

      {report.dry_run_actions.length > 0 && (
        <div className="space-y-1.5">
          <h4 className="text-xs font-medium text-muted">
            {t("settings.debug.commandMode.dryRunActions")}
          </h4>
          <ul className="space-y-1">
            {report.dry_run_actions.map((action, index) => (
              <li key={index}>
                <code className="whitespace-pre-wrap break-all text-xs">
                  {action}
                </code>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
};

/**
 * Type a command and see exactly what the router chose and what each tool
 * did, with Send / Cancel and answers for a command that waits. On the
 * Commands page and in Settings → Debug; `--run-command` reports and spoken
 * commands land here too.
 */
export const CommandTestBox: React.FC<{
  title: string;
  description: string;
}> = ({ title, description }) => {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const savedDryRun = settings?.command_dry_run ?? false;
  const [text, setText] = useState("");
  const [dryRun, setDryRun] = useState(savedDryRun);
  const dryRunTouched = useRef(false);
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<CommandReport | null>(null);
  // "Run after N seconds": typing and key presses go to the window in front,
  // which is this one when Run is pressed, so the delay gives the user time
  // to click into the app the command is for. `countdown` is the seconds left
  // while one runs, `null` otherwise.
  const [delayed, setDelayed] = useState(false);
  const [countdown, setCountdown] = useState<number | null>(null);
  const countdownTimer = useRef<ReturnType<typeof setInterval> | null>(null);

  const stopCountdown = useCallback(() => {
    if (countdownTimer.current !== null) {
      clearInterval(countdownTimer.current);
      countdownTimer.current = null;
    }
    setCountdown(null);
  }, []);

  // A countdown must not fire a command after the page that started it is
  // gone (Settings closed, another tab chosen).
  useEffect(
    () => () => {
      if (countdownTimer.current !== null) {
        clearInterval(countdownTimer.current);
      }
    },
    [],
  );

  // Follow the saved setting until the box's own switch is used.
  useEffect(() => {
    if (!dryRunTouched.current) setDryRun(savedDryRun);
  }, [savedDryRun]);

  useEffect(() => {
    const unlisten = listen<CommandReport>(COMMAND_REPORT_EVENT, (event) =>
      setReport(event.payload),
    );
    return () => {
      void unlisten.then((off) => off());
    };
  }, []);

  const runWith = useCallback(
    async (action: () => Promise<CommandReport>) => {
      setBusy(true);
      try {
        setReport(await action());
      } catch (error) {
        toast.error(
          t("settings.debug.commandMode.invokeFailed", {
            error: String(error),
          }),
        );
      } finally {
        setBusy(false);
      }
    },
    [t],
  );

  const run = () => {
    const command = text.trim();
    if (!command || busy || countdown !== null) return;
    // The command and its dry-run choice are taken now, so editing the box
    // during the countdown does not change what runs.
    const dry = dryRun;
    if (!delayed) {
      void runWith(() => commands.commandRunText(command, dry));
      return;
    }
    const startedAt = Date.now();
    setCountdown(RUN_DELAY_SECONDS);
    countdownTimer.current = setInterval(() => {
      const left = secondsLeft(startedAt, Date.now());
      if (left > 0) {
        setCountdown(left);
        return;
      }
      stopCountdown();
      void runWith(() => commands.commandRunText(command, dry));
    }, 200);
  };

  const confirm = (accept: boolean) => {
    if (!report) return;
    const id = report.id;
    void runWith(() => commands.commandConfirm(id, accept));
  };

  const answer = (value: string) => {
    const trimmed = value.trim();
    if (!report || !trimmed) return;
    const id = report.id;
    void runWith(() => commands.commandAnswer(id, trimmed));
  };

  return (
    <SettingContainer
      title={title}
      description={description}
      layout="stacked"
      grouped={true}
      details={
        report && (
          <ReportView
            report={report}
            busy={busy}
            onConfirm={confirm}
            onAnswer={answer}
          />
        )
      }
    >
      <div className="flex w-full flex-wrap items-center gap-2">
        <Input
          type="text"
          variant="compact"
          className="min-w-[220px] flex-1"
          value={text}
          placeholder={t("settings.debug.commandMode.input.placeholder")}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") run();
          }}
        />
        <label className="inline-flex cursor-pointer items-center gap-1.5 text-[0.8125rem] text-ink">
          <input
            type="checkbox"
            className="h-3.5 w-3.5 accent-[var(--color-accent)]"
            checked={dryRun}
            onChange={(e) => {
              dryRunTouched.current = true;
              setDryRun(e.target.checked);
            }}
          />
          {t("settings.debug.commandMode.dryRun.label")}
        </label>
        <label className="inline-flex cursor-pointer items-center gap-1.5 text-[0.8125rem] text-ink">
          <input
            type="checkbox"
            className="h-3.5 w-3.5 accent-[var(--color-accent)]"
            checked={delayed}
            disabled={countdown !== null}
            onChange={(e) => setDelayed(e.target.checked)}
          />
          {t("settings.debug.commandMode.delay.label", {
            seconds: RUN_DELAY_SECONDS,
          })}
        </label>
        {busy && (
          <Button
            variant="secondary"
            size="sm"
            onClick={() => void commands.commandCancel()}
          >
            <Square className="h-3.5 w-3.5" aria-hidden="true" />
            {t("settings.debug.commandMode.stop")}
          </Button>
        )}
        {countdown !== null ? (
          <Button variant="secondary" size="sm" onClick={stopCountdown}>
            {t("settings.debug.commandMode.delay.cancel", {
              seconds: countdown,
            })}
          </Button>
        ) : (
          <Button
            variant="secondary"
            size="sm"
            disabled={busy || !text.trim()}
            onClick={run}
          >
            <Play className="h-3.5 w-3.5" aria-hidden="true" />
            {busy
              ? t("settings.debug.commandMode.running")
              : t("settings.debug.commandMode.run")}
          </Button>
        )}
        <p className="w-full text-xs text-muted" aria-live="polite">
          {countdown !== null
            ? t("settings.debug.commandMode.delay.countdown", {
                seconds: countdown,
              })
            : t("settings.debug.commandMode.delay.hint")}
        </p>
      </div>
    </SettingContainer>
  );
};
