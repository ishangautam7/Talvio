import type { OrbMode } from "./orbMotion";

/**
 * What the quick ask is showing, derived from the assistant's pipeline state.
 *
 * Pure, so the decisions that make the orb feel alive or stuck are tests
 * rather than screenshots.
 *
 * The quick ask is voice-first: one question, answered aloud by the orb. There
 * is no answer card. The next ask may continue the same conversation while it
 * is remembered ("Remember the conversation"), but every phase below belongs
 * to the single exchange in front of the user.
 */

export type AssistantState =
  | "idle"
  | "listening"
  | "transcribing"
  | "searching"
  | "thinking"
  | "speaking";

export type QuickAskPhase =
  /** Opened from the tray with nothing in flight: a field to type into. */
  | "prompt"
  /** The microphone is open. */
  | "listening"
  /** The recording is being turned into text. */
  | "transcribing"
  /** The model is working and has not written anything yet. */
  | "working"
  /** The answer is being written, and its voice has not started yet. */
  | "answering"
  /** The answer is being spoken. */
  | "speaking"
  /** The answer is complete and nothing is playing. */
  | "done"
  /** The ask failed. */
  | "error";

/**
 * The surface's two shapes: the bar is for typing, the orb is for everything
 * else. They are one element, never a swap of windows.
 */
export type QuickAskShape = "bar" | "orb";

export interface QuickAskInputs {
  state: AssistantState;
  /** The answer as it streams in, before the backend's final snapshot. */
  stream: string;
  /** The finished answer to the current question, if there is one. */
  answer: string;
  /** An error is on screen. */
  hasError: boolean;
  /** The answer's voice is audible right now. */
  audible?: boolean;
}

export function quickAskPhase({
  state,
  stream,
  answer,
  hasError,
  audible = false,
}: QuickAskInputs): QuickAskPhase {
  if (state === "listening") return "listening";
  if (state === "transcribing") return "transcribing";
  // The voice outranks everything after the question: while it is heard, the
  // orb is speaking, even if the backend has already moved on to idle.
  if (audible) return "speaking";
  const busy = state !== "idle";
  if (hasError && !busy) return "error";
  // "speaking" from the backend means a voice is on its way, not yet heard.
  if (state === "speaking") return "answering";
  if (stream.trim() || answer.trim()) return busy ? "answering" : "done";
  return busy ? "working" : "prompt";
}

export function quickAskShape(phase: QuickAskPhase): QuickAskShape {
  return phase === "prompt" ? "bar" : "orb";
}

/** What the orb does in each phase. */
export function orbModeFor(phase: QuickAskPhase): OrbMode {
  switch (phase) {
    case "listening":
      return "listening";
    case "transcribing":
    case "working":
    case "answering":
      return "thinking";
    case "speaking":
      return "speaking";
    case "error":
      return "error";
    default:
      return "idle";
  }
}

/** How long the orb stays once a spoken answer has finished, before it puts
 *  itself away: long enough to read as an ending, not a disappearance. */
export const SETTLE_MS = 900;

/** How long it stays when it is offering something to click (Insert, Undo,
 *  Copy), unless the pointer is on it. */
export const LINGER_WITH_ACTION_MS = 8000;

/** How long a finished orb stays before it puts itself away. */
export function lingerFor(offersAction: boolean): number {
  return offersAction ? LINGER_WITH_ACTION_MS : SETTLE_MS;
}

/** A tool the current turn is running, as reported by the backend. */
export interface ToolActivity {
  /** Backend tool id, e.g. `web_search`. */
  name: string;
  /** The one argument worth showing — currently only the search query. */
  detail: string;
}

/**
 * The translation key for what the assistant is doing while the pill waits.
 *
 * Kept to a few short states — listening, thinking, searching the web, looking
 * at the screen, doing something on the computer — because the pill is
 * glanced at, not read. Transcription used
 * to have its own label, and every other tool a generic "Working on it"; to the
 * person waiting both are just the assistant thinking, and a pill that flips
 * through five labels in two seconds reads as noise. A tool that changes what
 * the user should expect (the web, their screen) still gets named, and beats
 * the pipeline state because it is the more specific answer.
 */
export function workingLabelKey(
  phase: QuickAskPhase,
  state: AssistantState,
  tool: ToolActivity | null,
): string {
  if (phase === "listening") return "assistant.status.listening";
  if (tool?.name === "web_search") return "assistant.status.searching";
  if (tool?.name === "capture_screen") return "assistant.tool.screen";
  if (tool?.name === "do_on_computer") return "assistant.tool.computer";
  if (!tool && state === "searching") return "assistant.status.searching";
  return "assistant.status.thinking";
}

/**
 * The one line of text the orb may carry while a tool runs: what it is doing,
 * in a few words ("Searching the web", "Open YouTube…"). Only tools whose
 * work the user would want to see named get a line; the rest (setting a
 * reminder, reading the clock) are over before a line could be read.
 */
export function toolCaption(
  tool: ToolActivity | null,
): { key: string; values?: Record<string, string> } | null {
  if (!tool) return null;
  switch (tool.name) {
    case "web_search":
      return { key: "assistant.status.searching" };
    case "capture_screen":
      return { key: "assistant.tool.screen" };
    case "search_meeting":
      return { key: "assistant.tool.meetingSearch" };
    case "read_meeting":
      return { key: "assistant.tool.meetingRead" };
    case "do_on_computer": {
      const action = oneLine(tool.detail);
      return action
        ? { key: "assistant.orb.doing", values: { action } }
        : { key: "assistant.tool.computer" };
    }
    default:
      return null;
  }
}

/** A request as one short line: its first line, capitalised, trailing
 *  punctuation dropped (the caption adds its own ellipsis), and cut at a word
 *  near 48 characters. */
export function oneLine(text: string): string {
  const first = (text.split("\n")[0] ?? "").trim().replace(/[.!?…:;,]+$/u, "");
  if (!first) return "";
  const capped = first.charAt(0).toLocaleUpperCase() + first.slice(1);
  if (capped.length <= 48) return capped;
  const cut = capped.slice(0, 48);
  const space = cut.lastIndexOf(" ");
  return (space > 24 ? cut.slice(0, space) : cut).trimEnd();
}

/**
 * Where the surface sits inside its fixed frame, as sent by Rust
 * (`assistant-ask-layout`). The frame is placed at the dock zone the user chose;
 * this pins the surface to the edge of the frame that is flush with the screen
 * edge, so the pill lands exactly where it was docked and the card grows away
 * from that edge.
 */
export interface QuickAskLayout {
  align: "top" | "bottom";
  justify: "start" | "center" | "end";
}

export const DEFAULT_LAYOUT: QuickAskLayout = {
  align: "top",
  justify: "center",
};

/** Accept only a well-formed layout, falling back to the default. */
export function parseLayout(payload: unknown): QuickAskLayout {
  if (!payload || typeof payload !== "object") return DEFAULT_LAYOUT;
  const { align, justify } = payload as Record<string, unknown>;
  return {
    align: align === "bottom" ? "bottom" : "top",
    justify: justify === "start" || justify === "end" ? justify : "center",
  };
}
