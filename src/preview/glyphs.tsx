/* A developer preview of every Talvio glyph, never shipped. */
import React from "react";
import ReactDOM from "react-dom/client";
import * as Glyphs from "@/components/icons/glyphs";
import type { Glyph } from "@/components/icons/glyphs";

const entries = Object.entries(Glyphs).filter(
  (entry): entry is [string, Glyph] =>
    typeof entry[1] === "function" && entry[0] !== "glyph",
);

const themes = [
  { name: "light", bg: "#ffffff", ink: "#1b1d33", muted: "#8a8aa0" },
  { name: "dark", bg: "#141418", ink: "#ecebf3", muted: "#7c7c8c" },
];

const Sheet: React.FC<(typeof themes)[number]> = ({ bg, ink, muted }) => (
  <div
    style={{
      background: bg,
      color: ink,
      padding: 12,
      display: "grid",
      gridTemplateColumns: "repeat(auto-fill, minmax(96px, 1fr))",
      gap: 6,
    }}
  >
    {entries.map(([name, Icon]) => (
      <div
        key={name}
        style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          gap: 3,
          padding: "6px 2px",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Icon size={16} />
          <Icon size={20} />
          <Icon size={32} />
        </div>
        <div style={{ fontSize: 9, color: muted, fontFamily: "sans-serif" }}>
          {name}
        </div>
      </div>
    ))}
  </div>
);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <div style={{ margin: -8 }}>
    {themes.map((theme) => (
      <Sheet key={theme.name} {...theme} />
    ))}
  </div>,
);
