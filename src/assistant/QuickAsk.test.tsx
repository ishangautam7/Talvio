import { afterEach, expect, mock, test } from "bun:test";
import React from "react";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import {
  lingerFor,
  oneLine,
  orbModeFor,
  parseLayout,
  quickAskPhase,
  quickAskShape,
  toolCaption,
  workingLabelKey,
  LINGER_WITH_ACTION_MS,
  SETTLE_MS,
} from "./quickAskState";

/**
 * The quick ask: one question, answered aloud by the orb.
 *
 * The state derivation decides whether the orb feels alive or stuck, and the
 * component is presentational, so both are checked directly — no window, no
 * backend, no pipeline.
 */

mock.module("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const { default: QuickAsk } = await import("./QuickAsk");
const { HIT_SURFACE_SELECTORS } = await import("./hitRegion");

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const idle = {
  state: "idle",
  stream: "",
  answer: "",
  hasError: false,
} as const;

test("typing is a bar, everything else is the orb", () => {
  expect(quickAskShape("prompt")).toBe("bar");
  for (const phase of [
    "listening",
    "transcribing",
    "working",
    "answering",
    "speaking",
    "done",
    "error",
  ] as const)
    expect(quickAskShape(phase)).toBe("orb");
});

test("the pipeline maps onto listening, thinking and speaking", () => {
  expect(quickAskPhase({ ...idle, state: "listening" })).toBe("listening");
  expect(quickAskPhase({ ...idle, state: "transcribing" })).toBe(
    "transcribing",
  );
  expect(quickAskPhase({ ...idle, state: "searching" })).toBe("working");
  expect(quickAskPhase({ ...idle, state: "thinking", stream: "The" })).toBe(
    "answering",
  );
  // The backend's "speaking" is a voice on its way, not yet heard.
  expect(quickAskPhase({ ...idle, state: "speaking", answer: "Hi." })).toBe(
    "answering",
  );
  expect(
    quickAskPhase({ ...idle, state: "speaking", answer: "Hi.", audible: true }),
  ).toBe("speaking");
  expect(quickAskPhase({ ...idle, answer: "Done." })).toBe("done");
  expect(quickAskPhase(idle)).toBe("prompt");
});

test("the voice outranks the backend going idle", () => {
  // The turn ends long before its voice does.
  expect(quickAskPhase({ ...idle, answer: "Hi.", audible: true })).toBe(
    "speaking",
  );
});

test("a new question being spoken outranks the previous answer", () => {
  expect(
    quickAskPhase({ ...idle, state: "listening", answer: "Old answer" }),
  ).toBe("listening");
});

test("an error shows once the pipeline has stopped", () => {
  expect(quickAskPhase({ ...idle, hasError: true })).toBe("error");
  expect(quickAskPhase({ ...idle, state: "thinking", hasError: true })).toBe(
    "working",
  );
});

test("the orb listens, thinks and speaks", () => {
  expect(orbModeFor("listening")).toBe("listening");
  expect(orbModeFor("transcribing")).toBe("thinking");
  expect(orbModeFor("working")).toBe("thinking");
  expect(orbModeFor("answering")).toBe("thinking");
  expect(orbModeFor("speaking")).toBe("speaking");
  expect(orbModeFor("done")).toBe("idle");
  expect(orbModeFor("error")).toBe("error");
});

test("a finished orb goes quickly, unless it offers something to click", () => {
  expect(lingerFor(false)).toBe(SETTLE_MS);
  expect(lingerFor(true)).toBe(LINGER_WITH_ACTION_MS);
  expect(SETTLE_MS).toBeLessThan(1500);
});

test("only tools worth naming get a line", () => {
  expect(toolCaption(null)).toBeNull();
  expect(toolCaption({ name: "web_search", detail: "rust" })).toEqual({
    key: "assistant.status.searching",
  });
  expect(toolCaption({ name: "capture_screen", detail: "" })).toEqual({
    key: "assistant.tool.screen",
  });
  expect(
    toolCaption({ name: "do_on_computer", detail: "open youtube." }),
  ).toEqual({ key: "assistant.orb.doing", values: { action: "Open youtube" } });
  expect(toolCaption({ name: "do_on_computer", detail: "" })).toEqual({
    key: "assistant.tool.computer",
  });
  // Over before a line could be read.
  expect(toolCaption({ name: "set_reminder", detail: "" })).toBeNull();
  expect(toolCaption({ name: "get_current_datetime", detail: "" })).toBeNull();
});

test("a request becomes one short line", () => {
  expect(oneLine("open the settings app\nand then wifi")).toBe(
    "Open the settings app",
  );
  const long = oneLine(
    "write an email to the whole team about the new release schedule for next quarter",
  );
  expect(long.length).toBeLessThanOrEqual(48);
  expect(long.endsWith(" ")).toBe(false);
  expect(oneLine("  ")).toBe("");
});

test("the screen-reader status is one of a few short states", () => {
  expect(workingLabelKey("listening", "listening", null)).toBe(
    "assistant.status.listening",
  );
  expect(workingLabelKey("working", "searching", null)).toBe(
    "assistant.status.searching",
  );
  expect(workingLabelKey("transcribing", "transcribing", null)).toBe(
    "assistant.status.thinking",
  );
});

test("a malformed layout falls back to the top centre", () => {
  expect(parseLayout({ align: "bottom", justify: "end" })).toEqual({
    align: "bottom",
    justify: "end",
  });
  expect(parseLayout(null)).toEqual({ align: "top", justify: "center" });
});

// ---------------------------------------------------------------------------
// The surface
// ---------------------------------------------------------------------------

let renderer: ReactTestRenderer | null = null;
const noop = () => {};

const base = {
  status: "assistant.status.thinking",
  question: "",
  answer: "",
  selectionChars: 0,
  screen: false,
  error: null,
  notice: null,
  canRetry: false,
  input: "",
  onInputChange: noop,
  onSubmit: noop,
  onClose: noop,
  onCancel: noop,
  onStop: noop,
  onRetry: noop,
  onInsert: async () => true,
};

const render = (node: React.ReactElement) => {
  act(() => {
    renderer = create(node);
  });
  return renderer!;
};

afterEach(() => {
  if (renderer) act(() => renderer!.unmount());
  renderer = null;
});

const labels = () =>
  renderer!.root
    .findAllByType("button")
    .map((button) => button.props["aria-label"] ?? button.props.title);

const surface = () =>
  renderer!.root.find(
    (node) =>
      typeof node.type === "string" && node.props.className === "qa-surface",
  );

const visibleText = () =>
  renderer!.root
    .findAll(
      (node) =>
        typeof node.type === "string" && node.props.className !== "qa-sr-only",
    )
    .flatMap((node) => node.children)
    .filter((child): child is string => typeof child === "string");

test("an answer is never drawn as text", () => {
  render(
    <QuickAsk
      {...base}
      phase="speaking"
      question="What is the capital of France?"
      answer="Paris is the capital of France."
    />,
  );
  expect(surface().props["data-shape"]).toBe("orb");
  expect(renderer!.root.findAllByType("input")).toHaveLength(0);
  expect(visibleText().join(" ")).not.toContain("Paris");
  expect(visibleText().join(" ")).not.toContain("capital");
});

test("the orb is the only control while it works, and it stops", () => {
  for (const phase of [
    "listening",
    "transcribing",
    "working",
    "answering",
    "speaking",
  ] as const) {
    render(<QuickAsk {...base} phase={phase} />);
    expect(labels()).toEqual(["assistant.stop"]);
    act(() => renderer!.unmount());
    renderer = null;
  }
});

test("a tool at work is the one line of words", () => {
  render(
    <QuickAsk
      {...base}
      phase="working"
      caption="Open YouTube in the browser…"
    />,
  );
  expect(visibleText()).toContain("Open YouTube in the browser…");
});

test("an answer about a selection offers Insert once it is ready", () => {
  const asked = {
    ...base,
    selectionChars: 40,
    question: "Make this formal",
    answer: "Dear team,",
  };
  render(<QuickAsk {...asked} phase="speaking" answerReady={false} />);
  expect(labels()).toEqual(["assistant.stop"]);
  act(() => renderer!.unmount());
  render(<QuickAsk {...asked} phase="speaking" answerReady />);
  expect(labels()).toEqual(["assistant.stop", "assistant.insertReplace"]);
  act(() => renderer!.unmount());
  render(<QuickAsk {...asked} phase="done" answerReady />);
  expect(labels()).toEqual(["common.close", "assistant.insertReplace"]);
});

test("an answer that was not about a selection offers nothing to click", () => {
  render(
    <QuickAsk
      {...base}
      phase="done"
      answer="Paris."
      answerReady
      question="q"
    />,
  );
  expect(labels()).toEqual(["common.close"]);
});

test("an answer nobody heard is offered to copy", () => {
  render(
    <QuickAsk
      {...base}
      phase="done"
      answer="```rm -rf```"
      answerReady
      offerCopy
    />,
  );
  expect(labels()).toEqual(["common.close", "assistant.copy"]);
});

test("an ask that set a reminder offers Undo, not Insert", () => {
  render(
    <QuickAsk
      {...base}
      phase="done"
      answer="Done."
      answerReady
      selectionChars={20}
      reminder={{ when: "2:30 PM" }}
    />,
  );
  expect(labels()).toEqual(["common.close", "reminders.settings.cancel"]);
});

test("an error is announced in the one line", () => {
  render(<QuickAsk {...base} phase="error" error="The provider refused" />);
  const alert = renderer!.root.find(
    (node) => typeof node.type === "string" && node.props.role === "alert",
  );
  expect(alert.children).toContain("The provider refused");
  expect(labels()).toEqual(["common.close"]);
});

test("the prompt is a field with send, and send is refused while empty", () => {
  render(<QuickAsk {...base} phase="prompt" />);
  expect(renderer!.root.findAllByType("input")).toHaveLength(1);
  const send = renderer!.root
    .findAllByType("button")
    .find((button) => button.props["aria-label"] === "assistant.send");
  expect(send?.props.disabled).toBe(true);
});

/**
 * The measured-surface list must contain the surface this component draws.
 * `hitRegion.ts` hands everything outside the listed surfaces to whatever is
 * behind the window, so a rename on either side must fail here rather than on a
 * user's desktop.
 */
test("the surface is one hitRegion measures", () => {
  render(<QuickAsk {...base} phase="listening" />);
  const listed = HIT_SURFACE_SELECTORS.filter((s) => s.startsWith(".")).map(
    (s) => s.slice(1),
  );
  expect(listed).toContain(surface().props.className);
});
