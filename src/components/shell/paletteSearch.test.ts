import { describe, expect, test } from "bun:test";
import { matchScore } from "./paletteSearch";

describe("matchScore", () => {
  test("an empty query matches everything", () => {
    expect(matchScore("Shortcuts", "")).toBeGreaterThan(0);
    expect(matchScore("Shortcuts", "   ")).toBeGreaterThan(0);
  });

  test("a prefix beats a word start, which beats a substring", () => {
    const prefix = matchScore("Cleanup model", "clean");
    const word = matchScore("AI cleanup", "clean");
    const inside = matchScore("Unclean", "clean");
    expect(prefix).toBeGreaterThan(word);
    expect(word).toBeGreaterThan(inside);
  });

  test("letters in order with gaps still match, below a substring", () => {
    expect(matchScore("Shortcuts", "shcut")).toBeGreaterThan(0);
    expect(matchScore("Shortcuts", "shcut")).toBeLessThan(
      matchScore("Shortcuts", "cut"),
    );
  });

  test("ignores case and accents", () => {
    expect(matchScore("Général", "gene")).toBeGreaterThan(0);
    expect(matchScore("HISTORY", "hist")).toBeGreaterThan(0);
  });

  test("no match is zero", () => {
    expect(matchScore("Models", "xyz")).toBe(0);
    expect(matchScore("Audio", "oa")).toBe(0);
  });
});
