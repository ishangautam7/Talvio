import type { CommandReport, CommandStatus, Feedback } from "@/bindings";

/**
 * Pure helpers for showing a command-mode report (`CommandModeTester`), kept
 * out of the component so they can be tested without React or Tauri.
 */

/** A feedback's parameters as the object i18next interpolates. */
export function feedbackParams(feedback: Feedback): Record<string, string> {
  const params: Record<string, string> = {};
  for (const [name, value] of feedback.params) {
    params[name] = value;
  }
  return params;
}

/**
 * The i18next options that render one feedback: its parameters, and the
 * English text the backend already rendered as the fallback, so a key no
 * translation has yet (a tool added since) still reads as words instead of
 * as `commandMode.feedback.something`.
 */
export function feedbackOptions(
  feedback: Feedback,
): Record<string, string> & { defaultValue: string } {
  return { ...feedbackParams(feedback), defaultValue: feedback.text };
}

/** The arguments of a step, pretty-printed when they are JSON. */
export function prettyArgs(argsJson: string): string {
  try {
    return JSON.stringify(JSON.parse(argsJson), null, 2);
  } catch {
    return argsJson;
  }
}

/** Whether the report waits on the user, and for what. */
export function waitsFor(report: CommandReport): "confirm" | "answer" | null {
  switch (report.status) {
    case "awaiting_confirmation":
      return "confirm";
    case "needs_answer":
      return "answer";
    default:
      return null;
  }
}

/** How the status badge reads: good, waiting, or bad. */
export function statusTone(status: CommandStatus): "ok" | "wait" | "bad" {
  switch (status) {
    case "done":
      return "ok";
    case "awaiting_confirmation":
    case "needs_answer":
      return "wait";
    default:
      return "bad";
  }
}

/**
 * The cost line under a report's status, as interpolation values for
 * `settings.debug.commandMode.meta`: the model, how many calls, the tokens
 * in, out and read from the cache, and how long the command took. The
 * tokens are shown as the provider counted them, not summed: Anthropic's
 * input count leaves cached tokens out and the OpenAI schema's includes
 * them, so a total would be wrong for one of the two.
 */
export function metaValues(report: CommandReport): Record<string, string> {
  return {
    model: report.model || "-",
    calls: String(report.llm_calls),
    input: String(report.usage.input_tokens),
    output: String(report.usage.output_tokens),
    cached: String(report.usage.cache_read_tokens),
    ms: String(report.latency_ms),
  };
}

/**
 * How long "Run after N seconds" waits before a typed command runs. The
 * Debug box is in Zynveliq's own window, and typing and key presses go to
 * whatever window is in front when the tool runs, so the wait is the time the
 * user has to click into the app the command is for.
 */
export const RUN_DELAY_SECONDS = 5;

/**
 * Whole seconds left in a countdown that started at `startedAtMs`, rounded
 * up so it reads 5, 4, 3, 2, 1 and only reaches 0 when the time is up, and
 * never below 0 however late the timer fires.
 */
export function secondsLeft(
  startedAtMs: number,
  nowMs: number,
  delaySeconds: number = RUN_DELAY_SECONDS,
): number {
  const remainingMs = startedAtMs + delaySeconds * 1000 - nowMs;
  return remainingMs <= 0 ? 0 : Math.ceil(remainingMs / 1000);
}
