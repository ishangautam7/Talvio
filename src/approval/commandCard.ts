import type { CommandReport } from "@/bindings";

/**
 * Pure helpers for a spoken command's card in the corner
 * (`src-tauri/src/command_mode/voice.rs`), kept out of the component so they
 * can be tested without React or Tauri.
 */

/** What a card is: a report, or a command that waits on the user. */
export type CommandCardKind = "done" | "problem" | "confirm" | "question";

export function commandCardKind(report: CommandReport): CommandCardKind {
  switch (report.status) {
    case "done":
      return "done";
    case "awaiting_confirmation":
      return "confirm";
    case "needs_answer":
      return "question";
    default:
      // Failed, cancelled, or nothing fits and the agent cannot take over:
      // all of them only say why nothing (more) happened.
      return "problem";
  }
}

/** How long a card that only reports stays once it has been read. */
export const AUTO_CLOSE_MS = 6000;

/** How long it stays after the pointer leaves it. */
export const AFTER_HOVER_MS = 2500;

/**
 * Whether the card closes by itself. A card that waits stays until it is
 * answered or what it waits for expires (the backend takes it down then).
 */
export function closesItself(report: CommandReport): boolean {
  const kind = commandCardKind(report);
  return kind === "done" || kind === "problem";
}

/** The card's heading: what was heard, or a word for the kind of card when
 *  nothing was. */
export function cardHeading(
  report: CommandReport,
  t: (key: string) => string,
): string {
  const heard = report.transcript.trim();
  return heard ? heard : t("commandCard.heading");
}

/** The answers a question card offers as buttons: its choices, once each. */
export function answerChoices(report: CommandReport): string[] {
  if (commandCardKind(report) !== "question") return [];
  const seen = new Set<string>();
  return report.choices
    .map((choice) => choice.trim())
    .filter((choice) => {
      if (!choice || seen.has(choice)) return false;
      seen.add(choice);
      return true;
    });
}
