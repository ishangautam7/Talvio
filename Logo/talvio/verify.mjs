// Verify the Talvio asset rollout written by build-all.mjs.
//
//  - every generated file matches what the generators draw now (pixels);
//  - the tray icons pass the same assertions as the tests in
//    src-tauri/src/tray.rs (mirrored here so they can be checked without cargo);
//  - icon.ico has every Windows size, icon.icns parses with its legacy small
//    entries, iOS icons have no alpha, nothing still carries the old Zynveliq
//    teal tile.
//
// Run: node Logo/talvio/verify.mjs

import fs from "node:fs";
import path from "node:path";
import sharp from "sharp";
import { HERE, renderIcon, renderOpaqueIcon } from "./icon-kit.mjs";
import {
  renderColorState,
  renderMenuBar,
  renderSymbolicIdle,
} from "./tray.mjs";

const REPO = path.resolve(HERE, "..", "..");
const at = (rel) => path.join(REPO, rel);
let bad = 0;
const report = (ok, what) => {
  if (!ok) bad++;
  console.log(`${ok ? "OK  " : "FAIL"} ${what}`);
};
const raw = async (input) =>
  sharp(input).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
async function samePixels(rel, expected) {
  const a = await raw(fs.readFileSync(at(rel)));
  const b = await raw(expected);
  return a.info.width === b.info.width && a.data.equals(b.data);
}

console.log("== generated icons match the renderer ==");
for (const [rel, s] of [
  ["src-tauri/icons/32x32.png", 32],
  ["src-tauri/icons/64x64.png", 64],
  ["src-tauri/icons/128x128.png", 128],
  ["src-tauri/icons/128x128@2x.png", 256],
  ["src-tauri/icons/icon.png", 512],
  ["src-tauri/icons/Square30x30Logo.png", 30],
  ["src-tauri/icons/Square44x44Logo.png", 44],
  ["src-tauri/icons/StoreLogo.png", 50],
  ["src-tauri/resources/tray_idle.png", 32],
  ["src-tauri/resources/tray_idle_dark.png", 32],
  ["src-tauri/resources/talvio.png", 128],
  ["src-tauri/resources/window_icon_light.png", 128],
  ["src-tauri/resources/window_icon_dark.png", 128],
  ["Logo/talvio/png/icon-256.png", 256],
  ["docs-site/favicon.png", 256],
])
  report(await samePixels(rel, await renderIcon(s)), `${rel} (${s}px)`);
report(
  !fs.existsSync(at("src-tauri/resources/zynveliq.png")),
  "src-tauri/resources/zynveliq.png is gone",
);

console.log(
  "\n== tray.rs: symbolic_idle_icon_is_a_white_tile_with_the_bars_cut_out ==",
);
{
  const { data, info } = await raw(
    fs.readFileSync(at("src-tauri/resources/tray_idle_symbolic.png")),
  );
  const W = info.width;
  const A = (x, y) => data[(y * W + x) * 4 + 3];
  report(W === 64 && info.height === 64, `64x64 (is ${W}x${info.height})`);
  let offColour = 0,
    margin = 0;
  for (let y = 0; y < 64; y++)
    for (let x = 0; x < 64; x++) {
      const p = (y * W + x) * 4;
      if (
        data[p + 3] &&
        (data[p] !== 240 || data[p + 1] !== 240 || data[p + 2] !== 240)
      )
        offColour++;
      if ((x < 4 || x >= 60 || y < 4 || y >= 60) && data[p + 3]) margin++;
    }
  report(offColour === 0, `one colour 240,240,240 (${offColour} off)`);
  report(margin === 0, `4px margin transparent (${margin} not)`);
  report(
    [
      [4, 4],
      [59, 4],
      [4, 59],
      [59, 59],
    ].every(([x, y]) => A(x, y) < 64),
    "rounded corners",
  );
  report(
    A(32, 10) === 255 && A(32, 54) === 255,
    "tile above and below the bars",
  );
  for (let i = 0; i < 5; i++) {
    const x = 11 + 9 * i + 3;
    report(A(x, 32) === 0, `bar ${i} cut out at (${x},32)`);
    if (i < 4) report(A(x + 4, 32) === 255, `gap after bar ${i}`);
  }
  report(A(32, 20) === 0, "middle bar reaches high");
  report(A(14, 25) === 255, "outer bar stays short");
  report(
    await samePixels(
      "src-tauri/resources/tray_idle_symbolic.png",
      await renderSymbolicIdle(),
    ),
    "matches tray.mjs",
  );
}

console.log("\n== tray.rs: colour state glyphs shaped like the white ones ==");
for (const [colour, white] of [
  [
    "src-tauri/resources/recording.png",
    "src-tauri/resources/tray_recording.png",
  ],
  [
    "src-tauri/resources/transcribing.png",
    "src-tauri/resources/tray_transcribing.png",
  ],
]) {
  const c = await raw(fs.readFileSync(at(colour)));
  const w = await raw(fs.readFileSync(at(white)));
  let alphaDiff = 0,
    notCoral = 0;
  for (let i = 0; i < c.data.length; i += 4) {
    if (c.data[i + 3] !== w.data[i + 3]) alphaDiff++;
    if (
      c.data[i + 3] &&
      (c.data[i] !== 0xf2 || c.data[i + 1] !== 0x54 || c.data[i + 2] !== 0x3d)
    )
      notCoral++;
  }
  report(
    c.info.width === w.info.width && alphaDiff === 0,
    `${colour} has ${white}'s alpha`,
  );
  report(
    notCoral === 0,
    `${colour} is coral 242,84,61 (tray.rs test must expect this, not 0,140,158)`,
  );
}

console.log(
  "\n== tray.rs: macos_idle_icon_is_a_36px_colour_tile_with_a_transparent_margin ==",
);
{
  const { data, info } = await raw(
    fs.readFileSync(at("src-tauri/resources/tray_idle_macos.png")),
  );
  const W = info.width;
  const px = (x, y) => data.subarray((y * W + x) * 4, (y * W + x) * 4 + 4);
  report(W === 36 && info.height === 36, `36x36 (is ${W}x${info.height})`);
  let margin = 0;
  for (let y = 0; y < 36; y++)
    for (let x = 0; x < 36; x++)
      if ((x < 2 || x >= 34 || y < 2 || y >= 34) && px(x, y)[3]) margin++;
  report(margin === 0, `2px margin transparent (${margin} not)`);
  report(
    [
      [2, 2],
      [33, 2],
      [2, 33],
      [33, 33],
    ].every(([x, y]) => px(x, y)[3] < 64),
    "rounded corners",
  );
  let coral = 0,
    total = 0;
  for (let y = 10; y < 26; y++)
    for (let x = 10; x < 26; x++) {
      const [r, g, b, a] = px(x, y);
      total++;
      if (a === 255 && r - g > 60 && r - b > 60) coral++;
    }
  report(
    coral * 4 > total,
    `centre is the colour tile: ${coral}/${total} coral (r-g>60 && r-b>60)`,
  );
  report(
    await samePixels(
      "src-tauri/resources/tray_idle_macos.png",
      await renderMenuBar(),
    ),
    "matches tray.mjs",
  );
}

console.log("\n== icon.ico ==");
{
  const ico = fs.readFileSync(at("src-tauri/icons/icon.ico"));
  const n = ico.readUInt16LE(4);
  const sizes = [];
  for (let i = 0; i < n; i++) sizes.push(ico[6 + i * 16] || 256);
  sizes.sort((a, b) => a - b);
  report(
    [16, 24, 32, 48, 64, 256].every((s) => sizes.includes(s)),
    `entries ${sizes.join(", ")} include 16/24/32/48/64/256`,
  );
}

console.log("\n== icon.icns ==");
{
  const icns = fs.readFileSync(at("src-tauri/icons/icon.icns"));
  let ok =
    icns.subarray(0, 4).toString("ascii") === "icns" &&
    icns.readUInt32BE(4) === icns.length;
  const types = [];
  let o = 8;
  while (ok && o + 8 <= icns.length) {
    const len = icns.readUInt32BE(o + 4);
    if (len < 8 || o + len > icns.length) ok = false;
    else {
      const type = icns.subarray(o, o + 4).toString("ascii");
      const data = icns.subarray(o + 8, o + len);
      types.push(type);
      if (data[0] === 0x89) {
        const m = await sharp(data).metadata();
        const want = {
          ic07: 128,
          ic08: 256,
          ic09: 512,
          ic10: 1024,
          ic11: 32,
          ic12: 64,
          ic13: 256,
          ic14: 512,
        }[type];
        if (m.width !== want) ok = false;
      }
      o += len;
    }
  }
  report(
    ok && o === icns.length,
    `parses, PNG entries at their sizes: ${types.join(" ")}`,
  );
  report(
    ["is32", "s8mk", "il32", "l8mk"].every((t) => types.includes(t)),
    "legacy 16/32 entries present",
  );
}

console.log("\n== iOS icons: opaque ==");
{
  const set = "mobile/ios/Runner/Assets.xcassets/AppIcon.appiconset";
  const dirs = [set, "src-tauri/icons/ios"];
  for (const d of dirs)
    for (const f of fs.readdirSync(at(d)).filter((f) => f.endsWith(".png"))) {
      const m = await sharp(fs.readFileSync(at(`${d}/${f}`))).metadata();
      if (m.hasAlpha) report(false, `${d}/${f} has alpha`);
    }
  const contents = JSON.parse(
    fs.readFileSync(at(`${set}/Contents.json`), "utf8"),
  );
  let wrong = 0;
  for (const img of contents.images) {
    if (!img.filename) continue;
    const m = await sharp(
      fs.readFileSync(at(`${set}/${img.filename}`)),
    ).metadata();
    if (m.width !== Math.round(parseFloat(img.size) * parseFloat(img.scale)))
      wrong++;
  }
  report(
    wrong === 0,
    `Contents.json sizes match (${wrong} wrong); no alpha anywhere`,
  );
  report(
    await samePixels(
      `${set}/Icon-App-1024x1024@1x.png`,
      await renderOpaqueIcon(1024),
    ),
    "1024 marketing icon matches the renderer",
  );
}

console.log("\n== Android ==");
for (const res of [
  "src-tauri/icons/android",
  "mobile/android/app/src/main/res",
]) {
  const need = [
    "mipmap-anydpi-v26/ic_launcher.xml",
    "mipmap-anydpi-v26/ic_launcher_round.xml",
  ];
  for (const d of ["mdpi", "hdpi", "xhdpi", "xxhdpi", "xxxhdpi"])
    for (const f of [
      "ic_launcher",
      "ic_launcher_round",
      "ic_launcher_foreground",
      "ic_launcher_background",
    ])
      need.push(`mipmap-${d}/${f}.png`);
  const missing = need.filter((f) => !fs.existsSync(at(`${res}/${f}`)));
  report(
    missing.length === 0,
    `${res}: adaptive + legacy launchers (${missing.join(", ") || "all present"})`,
  );
}

console.log("\n== no old teal tile left ==");
{
  const files = [
    ...fs
      .readdirSync(at("src-tauri/icons"))
      .filter((f) => f.endsWith(".png"))
      .map((f) => `src-tauri/icons/${f}`),
    ...fs
      .readdirSync(at("src-tauri/resources"))
      .filter((f) => f.endsWith(".png"))
      .map((f) => `src-tauri/resources/${f}`),
  ];
  for (const rel of files) {
    const { data } = await raw(fs.readFileSync(at(rel)));
    let teal = 0,
      opaque = 0;
    for (let i = 0; i < data.length; i += 4) {
      if (data[i + 3] < 128) continue;
      opaque++;
      // Zynveliq's #153e47 tile and #adf5cf mark.
      if (
        Math.abs(data[i] - 0x15) < 12 &&
        Math.abs(data[i + 1] - 0x3e) < 12 &&
        Math.abs(data[i + 2] - 0x47) < 12
      )
        teal++;
    }
    if (teal / Math.max(1, opaque) > 0.05)
      report(false, `${rel} still looks like the Zynveliq tile`);
  }
  const svgs = [
    "public/favicon.svg",
    "brand/mark.svg",
    "docs-site/logo/icon.svg",
  ];
  report(
    svgs.every((f) => !/153e47|adf5cf/i.test(fs.readFileSync(at(f), "utf8"))),
    "SVGs carry no Zynveliq colours",
  );
}

console.log(bad === 0 ? "\nALL CHECKS PASSED" : `\n${bad} CHECK(S) FAILED`);
process.exit(bad === 0 ? 0 : 1);
