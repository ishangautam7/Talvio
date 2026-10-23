// Rendering the Wave-T at every size the app ships.
//
// TWO TIERS
// Above SNAP_MAX px the 2048 px master (icon.svg rasterised once) is scaled
// down: at that size the 84-unit bars are several pixels wide and resampling
// is harmless. At or below it the mark is REDRAWN on the pixel grid instead
// (`snappedSvg`): bar widths, gaps and every bar end land on whole pixels, the
// centre bar is centred exactly (its width has the canvas's parity), and the
// three smallest sizes use a hand-tuned drawing with three thicker bars, since
// five 1.3 px bars with 0.5 px gaps are a grey smear at 16 px.
//
// Geometry comes from build-masters.mjs; nothing here re-derives the mark.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";
import {
  BAR,
  GAP,
  SIDE,
  STEM_BOTTOM,
  STEM_TOP,
  TILE,
  TREATMENTS,
  WAVE_Y,
  iconSvg,
  markShapes,
} from "./build-masters.mjs";

export const HERE = path.dirname(fileURLToPath(import.meta.url));
export const MASTER_SIZE = 2048;
export const SNAP_MAX = 64;

const CORAL_DEFS = TREATMENTS.coral.defs;
export const WHITE = "#FFFFFF";

/** The 2048 px master PNG of the primary icon (coral tile, white mark). */
let masterPromise;
export function master() {
  masterPromise ??= sharp(fs.readFileSync(path.join(HERE, "icon.svg")), {
    density: (72 * MASTER_SIZE) / 1024,
  })
    .resize(MASTER_SIZE, MASTER_SIZE)
    .png({ compressionLevel: 9 })
    .toBuffer();
  return masterPromise;
}

/**
 * Hand-tuned drawings in pixels. `bars` lists [x, top, bottom] per bar, all of
 * them `w` wide. The centre bar is always the stem.
 */
const HAND = {
  // 16: three 2 px bars, 2 px apart; side bars a 3 px nub, the stem 8 px.
  16: {
    inset: 0,
    rx: 3.5,
    w: 2,
    bars: [
      [3, 5, 9],
      [7, 3, 13],
      [11, 5, 9],
    ],
  },
  // 20: three 2 px bars with 2 px gaps, a little taller.
  20: {
    inset: 0,
    rx: 4.5,
    w: 2,
    bars: [
      [5, 6, 10],
      [9, 4, 16],
      [13, 6, 10],
    ],
  },
  // 24: all five bars, 2 px wide, 1 px apart.
  24: {
    inset: 0,
    rx: 5.5,
    w: 2,
    bars: [
      [5, 8, 11],
      [8, 7, 13],
      [11, 5, 19],
      [14, 7, 13],
      [17, 8, 11],
    ],
  },
  // 30 and 32: five 2 px bars read as hairlines here, and five 3 px bars
  // cannot be centred on an even canvas, so three 4 px bars 2 px apart.
  30: {
    inset: 1,
    rx: 6.5,
    w: 4,
    bars: [
      [7, 9, 15],
      [13, 7, 23],
      [19, 9, 15],
    ],
  },
  32: {
    inset: 1,
    rx: 7,
    w: 4,
    bars: [
      [8, 9, 16],
      [14, 7, 25],
      [20, 9, 16],
    ],
  },
};

/** Pick a whole-pixel bar width (same parity as the canvas) and gap. */
const GRID = { 40: { w: 4, g: 2 } }; // the cost below picks hairlines at 40

function snapGrid(size) {
  if (GRID[size]) return GRID[size];
  const k = size / 1024;
  const rawW = BAR * k;
  const rawG = GAP * k;
  const rawSpan = 5 * rawW + 4 * rawG;
  let best;
  for (let w = size % 2 ? 1 : 2; w <= Math.ceil(rawW) + 2; w += 2) {
    for (let g = Math.max(1, Math.ceil(w / 3)); g <= Math.ceil(rawG) + 2; g++) {
      const span = 5 * w + 4 * g;
      const cost =
        Math.abs(w - rawW) / rawW +
        0.6 * (Math.abs(g - rawG) / rawG) +
        2 * (Math.abs(span - rawSpan) / rawSpan);
      if (!best || cost < best.cost) best = { w, g, cost };
    }
  }
  return best;
}

function snappedSpec(size) {
  if (HAND[size]) return HAND[size];
  const k = size / 1024;
  const inset = Math.round(TILE.x * k);
  const rx = TILE.rx * k;
  const { w, g } = snapGrid(size);
  const left0 = size / 2 - w / 2 - 2 * (w + g);
  const cy = WAVE_Y * k;
  const half = (h) => Math.max(1, Math.round((h * k) / 2));
  const bars = [];
  const heights = Object.fromEntries(SIDE.map(([i, h]) => [i, h]));
  for (let i = -2; i <= 2; i++) {
    const x = left0 + (i + 2) * (w + g);
    if (i === 0) {
      bars.push([x, Math.round(STEM_TOP * k), Math.round(STEM_BOTTOM * k)]);
    } else {
      const c = Math.round(cy * 2) / 2; // centre on a pixel or half pixel
      const hh = half(heights[i]);
      bars.push([x, Math.round(c - hh), Math.round(c + hh)]);
    }
  }
  return { inset, rx, w, bars };
}

/** The mark drawn on the pixel grid of a `size` canvas. */
export function snappedMark(size, fill, spec = snappedSpec(size)) {
  return spec.bars
    .map(
      ([x, top, bottom]) =>
        `<rect x="${x}" y="${top}" width="${spec.w}" height="${bottom - top}" rx="${spec.w / 2}" fill="${fill}"/>`,
    )
    .join("");
}

export function snappedSvg(size) {
  const spec = snappedSpec(size);
  const t = spec.inset;
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${size} ${size}">` +
    `<defs>${CORAL_DEFS}</defs>` +
    `<rect x="${t}" y="${t}" width="${size - 2 * t}" height="${size - 2 * t}" rx="${spec.rx}" fill="url(#bg)"/>` +
    snappedMark(size, WHITE, spec) +
    `</svg>`
  );
}

/** Rasterise an SVG at 8x and reduce, so curved edges are antialiased evenly. */
export async function rasterSvg(svg, size, ss = 8) {
  return sharp(Buffer.from(svg), { density: 72 * ss })
    .resize(size, size, { kernel: "lanczos3" })
    .png({ compressionLevel: 9 })
    .toBuffer();
}

/** The primary app icon at `size`: snapped at small sizes, the master above. */
export async function renderIcon(size) {
  if (size <= SNAP_MAX) return rasterSvg(snappedSvg(size), size);
  return sharp(await master())
    .resize(size, size, { kernel: "lanczos3" })
    .png({ compressionLevel: 9 })
    .toBuffer();
}

/** The same icon with no alpha (iOS rejects icons with transparency): the
 *  coral tile fills the square, the system applies its own mask. */
export async function renderOpaqueIcon(size) {
  if (size <= SNAP_MAX) {
    const small = `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${size} ${size}"><defs>${CORAL_DEFS}</defs><rect width="${size}" height="${size}" fill="url(#bg)"/>${snappedMark(size, WHITE)}</svg>`;
    return sharp(await rasterSvg(small, size))
      .removeAlpha()
      .png({ compressionLevel: 9 })
      .toBuffer();
  }
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024"><defs>${CORAL_DEFS}</defs><rect width="1024" height="1024" fill="url(#bg)"/>${markShapes(WHITE)}</svg>`;
  const big = await rasterSvg(svg, Math.max(size, 512), 1);
  return sharp(big)
    .resize(size, size, { kernel: "lanczos3" })
    .flatten({ background: "#F8704C" })
    .removeAlpha()
    .png({ compressionLevel: 9 })
    .toBuffer();
}

/** The white Wave-T alone, scaled into `size` with the mark taking `share` of
 *  the canvas height (Android adaptive foreground: content in the safe 66%). */
export async function renderForeground(size, share = 0.5) {
  // Mark bounds on the 1024 grid: x 234..790, y 236..800.
  const markH = STEM_BOTTOM - STEM_TOP;
  const scale = (share * 1024) / markH;
  const cx = 512;
  const cy = (STEM_TOP + STEM_BOTTOM) / 2;
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024"><g transform="translate(512 512) scale(${scale}) translate(${-cx} ${-cy})">${markShapes(WHITE)}</g></svg>`;
  return rasterSvg(svg, size, Math.max(1, Math.ceil(size / 1024) * 2));
}

/** The coral tile clipped to a circle (Android legacy round launcher). */
export async function renderRound(size) {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024"><defs>${CORAL_DEFS}</defs><circle cx="512" cy="512" r="488" fill="url(#bg)"/><g transform="translate(512 512) scale(0.86) translate(-512 -512)">${markShapes(WHITE)}</g></svg>`;
  return rasterSvg(svg, size, Math.max(1, Math.ceil((size * 4) / 1024)));
}

export { iconSvg, TREATMENTS, markShapes };
