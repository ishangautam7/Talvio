// Must be first: installs the fake Tauri backend before any module touches it.
import { previewParams } from "./mockBackend";

/*
 * Browser preview of the small floating windows. Dev-only, like `main.tsx`:
 * not a Vite build input, never shipped. Each window is drawn at about its
 * real size over a desktop-ish backdrop, so its transparent edges can be
 * judged.
 *
 * URL knobs: ?w=ask|ask-listening|ask-working|ask-tool|ask-speaking|
 *               ask-done|ask-insert|ask-error|ask-bottom|overlay|
 *               overlay-live|overlay-working|reminder|approval
 *            &bg=light|dark|photo
 */
import React, { useEffect } from "react";
import ReactDOM from "react-dom/client";
import { emit } from "@tauri-apps/api/event";
import "@fontsource-variable/inter";
import "../i18n";
import QuickAsk from "@/assistant/QuickAsk";
import { LuminousOrb } from "@/assistant/LuminousOrb";
import type { OrbMode } from "@/assistant/orbMotion";
import "@/assistant/AssistantPanel.css";
import RecordingOverlay from "@/overlay/RecordingOverlay";
import ReminderPopup from "@/reminder/ReminderPopup";
import ApprovalPopup from "@/approval/ApprovalPopup";

const BACKDROPS: Record<string, string> = {
  light: "linear-gradient(135deg, #eef0f4 0%, #dfe3ea 100%)",
  dark: "linear-gradient(135deg, #1f2937 0%, #111827 100%)",
  photo:
    "radial-gradient(circle at 20% 30%, #5b8def 0%, transparent 45%), radial-gradient(circle at 80% 70%, #f2a65a 0%, transparent 40%), #2c3e50",
};

const noop = () => {};
const ANSWER = `**git branch -m** renames the branch you are on:

1. \`git branch -m new-name\` renames it locally.
2. \`git push origin -u new-name\` publishes the new name.
3. \`git push origin --delete old-name\` removes the old one.

Anyone else on the branch needs to fetch and reset their upstream.`;

/** A voice that rises and falls like speech: syllables on a slower phrase. */
const speechLike = () => {
  const t = performance.now() / 1000;
  const syllable = Math.max(
    0,
    Math.sin(t * 11) * 0.6 + Math.sin(t * 7.3) * 0.4,
  );
  const phrase = 0.55 + 0.45 * Math.sin(t * 1.3);
  return Math.min(1, syllable * phrase * 1.15);
};

type AskPhase =
  | "prompt"
  | "listening"
  | "working"
  | "speaking"
  | "done"
  | "error";

const Ask: React.FC<{
  phase: AskPhase;
  caption?: string | null;
  selection?: boolean;
  align?: "top" | "bottom";
}> = ({ phase, caption = null, selection = false, align = "top" }) => (
  <div
    className="assistant-scope"
    style={
      {
        position: "absolute",
        inset: 0,
        "--as-msg-font": "15px",
        "--as-alpha": "1",
      } as React.CSSProperties
    }
  >
    <div className="qa-frame" data-align={align} data-justify="center">
      <QuickAsk
        phase={phase}
        status={phase === "listening" ? "Listening" : "Thinking"}
        caption={caption}
        levelSource={
          phase === "listening" || phase === "speaking" ? speechLike : undefined
        }
        question="What's the fastest way to rename a git branch?"
        answer={phase === "done" || phase === "speaking" ? ANSWER : ""}
        answerReady
        selectionChars={selection ? 120 : 0}
        screen={false}
        error={phase === "error" ? "Couldn't reach the assistant" : null}
        notice={null}
        canRetry={phase === "done"}
        input=""
        onInputChange={noop}
        onSubmit={noop}
        onClose={noop}
        onCancel={noop}
        onStop={noop}
        onRetry={noop}
        onInsert={async () => false}
      />
    </div>
  </div>
);

/** Every mode of the orb side by side, large, for judging the shader. The
 *  `size` knob sets the sphere's diameter. */
const OrbLab: React.FC = () => {
  const size = Number(previewParams.get("size") ?? 120);
  const modes: OrbMode[] = [
    "idle",
    "listening",
    "thinking",
    "speaking",
    "muted",
    "error",
  ];
  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        display: "flex",
        flexWrap: "wrap",
        alignItems: "center",
        justifyContent: "center",
        gap: 0,
      }}
    >
      {modes.map((mode) => (
        <LuminousOrb
          key={mode}
          mode={mode}
          size={size}
          levelSource={
            mode === "listening" || mode === "speaking" ? speechLike : undefined
          }
        />
      ))}
    </div>
  );
};

/** The overlay window's own sizes (overlay.rs): the oval, the live card, the
 *  labelled pill and the recovery pill. */
const OVERLAY_SIZE: Record<string, [number, number]> = {
  overlay: [172, 44],
  "overlay-live": [400, 120],
  "overlay-command": [300, 60],
  "overlay-downloading": [300, 60],
  "overlay-dismissed": [184, 40],
};

/** What each overlay preview shows: its state, and whether the mic is live. */
const OVERLAY_SHOW: Record<
  string,
  { state: string; levels: boolean; handsFree?: boolean; extra?: object }
> = {
  overlay: { state: "recording", levels: true },
  "overlay-preparing": { state: "recording", levels: false },
  "overlay-handsfree": { state: "recording", levels: true, handsFree: true },
  "overlay-live": { state: "recording", levels: true },
  "overlay-working": {
    state: "transcribing",
    levels: false,
    extra: { cleanupNext: true },
  },
  "overlay-processing": { state: "processing", levels: false },
  "overlay-command": { state: "command", levels: false },
  "overlay-downloading": {
    state: "downloading",
    levels: false,
    extra: { download: "parakeet" },
  },
  "overlay-dismissed": { state: "dismissed", levels: false },
};

/** The overlay fills its window, so it is drawn in a frame of that size. */
const OverlayFrame: React.FC<{ mode: string }> = ({ mode }) => {
  const [width, height] = OVERLAY_SIZE[mode] ?? OVERLAY_SIZE.overlay;
  return (
    <div
      style={{
        position: "absolute",
        insetInline: 0,
        bottom: 40,
        display: "flex",
        justifyContent: "center",
      }}
    >
      <iframe
        title="overlay"
        src={`${window.location.pathname}?w=frame-${mode}`}
        style={{
          width,
          height,
          border: 0,
          background: "transparent",
          colorScheme: "normal",
        }}
      />
    </div>
  );
};

const Overlay: React.FC<{ mode: string }> = ({ mode }) => {
  useEffect(() => {
    let interval = 0;
    const show = OVERLAY_SHOW[mode] ?? OVERLAY_SHOW.overlay;
    const timer = window.setTimeout(() => {
      void emit("show-overlay", {
        state: show.state,
        streamingWindow: mode === "overlay-live",
        interactive: true,
        handsFree: !!show.handsFree,
        justify: "center",
        epoch: 1,
        ...show.extra,
      });
      if (mode === "overlay-live") {
        void emit("stream-text", {
          committed: "Let's try the smaller model first and compare the ",
          tentative: "results before we switch",
        });
      }
      if (!show.levels) return;
      let tick = 0;
      interval = window.setInterval(() => {
        tick += 1;
        void emit(
          "mic-level",
          Array.from(
            { length: 16 },
            (_, i) => Math.abs(Math.sin(tick / 3 + i / 2)) * 0.8,
          ),
        );
      }, 60);
    }, 200);
    return () => {
      window.clearTimeout(timer);
      window.clearInterval(interval);
    };
  }, [mode]);
  return <RecordingOverlay />;
};

const Popup: React.FC<{ children: React.ReactNode }> = ({ children }) => (
  <div style={{ position: "absolute", top: 24, right: 24, width: 380 }}>
    {children}
  </div>
);

const Harness: React.FC = () => {
  const w = previewParams.get("w") ?? "ask";
  if (w.startsWith("frame-")) return <Overlay mode={w.slice(6)} />;
  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: BACKDROPS[previewParams.get("bg") ?? "light"],
      }}
    >
      {w === "orb-lab" && <OrbLab />}
      {w === "ask" && <Ask phase="prompt" />}
      {w === "ask-listening" && <Ask phase="listening" />}
      {w === "ask-working" && <Ask phase="working" />}
      {w === "ask-tool" && (
        <Ask phase="working" caption="Open YouTube in the browser…" />
      )}
      {w === "ask-speaking" && <Ask phase="speaking" />}
      {w === "ask-done" && <Ask phase="done" />}
      {w === "ask-insert" && <Ask phase="done" selection />}
      {w === "ask-error" && <Ask phase="error" />}
      {w === "ask-bottom" && <Ask phase="speaking" align="bottom" />}
      {w.startsWith("overlay") && <OverlayFrame mode={w} />}
      {w === "reminder" && (
        <Popup>
          <ReminderPopup />
        </Popup>
      )}
      {w === "approval" && (
        <Popup>
          <ApprovalPopup />
        </Popup>
      )}
    </div>
  );
};

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Harness />
  </React.StrictMode>,
);
