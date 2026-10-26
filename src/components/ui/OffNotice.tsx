import React from "react";
import { Power } from "@/components/icons/glyphs";

/**
 * What a feature page says while its switch (in the header) is off: the
 * page's colour, a power glyph and one sentence on what turning it on does.
 */
export const OffNotice: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => (
  <div className="stage flex items-center gap-4 rounded-2xl border border-hairline px-5 py-4">
    <span className="hue-tile grid h-9 w-9 shrink-0 place-items-center rounded-lg">
      <Power className="h-4 w-4" aria-hidden="true" />
    </span>
    <p className="max-w-xl text-sm leading-relaxed text-body text-pretty">
      {children}
    </p>
  </div>
);
