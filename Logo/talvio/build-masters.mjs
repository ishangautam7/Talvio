// The Talvio Wave-T masters: three colour treatments plus the wordmark lockup.
//
// Importable: build-all.mjs and the app-asset generators reuse `markShapes`,
// the geometry constants, `TREATMENTS`, `iconSvg` and `wordPath` from here, so
// no other file re-derives the mark. Running it writes the masters next to it
// (icon.svg, icon-ink.svg, icon-paper.svg, lockup.svg, lockup-dark.svg) and a
// review sheet into preview/.
//
// Usage (from Logo/): node talvio/build-masters.mjs
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";
import opentype from "opentype.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const LOGO = path.dirname(HERE);

// Geometry on a 1024 grid. The waveform's centre line sits a little above the
// tile's centre so the stem below it balances the bars above.
export const BAR = 84; // bar width
export const GAP = 34; // space between bars
export const STEP = BAR + GAP;
export const MID = 512;
export const WAVE_Y = 404;
export const SIDE = [
  [-2, 128],
  [-1, 236],
  [1, 236],
  [2, 128],
];
export const STEM_TOP = 236;
export const STEM_BOTTOM = 800;

export function markShapes(fill) {
  const side = SIDE.map(([i, h]) => {
    const x = MID + i * STEP - BAR / 2;
    return `<rect x="${x}" y="${WAVE_Y - h / 2}" width="${BAR}" height="${h}" rx="${BAR / 2}" fill="${fill}"/>`;
  }).join("");
  const stem = `<rect x="${MID - BAR / 2}" y="${STEM_TOP}" width="${BAR}" height="${STEM_BOTTOM - STEM_TOP}" rx="${BAR / 2}" fill="${fill}"/>`;
  return side + stem;
}

export const TREATMENTS = {
  coral: {
    defs: `<linearGradient id="bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#FF8F3F"/><stop offset="1" stop-color="#EE3A5C"/></linearGradient>`,
    tile: "url(#bg)",
    mark: "#FFFFFF",
  },
  ink: {
    defs: `<linearGradient id="bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#262B6B"/><stop offset="1" stop-color="#11122E"/></linearGradient>
           <linearGradient id="mk" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#FFB25B"/><stop offset="1" stop-color="#FF5A6E"/></linearGradient>`,
    tile: "url(#bg)",
    mark: "url(#mk)",
  },
  paper: {
    defs: `<linearGradient id="mk" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#FF8F3F"/><stop offset="1" stop-color="#EE3A5C"/></linearGradient>`,
    tile: "#FFF8F2",
    mark: "url(#mk)",
  },
};

/** The tile: x=32 y=32 960x960 rx=224 on the 1024 grid. */
export const TILE = { x: 32, size: 960, rx: 224 };

export const iconSvg = (
  t,
) => `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
<defs>${t.defs}</defs>
<rect x="32" y="32" width="960" height="960" rx="224" fill="${t.tile}"/>
${markShapes(t.mark)}
</svg>`;

// Wordmark: "Talvio" as outlines from Plus Jakarta Sans 800.
const fontBytes = fs.readFileSync(
  path.join(LOGO, "fonts", "PlusJakartaSans-800.ttf"),
);
const font = opentype.parse(
  fontBytes.buffer.slice(
    fontBytes.byteOffset,
    fontBytes.byteOffset + fontBytes.byteLength,
  ),
);
export function wordPath(text, size, x, y, letterSpacing = -0.02) {
  let cursor = x;
  const parts = [];
  for (const ch of text) {
    const glyph = font.charToGlyph(ch);
    parts.push(glyph.getPath(cursor, y, size).toPathData(2));
    cursor +=
      (glyph.advanceWidth / font.unitsPerEm) * size + letterSpacing * size;
  }
  return { d: parts.join(" "), width: cursor - x };
}

/** Lockup proportions, shared with the app's share-card wordmark (wordmark.ts). */
export const LOCKUP = {
  wordSize: 300,
  iconSize: 340,
  gap: 70,
  height: 400,
  capHeight: 0.71,
};

export const lockupSvg = (t, ink) => {
  const { wordSize: size, iconSize, gap, height, capHeight } = LOCKUP;
  const word = wordPath("Talvio", size, 0, 0);
  const width = Math.ceil(iconSize + gap + word.width + 40);
  // Cap height of Plus Jakarta Sans is ~0.71 em; centre the caps on the icon.
  const baseline = height / 2 + (capHeight * size) / 2;
  const w = wordPath("Talvio", size, iconSize + gap, baseline);
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">
<defs>${t.defs}</defs>
<g transform="translate(0 ${(height - iconSize) / 2}) scale(${iconSize / 1024})">
<rect x="32" y="32" width="960" height="960" rx="224" fill="${t.tile}"/>${markShapes(t.mark)}
</g>
<path d="${w.d}" fill="${ink}"/>
</svg>`;
};

const MASTER_NAMES = {
  coral: "icon.svg",
  ink: "icon-ink.svg",
  paper: "icon-paper.svg",
};

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const OUT = path.join(HERE, "preview");
  fs.mkdirSync(OUT, { recursive: true });
  const shots = [];
  for (const [name, t] of Object.entries(TREATMENTS)) {
    const svg = iconSvg(t);
    fs.writeFileSync(path.join(HERE, MASTER_NAMES[name]), svg);
    await sharp(Buffer.from(svg))
      .resize(400)
      .png()
      .toFile(path.join(OUT, `icon_${name}.png`));
    for (const s of [32, 16]) {
      const small = await sharp(Buffer.from(svg)).resize(s).png().toBuffer();
      await sharp(small)
        .resize(128, 128, { kernel: "nearest" })
        .png()
        .toFile(path.join(OUT, `icon_${name}_${s}.png`));
    }
    shots.push(name);
  }
  const lightLockup = lockupSvg(TREATMENTS.coral, "#1B1D33");
  const darkLockup = lockupSvg(TREATMENTS.coral, "#FFFFFF");
  fs.writeFileSync(path.join(HERE, "lockup.svg"), lightLockup);
  fs.writeFileSync(path.join(HERE, "lockup-dark.svg"), darkLockup);
  await sharp(Buffer.from(lightLockup))
    .resize({ height: 160 })
    .flatten({ background: "#FFFFFF" })
    .png()
    .toFile(path.join(OUT, "lockup.png"));
  await sharp(Buffer.from(darkLockup))
    .resize({ height: 160 })
    .flatten({ background: "#15162B" })
    .png()
    .toFile(path.join(OUT, "lockup-dark.png"));

  // Sheet: three icons in a row with their 32/16 px checks, then both lockups.
  const lockMeta = await sharp(path.join(OUT, "lockup.png")).metadata();
  const W = Math.max(3 * 420 + 20, lockMeta.width + 40);
  const layers = [];
  shots.forEach((name, i) => {
    layers.push({
      input: path.join(OUT, `icon_${name}.png`),
      left: 20 + i * 420,
      top: 20,
    });
    layers.push({
      input: path.join(OUT, `icon_${name}_32.png`),
      left: 40 + i * 420,
      top: 440,
    });
    layers.push({
      input: path.join(OUT, `icon_${name}_16.png`),
      left: 190 + i * 420,
      top: 440,
    });
  });
  layers.push({ input: path.join(OUT, "lockup.png"), left: 20, top: 600 });
  layers.push({ input: path.join(OUT, "lockup-dark.png"), left: 20, top: 780 });
  await sharp({
    create: { width: W, height: 960, channels: 4, background: "#E9EBEF" },
  })
    .composite(layers)
    .png()
    .toFile(path.join(OUT, "sheet.png"));
  console.log("ok");
}
