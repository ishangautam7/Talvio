import { describe, expect, test } from "bun:test";
import type { CommandReport, CommandStatus } from "@/bindings";
import {
  answerChoices,
  cardHeading,
  closesItself,
  commandCardKind,
} from "./commandCard";

const report = (
  status: CommandStatus,
  extra: Partial<CommandReport> = {},
): CommandReport => ({
  id: 1,
  origin: "voice",
  transcript: "open YouTube",
  status,
  feedback: [],
  steps: [],
  pending_summary: null,
  question: null,
  choices: [],
  escalation: null,
  dry_run: false,
  dry_run_actions: [],
  model: "",
  llm_calls: 1,
  usage: {
    input_tokens: 0,
    output_tokens: 0,
    cache_read_tokens: 0,
    cache_write_tokens: 0,
  },
  latency_ms: 0,
  error: null,
  ...extra,
});

describe("commandCardKind", () => {
  test("a report, a confirmation or a question", () => {
    expect(commandCardKind(report("done"))).toBe("done");
    expect(commandCardKind(report("awaiting_confirmation"))).toBe("confirm");
    expect(commandCardKind(report("needs_answer"))).toBe("question");
    for (const status of [
      "failed",
      "cancelled",
      "needs_computer_use",
    ] as const) {
      expect(commandCardKind(report(status))).toBe("problem");
    }
  });
});

describe("closesItself", () => {
  test("only a card that waits on nobody closes by itself", () => {
    expect(closesItself(report("done"))).toBe(true);
    expect(closesItself(report("failed"))).toBe(true);
    expect(closesItself(report("awaiting_confirmation"))).toBe(false);
    expect(closesItself(report("needs_answer"))).toBe(false);
  });
});

describe("cardHeading", () => {
  const t = (key: string) => key;
  test("what was heard, or a word for the card when nothing was", () => {
    expect(cardHeading(report("done"), t)).toBe("open YouTube");
    expect(cardHeading(report("failed", { transcript: "  " }), t)).toBe(
      "commandCard.heading",
    );
  });
});

describe("answerChoices", () => {
  test("a question's choices, trimmed and once each", () => {
    const question = report("needs_answer", {
      question: "Which Ron?",
      choices: [" Ron Weasley ", "Ron Swanson", "Ron Weasley", ""],
    });
    expect(answerChoices(question)).toEqual(["Ron Weasley", "Ron Swanson"]);
  });

  test("no buttons on a card that asks nothing", () => {
    expect(answerChoices(report("done", { choices: ["leftover"] }))).toEqual(
      [],
    );
  });
});
