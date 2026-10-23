// Tray icons for src-tauri/resources, drawn to the rules the tests in
// src-tauri/src/tray.rs assert.
//
// tray_idle_symbolic.png (Linux default): 64x64, one colour (240,240,240), a
//   4 px transparent margin around a rounded tile (x/y 4..60), with the Wave-T
//   cut out of it. The test pins the bar grid: 6 px bars on a 9 px pitch from
//   x = 11, every bar open at y = 32, tile solid at (32,10) and (32,54), the
//   stem open at (32,20), the outer bars closed at (14,25). So the waveform's
//   centre line sits at y = 30 and the stem runs 17..52: a T, not a plain
//   five-bar waveform, which is what the transcribing glyph is.
// tray_idle_macos.png: 36x36, the 32 px colour icon in a 2 px transparent
//   margin (18 pt @2x, the height the tray crate scales menu-bar images to).
// recording.png / transcribing.png ("Color tray icon" on Linux): the white
//   state glyphs' alpha, in brand coral.

import sharp from "sharp";
import { renderIcon } from "./icon-kit.mjs";

export const SIZE = 64;
export const WHITE = [240, 240, 240];
/** Brand coral (#F2543D): 3.4:1 on a white panel, 6:1 on black. */
export const CORAL = [0xf2, 0x54, 0x3d];
export const TILE = { x: 4, size: 56, corner: 0.225 };
/** [x, top, bottom] of each cut-out, 6 px wide. */
export const CUTS = [
  [11, 26, 34],
  [20, 21, 39],
  [29, 17, 52],
  [38, 21, 39],
  [47, 26, 34],
];
const CUT_W = 6;

export function symbolicIdleSvg() {
  const cuts = CUTS.map(
    ([x, t, b]) =>
      `<rect x="${x}" y="${t}" width="${CUT_W}" height="${b - t}" rx="${CUT_W / 2}" fill="black"/>`,
  ).join("");
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="${SIZE}" height="${SIZE}" viewBox="0 0 ${SIZE} ${SIZE}">` +
    `<mask id="cut"><rect width="${SIZE}" height="${SIZE}" fill="white"/>${cuts}</mask>` +
    `<rect x="${TILE.x}" y="${TILE.x}" width="${TILE.size}" height="${TILE.size}" rx="${TILE.size * TILE.corner}" fill="white" mask="url(#cut)"/>` +
    `</svg>`
  );
}

async function flat(alpha, [r, g, b], size = SIZE) {
  const px = Buffer.alloc(size * size * 4);
  for (let i = 0; i < size * size; i++) {
    px[i * 4] = alpha[i] ? r : 0;
    px[i * 4 + 1] = alpha[i] ? g : 0;
    px[i * 4 + 2] = alpha[i] ? b : 0;
    px[i * 4 + 3] = alpha[i];
  }
  return sharp(px, { raw: { width: size, height: size, channels: 4 } })
    .png({ compressionLevel: 9 })
    .toBuffer();
}

async function alphaOf(input) {
  const { data, info } = await sharp(input)
    .ensureAlpha()
    .raw()
    .toBuffer({ resolveWithObject: true });
  const n = info.width * info.height;
  const alpha = Buffer.alloc(n);
  for (let i = 0; i < n; i++) alpha[i] = data[i * 4 + 3];
  return { alpha, size: info.width };
}

/** Rendered at the target size (no resampling, so no ringing into the margin). */
export async function renderSymbolicIdle() {
  const { alpha } = await alphaOf(
    await sharp(Buffer.from(symbolicIdleSvg())).png().toBuffer(),
  );
  return flat(alpha, WHITE);
}

export async function renderColorState(whiteGlyph) {
  const { alpha, size } = await alphaOf(whiteGlyph);
  return flat(alpha, CORAL, size);
}

export const MENU_BAR_SIZE = 36;
export async function renderMenuBar() {
  const m = (MENU_BAR_SIZE - 32) / 2;
  return sharp(await renderIcon(32))
    .extend({
      top: m,
      bottom: m,
      left: m,
      right: m,
      background: { r: 0, g: 0, b: 0, alpha: 0 },
    })
    .png({ compressionLevel: 9 })
    .toBuffer();
}
