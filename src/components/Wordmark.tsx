/* eslint-disable i18next/no-literal-string -- brand name is not translatable */
import React from "react";

interface WordmarkProps {
  className?: string;
}

/**
 * Talvio brand wordmark, in the brand sans (`.font-brand`). One word in one
 * colour: the Wave-T mark beside it carries the accent.
 */
export const Wordmark: React.FC<WordmarkProps> = ({ className = "" }) => (
  <span
    className={`font-brand font-semibold tracking-tight text-ink leading-none select-none inline-block ${className}`}
  >
    Talvio
  </span>
);

export default Wordmark;
