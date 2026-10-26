import { expect, test } from "bun:test";
import {
  approach,
  flowSpeed,
  followLevel,
  orbEnergy,
  speechLevel,
  targetWeights,
} from "./orbMotion";
import { orbModeForCall } from "./VoiceOrb";

test("easing is frame-rate independent", () => {
  const once = approach(0, 1, 0.016, 7);
  const twice = approach(approach(0, 1, 0.008, 7), 1, 0.008, 7);
  expect(Math.abs(once - twice)).toBeLessThan(1e-9);
  expect(approach(0.3, 1, 0, 7)).toBe(0.3);
});

test("a level rises fast and falls slowly", () => {
  const up = followLevel(0, 1, 0.05);
  const down = 1 - followLevel(1, 0, 0.05);
  expect(up).toBeGreaterThan(down * 2);
  expect(followLevel(0.5, Number.NaN, 0.05)).toBeLessThan(0.5);
});

test("speech loudness spreads across the whole swell", () => {
  expect(speechLevel(0)).toBe(0);
  expect(speechLevel(0.005)).toBe(0);
  expect(speechLevel(0.05)).toBeGreaterThan(0.2);
  expect(speechLevel(0.3)).toBe(1);
  expect(speechLevel(Number.NaN)).toBe(0);
});

test("a voice quickens the flow; reduced motion stops it", () => {
  const listening = targetWeights("listening");
  expect(flowSpeed(listening, 1, false)).toBeGreaterThan(
    flowSpeed(listening, 0, false),
  );
  expect(flowSpeed(listening, 1, true)).toBe(0);
});

test("thinking breathes, a voice lights, muted is dim", () => {
  const thinking = targetWeights("thinking");
  const a = orbEnergy(thinking, 0, 0, false);
  const b = orbEnergy(thinking, 0, Math.PI / 3.4, false);
  expect(a).not.toBe(b);
  expect(orbEnergy(targetWeights("speaking"), 1, 0, false)).toBeGreaterThan(
    orbEnergy(targetWeights("idle"), 0, 0, false),
  );
  expect(orbEnergy(targetWeights("muted"), 0, 0, false)).toBe(0);
});

test("the call's moments map onto the same orb", () => {
  expect(orbModeForCall("hearing")).toBe("listening");
  expect(orbModeForCall("responding")).toBe("thinking");
  expect(orbModeForCall("speaking")).toBe("speaking");
  expect(orbModeForCall("held")).toBe("muted");
  expect(orbModeForCall("error")).toBe("error");
});
