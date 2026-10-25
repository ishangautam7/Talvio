// Must be first: installs the fake Tauri backend before any module touches it.
import { previewParams } from "./mockBackend";

/*
 * Browser preview of the small floating windows. Dev-only, like `main.tsx`:
 * not a Vite build input, never shipped. Each window is drawn at about its
 * real size over a desktop-ish backdrop, so its transparent edges can be
 * judged.
 *
 * URL knobs: ?w=ask|ask-listening|ask-working|overlay|overlay-live|
 *               overlay-working|reminder|approval
 *            &bg=light|dark|photo
 */
import React, { useEffect } from "react";
import ReactDOM from "react-dom/client";
import { emit } from "@tauri-apps/api/event";
import "@fontsource-variable/inter";
import "../i18n";
import QuickAsk from "@/assistant/QuickAsk";
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

const Ask: React.FC<{ phase: "listening" | "working" | "done" }> = ({
  phase,
}) => (
  <div
    className="assistant-scope"
    style={
      {
        position: "absolute",
        insetInline: 0,
        bottom: 24,
        display: "flex",
        justifyContent: "center",
        "--as-msg-font": "15px",
        "--as-alpha": "1",
      } as React.CSSProperties
    }
  >
    <div style={{ width: 560 }}>
      <QuickAsk
        phase={phase}
        status={phase === "listening" ? "Listening" : "Thinking"}
        levels={[0.3, 0.7, 0.5]}
        question="What's the fastest way to rename a git branch?"
        answer={phase === "done" ? ANSWER : ""}
        selectionChars={0}
        screen={false}
        error={null}
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

/** The overlay window's own sizes (overlay.rs): the pill, the live card, and
 *  the labelled working pill. */
const OVERLAY_SIZE: Record<string, [number, number]> = {
  overlay: [96, 44],
  "overlay-live": [400, 120],
  "overlay-working": [300, 60],
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
    const timer = window.setTimeout(() => {
      const state = mode === "overlay-working" ? "transcribing" : "recording";
      void emit("show-overlay", {
        state,
        streamingWindow: mode === "overlay-live",
        interactive: true,
        justify: "center",
        epoch: 1,
      });
      if (mode === "overlay-live") {
        void emit("stream-text", {
          committed: "Let's try the smaller model first and compare the ",
          tentative: "results before we switch",
        });
      }
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
      {w === "ask" && <Ask phase="done" />}
      {w === "ask-listening" && <Ask phase="listening" />}
      {w === "ask-working" && <Ask phase="working" />}
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
