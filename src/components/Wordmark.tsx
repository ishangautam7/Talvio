/* eslint-disable i18next/no-literal-string -- brand name is not translatable */
import React from "react";

interface WordmarkProps {
  className?: string;
}

/**
 * Zynveliq brand wordmark, in the brand sans (`.font-brand`) — the display
 * serif is for page titles, never for the name. "veliq" takes the accent so the
 * name and the brand color read as one mark.
 */
export const Wordmark: React.FC<WordmarkProps> = ({ className = "" }) => (
  <span
    className={`font-brand font-semibold tracking-tight text-ink leading-none select-none inline-block ${className}`}
  >
    Zyn<span className="text-accent">veliq</span>
  </span>
);

export default Wordmark;
