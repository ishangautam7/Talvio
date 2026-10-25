/**
 * How the orb moves: which state it is in, how fast its light flows, and how a
 * voice level becomes a swell. Pure, so the feel is a test rather than a
 * screenshot, and the renderer (`LuminousOrb.tsx`) only draws what this says.
 *
 * Every quantity eases toward its target on the frame clock instead of
 * jumping, so a change of state (listening → thinking → speaking) is a
 * continuous change of character, never a cut.
 */

export type OrbMode =
  /** On screen with nothing going on: a slow, quiet drift. */
  | "idle"
  /** The microphone is open; the level is the user's voice. */
  | "listening"
  /** Transcribing, writing the answer, using a tool. */
  | "thinking"
  /** The answer is being spoken; the level is the assistant's voice. */
  | "speaking"
  /** A call's microphone is off or held. */
  | "muted"
  /** Something went wrong. */
  | "error";

/** How much of each state's character is in the current frame, 0..1 each. */
export interface OrbWeights {
  listen: number;
  think: number;
  speak: number;
  mute: number;
  error: number;
}

export const ORB_MODES: readonly OrbMode[] = [
  "idle",
  "listening",
  "thinking",
  "speaking",
  "muted",
  "error",
];

export function targetWeights(mode: OrbMode): OrbWeights {
  return {
    listen: mode === "listening" ? 1 : 0,
    think: mode === "thinking" ? 1 : 0,
    speak: mode === "speaking" ? 1 : 0,
    mute: mode === "muted" ? 1 : 0,
    error: mode === "error" ? 1 : 0,
  };
}

/**
 * Move `current` toward `target` by the share of the gap a first-order system
 * with this `rate` (per second) closes in `dt` seconds. Frame-rate
 * independent: two 8 ms frames land where one 16 ms frame does.
 */
export function approach(
  current: number,
  target: number,
  dt: number,
  rate: number,
): number {
  if (!(dt > 0)) return current;
  return current + (target - current) * (1 - Math.exp(-dt * rate));
}

/** A state change takes about a third of a second to settle. */
export const MODE_RATE = 7;

export function approachWeights(
  current: OrbWeights,
  target: OrbWeights,
  dt: number,
): OrbWeights {
  return {
    listen: approach(current.listen, target.listen, dt, MODE_RATE),
    think: approach(current.think, target.think, dt, MODE_RATE),
    speak: approach(current.speak, target.speak, dt, MODE_RATE),
    mute: approach(current.mute, target.mute, dt, MODE_RATE),
    error: approach(current.error, target.error, dt, MODE_RATE),
  };
}

/**
 * Follow a voice level: rise quickly with a syllable, fall back slowly, the
 * way a meter's needle does. A level that fell as fast as it rose made the orb
 * flicker on every consonant.
 */
export function followLevel(current: number, input: number, dt: number) {
  const target = Math.max(0, Math.min(1, Number.isFinite(input) ? input : 0));
  return approach(current, target, dt, target > current ? 22 : 5);
}

/**
 * The assistant's voice level as the orb uses it, from the RMS the playback
 * thread reports (`assistant-tts-level`). Speech sits around 0.03–0.25 RMS, so
 * that range is spread across the orb's whole swell, with a little curve so
 * quiet syllables still show.
 */
export function speechLevel(rms: number): number {
  if (!Number.isFinite(rms) || rms <= 0.008) return 0;
  return Math.min(1, Math.pow((rms - 0.008) / 0.2, 0.7));
}

/**
 * How fast the light inside the orb flows, in noise units per second. Calm
 * when idle, quicker as a voice gets louder, and a steady, unhurried swirl
 * while it thinks. Reduced motion stops the flow altogether: the orb still
 * changes colour and brightness with its state, it just does not move.
 */
export function flowSpeed(
  weights: OrbWeights,
  level: number,
  reducedMotion: boolean,
): number {
  if (reducedMotion) return 0;
  const voice = weights.listen + weights.speak;
  return (
    0.1 +
    0.16 * weights.think +
    voice * (0.14 + 0.55 * level) -
    0.08 * weights.mute
  );
}

/**
 * How lit the orb is, 0..1: what drives its glow, its rim and the size of its
 * swell. A voice drives it directly; thinking is a slow breath so a long wait
 * still reads as alive.
 */
export function orbEnergy(
  weights: OrbWeights,
  level: number,
  time: number,
  reducedMotion: boolean,
): number {
  const voice = (weights.listen + weights.speak) * level;
  const breath = reducedMotion ? 0.5 : 0.5 + 0.5 * Math.sin(time * 1.7);
  const thinking = weights.think * (0.18 + 0.22 * breath);
  const resting = 0.12 * (1 - weights.mute) * (1 - weights.error);
  return Math.max(0, Math.min(1, resting + voice * 0.9 + thinking));
}
