import React from "react";
import { useTranslation } from "react-i18next";
import Logo from "./Logo";
import { WORDMARK_PATH, WORDMARK_WIDTH } from "./pages/insights/wordmark";

interface LogoLockupProps {
  /** Extra classes for the wrapper (margins, alignment, etc.). */
  className?: string;
  /** Sizing for the mark; the wordmark is tuned to sit beside `h-6`. */
  iconClassName?: string;
  /** Accessible label for the whole lockup. */
  title?: string;
}

/**
 * Talvio brand lockup: the Wave-T tile next to the "Talvio" wordmark.
 *
 * The wordmark is Plus Jakarta Sans 800 set as outlines (generated into
 * `wordmark.ts` by `Logo/talvio/build-all.mjs`), so it needs no font load and
 * fills with `currentColor`: pair the wrapper with `text-ink` and it follows
 * light and dark. Its cap height is 60% of the icon's, as in
 * `Logo/talvio/lockup.svg`.
 */
export const LogoLockup: React.FC<LogoLockupProps> = ({
  className = "",
  iconClassName = "h-6 w-auto",
  title,
}) => {
  const { t } = useTranslation();
  return (
    <div
      className={`flex items-center gap-2 text-ink select-none ${className}`}
      role="img"
      aria-label={title ?? t("settingsDialog.about.appName")}
    >
      <Logo className={`${iconClassName} shrink-0`} />
      <svg
        viewBox={`0 -60 ${Math.ceil(WORDMARK_WIDTH)} 61.5`}
        className="h-[1.05em] w-auto shrink-0 text-lg"
        aria-hidden
        xmlns="http://www.w3.org/2000/svg"
      >
        <path d={WORDMARK_PATH} fill="currentColor" />
      </svg>
    </div>
  );
};

export default LogoLockup;
