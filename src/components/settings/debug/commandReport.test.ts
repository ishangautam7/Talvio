import { describe, expect, test } from "bun:test";
import type { CommandReport } from "@/bindings";
import {
  feedbackOptions,
  feedbackParams,
  metaValues,
  prettyArgs,
  RUN_DELAY_SECONDS,
  secondsLeft,
  statusTone,
  waitsFor,
} from "./commandReport";

function report(overrides: Partial<CommandReport> = {}): CommandReport {
  return {
    id: 1,
    origin: "typed",
    transcript: "send an email to Ron",
    status: "done",
    feedback: [],
    steps: [],
    pending_summary: null,
    question: null,
    choices: [],
    escalation: null,
    dry_run: false,
    dry_run_actions: [],
    model: "anthropic/claude-haiku-5-5",
    llm_calls: 1,
    usage: {
      input_tokens: 120,
      output_tokens: 40,
      cache_read_tokens: 900,
      cache_write_tokens: 0,
    },
    latency_ms: 812,
    error: null,
    ...overrides,
  };
}

describe("feedback", () => {
  const feedback = {
    key: "commandMode.feedback.emailDraft",
    params: [["name", "Ron Weasley"]] as [string, string][],
    text: "Draft to Ron Weasley is open. Say send or cancel.",
  };

  // i18next interpolates an object, the backend sends pairs.
  test("parameters become an object", () => {
    expect(feedbackParams(feedback)).toEqual({ name: "Ron Weasley" });
  });

  // A key no translation has yet must still read as words.
  test("the English text is the fallback", () => {
    expect(feedbackOptions(feedback)).toEqual({
      name: "Ron Weasley",
      defaultValue: "Draft to Ron Weasley is open. Say send or cancel.",
    });
  });
});

describe("prettyArgs", () => {
  test("JSON is indented", () => {
    expect(prettyArgs('{"to":"Ron"}')).toBe('{\n  "to": "Ron"\n}');
  });

  // The executor only promises text; anything else is shown as it came.
  test("text that is not JSON is shown as it is", () => {
    expect(prettyArgs("not json")).toBe("not json");
  });
});

describe("status", () => {
  // Only these two statuses show Send/Cancel or an answer field.
  test("a report waits for confirmation or an answer, or for nothing", () => {
    expect(waitsFor(report({ status: "awaiting_confirmation" }))).toBe(
      "confirm",
    );
    expect(waitsFor(report({ status: "needs_answer" }))).toBe("answer");
    expect(waitsFor(report({ status: "done" }))).toBeNull();
    expect(waitsFor(report({ status: "failed" }))).toBeNull();
  });

  test("the badge tone follows the status", () => {
    expect(statusTone("done")).toBe("ok");
    expect(statusTone("needs_answer")).toBe("wait");
    expect(statusTone("awaiting_confirmation")).toBe("wait");
    expect(statusTone("needs_computer_use")).toBe("bad");
    expect(statusTone("cancelled")).toBe("bad");
    expect(statusTone("failed")).toBe("bad");
  });
});

describe("metaValues", () => {
  // Tokens are shown as counted: cached tokens are their own figure.
  test("the cost line carries the model, calls, tokens and time", () => {
    expect(metaValues(report())).toEqual({
      model: "anthropic/claude-haiku-5-5",
      calls: "1",
      input: "120",
      output: "40",
      cached: "900",
      ms: "812",
    });
  });

  // A command that failed before any model was chosen has no model name.
  test("a missing model reads as a dash", () => {
    expect(metaValues(report({ model: "" })).model).toBe("-");
  });
});

describe("secondsLeft", () => {
  // The countdown reads 5 the moment it starts, so the user sees the whole
  // wait they were promised.
  test("a fresh countdown shows the whole delay", () => {
    expect(secondsLeft(1000, 1000)).toBe(RUN_DELAY_SECONDS);
  });

  // Rounding up keeps "1" on screen for the last second instead of showing
  // "0" while the command has not run yet.
  test("the last second still reads 1", () => {
    expect(secondsLeft(0, 4001, 5)).toBe(1);
    expect(secondsLeft(0, 4999, 5)).toBe(1);
  });

  // The command runs at 0, and a timer that fires late never goes negative.
  test("time up reads 0, even when the timer is late", () => {
    expect(secondsLeft(0, 5000, 5)).toBe(0);
    expect(secondsLeft(0, 9000, 5)).toBe(0);
  });
});
