import React from "react";
import { Power } from "@/components/icons/glyphs";

/**
 * What a feature page says while its switch (in the header) is off: the
 * page's colour, a power glyph and one sentence on what turning it on does.
 */
export const OffNotice: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => (
  <div className="stage flex items-center gap-4 rounded-[1.5rem] border border-hairline px-5 py-5">
    <span className="hue-tile grid h-11 w-11 shrink-0 place-items-center rounded-[0.875rem]">
      <Power className="h-5 w-5" aria-hidden="true" />
    </span>
    <p className="max-w-xl text-sm leading-relaxed text-body text-pretty">
      {children}
    </p>
  </div>
);
