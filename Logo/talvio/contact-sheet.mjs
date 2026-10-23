// Contact sheet: every listed asset at real size (and 4x nearest-neighbour for
// the small ones), on a light and a dark strip, so they can be eyeballed.
// Usage (repo root): node Logo/talvio/contact-sheet.mjs <out.png> <file>...
import sharp from "sharp";
const [out, ...files] = process.argv.slice(2);
const rows = [];
let y = 10;
const W = 1420;
for (const f of files) {
  const m = await sharp(f).metadata();
  const s = Math.min(m.width, 256);
  const real = await sharp(f)
    .resize(s, s, {
      fit: "contain",
      background: { r: 0, g: 0, b: 0, alpha: 0 },
    })
    .png()
    .toBuffer();
  const zoom =
    m.width <= 64
      ? await sharp(f)
          .resize(m.width * 4, m.height * 4, { kernel: "nearest" })
          .png()
          .toBuffer()
      : null;
  const h = Math.max(s, zoom ? m.height * 4 : 0) + 20;
  const label = Buffer.from(
    `<svg xmlns="http://www.w3.org/2000/svg" width="560" height="20"><text x="0" y="14" font-family="Arial" font-size="13" fill="#333">${f.replace(/&/g, "&amp;")} ${m.width}x${m.height}${m.hasAlpha ? "" : " (no alpha)"}</text></svg>`,
  );
  rows.push({ input: label, left: 10, top: y });
  rows.push({ input: real, left: 580, top: y });
  rows.push({ input: real, left: 870, top: y });
  if (zoom) rows.push({ input: zoom, left: 1150, top: y });
  rows.push({ bg: true, top: y - 4, h, left: 860, w: 276 });
  y += h + 6;
}
const H = y + 10;
const layers = [];
for (const r of rows)
  if (r.bg)
    layers.push({
      input: {
        create: { width: r.w, height: r.h, channels: 4, background: "#202233" },
      },
      left: r.left,
      top: r.top,
    });
for (const r of rows)
  if (!r.bg) layers.push({ input: r.input, left: r.left, top: r.top });
await sharp({
  create: { width: W, height: H, channels: 4, background: "#F4F1EE" },
})
  .composite(layers)
  .png()
  .toFile(out);
console.log(out, W, H);
