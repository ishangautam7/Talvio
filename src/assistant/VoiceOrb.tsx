import { useCallback, useRef } from "react";
import { LuminousOrb } from "./LuminousOrb";
import { speechLevel, type OrbMode } from "./orbMotion";
import type { ConversationPhase } from "./useVoiceConversation";

/** What the orb does for each moment of a call. */
export function orbModeForCall(phase: ConversationPhase): OrbMode {
  switch (phase) {
    case "listening":
    case "hearing":
      return "listening";
    case "starting":
    case "transcribing":
    case "responding":
      return "thinking";
    case "speaking":
      return "speaking";
    case "muted":
    case "held":
    case "off":
      return "muted";
    case "error":
      return "error";
  }
}

/**
 * The call's presence: the same luminous orb the quick ask is, at the call
 * bar's size. It swells with the user's voice while they speak and with the
 * assistant's while it answers (`speechLevel`, read from the call's audio
 * queue every frame).
 */
export function VoiceOrb({
  phase,
  level = 0,
  speechLevel: readSpeech,
  size = 26,
}: {
  phase: ConversationPhase;
  /** The microphone's level while the user speaks. */
  level?: number;
  /** The assistant's voice as an RMS, read once per frame while it speaks. */
  speechLevel?: () => number;
  size?: number;
}) {
  const levelRef = useRef(level);
  levelRef.current = level;
  const phaseRef = useRef(phase);
  phaseRef.current = phase;
  const speechRef = useRef(readSpeech);
  speechRef.current = readSpeech;
  const source = useCallback(() => {
    if (phaseRef.current === "speaking" && speechRef.current)
      return speechLevel(speechRef.current());
    return phaseRef.current === "hearing" ||
      (phaseRef.current === "speaking" && !speechRef.current)
      ? levelRef.current
      : 0;
  }, []);
  return (
    <LuminousOrb
      mode={orbModeForCall(phase)}
      size={size}
      level={phase === "hearing" ? level : 0}
      levelSource={source}
      className="call-voice-orb"
    />
  );
}
