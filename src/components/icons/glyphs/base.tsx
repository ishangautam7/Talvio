import React from "react";

/**
 * Talvio Glyphs: the main window's own icon set.
 *
 * One drawing style for every icon: a 24-unit grid, a 1.7 stroke with round
 * caps and joins, and one "soft" shape per glyph filled with the glyph's own
 * colour at low opacity (`SOFT`), so an icon reads as a small object with a
 * body rather than as a wire outline. Utility glyphs (a check, a chevron, an
 * arrow) have no body and are stroke only.
 *
 * Every glyph takes the props the app already passed to lucide-react
 * (`className`, `size`, `strokeWidth`, `style`, aria attributes), so a call
 * site only changes its import.
 */
export interface GlyphProps extends React.SVGProps<SVGSVGElement> {
  size?: number | string;
  strokeWidth?: number | string;
  /** Accepted for lucide compatibility; glyph strokes never scale. */
  absoluteStrokeWidth?: boolean;
}

export type Glyph = React.FC<GlyphProps>;

/** The body of a glyph: its colour at low opacity, no stroke of its own. */
export const SOFT = {
  fill: "currentColor",
  fillOpacity: 0.16,
  stroke: "none",
} as const;

/** A shape painted solid in the glyph's colour (a dot, a sparkle's core). */
export const SOLID = { fill: "currentColor", stroke: "none" } as const;

export const glyph = (name: string, body: React.ReactNode): Glyph => {
  const Component: Glyph = ({
    size = 24,
    strokeWidth = 1.7,
    className,
    absoluteStrokeWidth: _absolute,
    children,
    ...rest
  }) => (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden={rest["aria-label"] ? undefined : true}
      focusable="false"
      className={`glyph glyph-${name}${className ? ` ${className}` : ""}`}
      {...rest}
    >
      {body}
      {children}
    </svg>
  );
  Component.displayName = `Glyph(${name})`;
  return Component;
};
