// Regenerate every Talvio app asset from the Wave-T masters in this folder.
//
// One command, from the repo root or from Logo/:
//     node Logo/talvio/build-all.mjs
// then check it with:
//     node Logo/talvio/verify.mjs
//
// It replaces the old Zynveliq pipeline (apply-icon-to-app.mjs,
// build-small-icon.mjs, patch-icns-small.mjs, tray-glyphs.mjs) and the
// `tauri icon` step: the .ico and .icns are assembled here, so small entries
// carry the pixel-snapped drawing instead of a downscaled master.
//
// Sizes at or below 64 px are redrawn on the pixel grid (icon-kit.mjs); larger
// ones are scaled from the 2048 px master.

import fs from "node:fs";
import path from "node:path";
import sharp from "sharp";
import pngToIco from "png-to-ico";
import { LOCKUP, TREATMENTS, lockupSvg, wordPath } from "./build-masters.mjs";
import {
  HERE,
  master,
  rasterSvg,
  renderForeground,
  renderIcon,
  renderOpaqueIcon,
  renderRound,
  snappedSvg,
} from "./icon-kit.mjs";
import {
  MENU_BAR_SIZE,
  renderColorState,
  renderMenuBar,
  renderSymbolicIdle,
} from "./tray.mjs";

const REPO = path.resolve(HERE, "..", "..");
const at = (rel) => path.join(REPO, rel);
const written = [];

function write(rel, bytes) {
  const dest = at(rel);
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  fs.writeFileSync(dest, bytes);
  written.push(rel);
}

// ---------------------------------------------------------------------------
// Logo/talvio/png: the master and a size ladder (shareCard.ts reads icon-256).
// ---------------------------------------------------------------------------
write("Logo/talvio/png/icon-master-2048.png", await master());
const LADDER = [16, 20, 24, 32, 40, 48, 64, 96, 128, 180, 192, 256, 512, 1024];
for (const s of LADDER)
  write(`Logo/talvio/png/icon-${s}.png`, await renderIcon(s));
for (const [name, ink] of [
  ["lockup", "#1B1D33"],
  ["lockup-dark", "#FFFFFF"],
]) {
  const svg = lockupSvg(TREATMENTS.coral, ink);
  for (const h of [128, 256])
    write(
      `Logo/talvio/png/${name}-h${h}.png`,
      await sharp(Buffer.from(svg), { density: 72 * 2 })
        .resize({ height: h })
        .png({ compressionLevel: 9 })
        .toBuffer(),
    );
}

// ---------------------------------------------------------------------------
// src-tauri/icons: what `tauri icon` used to write, plus logo.png.
// ---------------------------------------------------------------------------
const TAURI_PNGS = [
  ["32x32.png", 32],
  ["64x64.png", 64],
  ["128x128.png", 128],
  ["128x128@2x.png", 256],
  ["icon.png", 512],
  ["logo.png", 1024],
  ["Square30x30Logo.png", 30],
  ["Square44x44Logo.png", 44],
  ["Square71x71Logo.png", 71],
  ["Square89x89Logo.png", 89],
  ["Square107x107Logo.png", 107],
  ["Square142x142Logo.png", 142],
  ["Square150x150Logo.png", 150],
  ["Square284x284Logo.png", 284],
  ["Square310x310Logo.png", 310],
  ["StoreLogo.png", 50],
];
for (const [name, s] of TAURI_PNGS)
  write(`src-tauri/icons/${name}`, await renderIcon(s));

// icon.ico: an exact entry for every size Windows asks for.
const ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256];
write(
  "src-tauri/icons/icon.ico",
  await pngToIco(await Promise.all(ICO_SIZES.map((s) => renderIcon(s)))),
);
write(
  "Logo/talvio/talvio.ico",
  fs.readFileSync(at("src-tauri/icons/icon.ico")),
);

// icon.icns: the entries Tauri wrote, same order. 16 and 32 px are the legacy
// RLE24 RGB + 8-bit mask pair; everything else is PNG.
function encodeRle24(planes) {
  const out = [];
  for (const plane of planes) {
    let i = 0;
    while (i < plane.length) {
      let run = 1;
      while (run < 130 && i + run < plane.length && plane[i + run] === plane[i])
        run++;
      if (run >= 3) {
        out.push(run + 125, plane[i]);
        i += run;
        continue;
      }
      let lit = 0;
      while (lit < 128 && i + lit < plane.length) {
        const a = plane[i + lit];
        if (
          i + lit + 2 < plane.length &&
          plane[i + lit + 1] === a &&
          plane[i + lit + 2] === a
        )
          break;
        lit++;
      }
      if (lit === 0) lit = 1;
      out.push(lit - 1);
      for (let k = 0; k < lit; k++) out.push(plane[i + k]);
      i += lit;
    }
  }
  return Buffer.from(out);
}
function decodeRle24(buf, planeLen, planeCount) {
  const planes = [];
  let i = 0;
  for (let p = 0; p < planeCount; p++) {
    const plane = Buffer.alloc(planeLen);
    let w = 0;
    while (w < planeLen) {
      const ctl = buf[i++];
      if (ctl & 0x80) {
        const n = ctl - 125;
        const v = buf[i++];
        for (let k = 0; k < n && w < planeLen; k++) plane[w++] = v;
      } else {
        for (let k = 0; k < ctl + 1 && w < planeLen; k++) plane[w++] = buf[i++];
      }
    }
    planes.push(plane);
  }
  return planes;
}
async function legacyPair(size) {
  const { data } = await sharp(await renderIcon(size))
    .ensureAlpha()
    .raw()
    .toBuffer({ resolveWithObject: true });
  const n = size * size;
  const planes = [0, 1, 2].map((c) =>
    Buffer.from(Array.from({ length: n }, (_, i) => data[i * 4 + c])),
  );
  const alpha = Buffer.from(
    Array.from({ length: n }, (_, i) => data[i * 4 + 3]),
  );
  const rle = encodeRle24(planes);
  const back = decodeRle24(rle, n, 3);
  if (!back.every((p, i) => p.equals(planes[i])))
    throw new Error(`RLE24 round-trip failed at ${size}`);
  return [rle, alpha];
}
{
  const [is32, s8mk] = await legacyPair(16);
  const [il32, l8mk] = await legacyPair(32);
  const entries = [
    ["ic07", await renderIcon(128)],
    ["il32", il32],
    ["l8mk", l8mk],
    ["ic13", await renderIcon(256)],
    ["ic11", await renderIcon(32)],
    ["ic09", await renderIcon(512)],
    ["ic14", await renderIcon(512)],
    ["ic08", await renderIcon(256)],
    ["ic12", await renderIcon(64)],
    ["ic10", await renderIcon(1024)],
    ["is32", is32],
    ["s8mk", s8mk],
  ];
  const chunks = entries.flatMap(([type, data]) => {
    const head = Buffer.alloc(8);
    head.write(type, 0, 4, "ascii");
    head.writeUInt32BE(data.length + 8, 4);
    return [head, data];
  });
  const body = Buffer.concat(chunks);
  const header = Buffer.alloc(8);
  header.write("icns", 0, 4, "ascii");
  header.writeUInt32BE(body.length + 8, 4);
  write("src-tauri/icons/icon.icns", Buffer.concat([header, body]));
}

// Tauri's iOS set: opaque, since the App Store rejects icons with alpha.
const TAURI_IOS = {
  "AppIcon-20x20@1x.png": 20,
  "AppIcon-20x20@2x.png": 40,
  "AppIcon-20x20@2x-1.png": 40,
  "AppIcon-20x20@3x.png": 60,
  "AppIcon-29x29@1x.png": 29,
  "AppIcon-29x29@2x.png": 58,
  "AppIcon-29x29@2x-1.png": 58,
  "AppIcon-29x29@3x.png": 87,
  "AppIcon-40x40@1x.png": 40,
  "AppIcon-40x40@2x.png": 80,
  "AppIcon-40x40@2x-1.png": 80,
  "AppIcon-40x40@3x.png": 120,
  "AppIcon-60x60@2x.png": 120,
  "AppIcon-60x60@3x.png": 180,
  "AppIcon-76x76@1x.png": 76,
  "AppIcon-76x76@2x.png": 152,
  "AppIcon-83.5x83.5@2x.png": 167,
  "AppIcon-512@2x.png": 1024,
};
for (const [name, s] of Object.entries(TAURI_IOS))
  write(`src-tauri/icons/ios/${name}`, await renderOpaqueIcon(s));

// ---------------------------------------------------------------------------
// Android launchers (Tauri's android set and the Flutter phone app share the
// shape): legacy square + round PNGs, and an adaptive icon whose background is
// the coral gradient and whose foreground is the white Wave-T inside the 66 dp
// safe zone of the 108 dp canvas.
// ---------------------------------------------------------------------------
const DENSITIES = { mdpi: 1, hdpi: 1.5, xhdpi: 2, xxhdpi: 3, xxxhdpi: 4 };
const backgroundSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024"><defs>${TREATMENTS.coral.defs}</defs><rect width="1024" height="1024" fill="url(#bg)"/></svg>`;
const adaptiveXml = `<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
  <background android:drawable="@mipmap/ic_launcher_background"/>
  <foreground android:drawable="@mipmap/ic_launcher_foreground"/>
  <monochrome android:drawable="@mipmap/ic_launcher_foreground"/>
</adaptive-icon>
`;
for (const res of [
  "src-tauri/icons/android",
  "mobile/android/app/src/main/res",
]) {
  for (const [d, k] of Object.entries(DENSITIES)) {
    const dir = `${res}/mipmap-${d}`;
    write(`${dir}/ic_launcher.png`, await renderIcon(Math.round(48 * k)));
    write(
      `${dir}/ic_launcher_round.png`,
      await renderRound(Math.round(48 * k)),
    );
    // Mark height 40% of the 108 dp canvas keeps every bar inside the 66 dp
    // circle a launcher may crop to.
    write(
      `${dir}/ic_launcher_foreground.png`,
      await renderForeground(Math.round(108 * k), 0.4),
    );
    write(
      `${dir}/ic_launcher_background.png`,
      await rasterSvg(backgroundSvg, Math.round(108 * k), 1),
    );
  }
  write(`${res}/mipmap-anydpi-v26/ic_launcher.xml`, adaptiveXml);
  write(`${res}/mipmap-anydpi-v26/ic_launcher_round.xml`, adaptiveXml);
}
// Tauri's generated set also has a background colour resource; keep it coral
// for anything that still refers to it.
write(
  "src-tauri/icons/android/values/ic_launcher_background.xml",
  `<?xml version="1.0" encoding="utf-8"?>
<resources>
  <color name="ic_launcher_background">#F2543D</color>
</resources>`,
);

// The Flutter iOS app: keep Contents.json, regenerate every PNG it names.
{
  const set = "mobile/ios/Runner/Assets.xcassets/AppIcon.appiconset";
  const contents = JSON.parse(
    fs.readFileSync(at(`${set}/Contents.json`), "utf8"),
  );
  const done = new Set();
  for (const img of contents.images) {
    if (!img.filename || done.has(img.filename)) continue;
    done.add(img.filename);
    const s = Math.round(parseFloat(img.size) * parseFloat(img.scale));
    write(`${set}/${img.filename}`, await renderOpaqueIcon(s));
  }
}

// ---------------------------------------------------------------------------
// Tray and window icons (src-tauri/resources). Sizes and shapes follow the
// tests in src-tauri/src/tray.rs; see tray.mjs.
// ---------------------------------------------------------------------------
write("src-tauri/resources/tray_idle.png", await renderIcon(32));
write("src-tauri/resources/tray_idle_dark.png", await renderIcon(32));
write("src-tauri/resources/talvio.png", await renderIcon(128));
write("src-tauri/resources/window_icon_light.png", await renderIcon(128));
write("src-tauri/resources/window_icon_dark.png", await renderIcon(128));
write("src-tauri/resources/tray_idle_macos.png", await renderMenuBar());
write("src-tauri/resources/tray_idle_symbolic.png", await renderSymbolicIdle());
write(
  "src-tauri/resources/recording.png",
  await renderColorState(at("src-tauri/resources/tray_recording.png")),
);
write(
  "src-tauri/resources/transcribing.png",
  await renderColorState(at("src-tauri/resources/tray_transcribing.png")),
);
const old = at("src-tauri/resources/zynveliq.png");
if (fs.existsSync(old)) {
  fs.rmSync(old);
  console.log("removed src-tauri/resources/zynveliq.png (now talvio.png)");
}

// ---------------------------------------------------------------------------
// Web: favicon (the 16 px drawing as vectors, so it is exact at 16 and 32),
// brand mark, docs site.
// ---------------------------------------------------------------------------
const favicon = snappedSvg(16).replace(
  `viewBox="0 0 16 16">`,
  `viewBox="0 0 16 16"><title>Talvio</title>`,
);
write("public/favicon.svg", `${favicon}\n`);
const iconMaster = fs.readFileSync(path.join(HERE, "icon.svg"));
write("brand/mark.svg", iconMaster);
write("docs-site/logo/icon.svg", iconMaster);
write(
  "docs-site/logo/lockup.svg",
  fs.readFileSync(path.join(HERE, "lockup.svg")),
);
write(
  "docs-site/logo/lockup-dark.svg",
  fs.readFileSync(path.join(HERE, "lockup-dark.svg")),
);
write("docs-site/favicon.png", await renderIcon(256));

// ---------------------------------------------------------------------------
// The share cards' wordmark (src/components/pages/insights/wordmark.ts): the
// lockup re-expressed with the icon 88 units tall at the origin.
// ---------------------------------------------------------------------------
{
  const f = 88 / LOCKUP.iconSize;
  const size = LOCKUP.wordSize * f;
  const word = wordPath("Talvio", size, 0, 0);
  const iconTop = (LOCKUP.height - LOCKUP.iconSize) / 2;
  const baseline = LOCKUP.height / 2 + (LOCKUP.capHeight * LOCKUP.wordSize) / 2;
  const origin = {
    x: +((LOCKUP.iconSize + LOCKUP.gap) * f).toFixed(3),
    y: +((baseline - iconTop) * f).toFixed(3),
  };
  const width = +(origin.x + word.width).toFixed(3);
  write(
    "src/components/pages/insights/wordmark.ts",
    `/**
 * The Talvio wordmark as vector paths, generated by
 * \`Logo/talvio/build-all.mjs\` from Plus Jakarta Sans 800 with the lockup's
 * proportions (\`Logo/talvio/lockup.svg\`), so a card can set it in whatever
 * colour its ground needs. The icon is an 88-unit square at the origin and the
 * word's baseline starts at \`WORDMARK_ORIGIN\`; one unit is one pixel of the
 * lockup at 88 px tall. Do not edit by hand.
 */

export const WORDMARK_ORIGIN = { x: ${origin.x}, y: ${origin.y} } as const;

/** The icon's size and the whole lockup's width, in lockup units. */
export const LOCKUP_ICON = 88;
export const LOCKUP_WIDTH = ${width};

/** The wordmark's own width and cap height, in lockup units. */
export const WORDMARK_WIDTH = ${+word.width.toFixed(3)};
export const WORDMARK_CAP_HEIGHT = ${+(LOCKUP.capHeight * size).toFixed(3)};

/** "Talvio", baseline at y = 0. */
export const WORDMARK_PATH =
  "${word.d}";
`,
  );
}

for (const rel of written)
  console.log(`${rel.padEnd(72)} ${fs.statSync(at(rel)).size}b`);
console.log(
  `\n${written.length} files written. Check with: node Logo/talvio/verify.mjs`,
);
