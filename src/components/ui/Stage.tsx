import React from "react";
import { Trans } from "react-i18next";

/**
 * The live banner at the top of a feature page: a card lit by the page's
 * colour from one corner, with what the page is for on one side (a short
 * headline, one line under it, and the page's main action) and a live
 * picture of the feature on the other — the meeting recorder's two voices,
 * the assistant's orb, Home's voice line. The picture sits in a framed panel
 * of its own column, so it is part of the layout rather than art adrift
 * behind the text, and it moves with real state wherever there is some.
 *
 * Only pages whose banner carries the page's main action have one: Home (the
 * dictation keys), the Assistant (the ask and call keys) and Meetings
 * (Start). Everything else is a plain settings group.
 */
export const Stage: React.FC<{
  /** A short line over the title, in the page's colour (a status). */
  kicker?: React.ReactNode;
  title: React.ReactNode;
  /** One short line under the title. */
  subtitle?: React.ReactNode;
  /** The page's main action, or its keys. */
  actions?: React.ReactNode;
  /** The live picture, drawn in the panel on the end side. */
  picture?: React.ReactNode;
  /** A small control in the panel's corner (the recorder's indicator). */
  corner?: React.ReactNode;
  className?: string;
  /** Extra accessible name for the whole stage (the recorder's clock). */
  label?: string;
  /** Wider picture column (the recorder). */
  wide?: boolean;
}> = ({
  kicker,
  title,
  subtitle,
  actions,
  picture,
  corner,
  className = "",
  label,
  wide = false,
}) => (
  <section
    className={`stage @container rounded-[1.5rem] border border-hairline ${className}`}
    aria-label={label}
  >
    <div
      className={`grid gap-5 p-5 @3xl:items-stretch @3xl:gap-6 @3xl:p-6 ${
        picture
          ? wide
            ? "@3xl:grid-cols-[minmax(0,1fr)_minmax(0,1.15fr)]"
            : "@3xl:grid-cols-[minmax(0,1fr)_minmax(15rem,0.9fr)]"
          : ""
      }`}
    >
      <div className="flex min-w-0 flex-col justify-center py-1 @3xl:py-2">
        {kicker && <div className="stage-kicker mb-2.5">{kicker}</div>}
        <h2 className="hero-title">{title}</h2>
        {subtitle && (
          <p className="mt-2 max-w-md text-[0.9375rem] leading-relaxed text-muted text-pretty">
            {subtitle}
          </p>
        )}
        {actions && <div className="pt-5">{actions}</div>}
      </div>
      {picture && (
        <div className="stage-panel relative flex min-h-[9.5rem] min-w-0 items-stretch overflow-hidden rounded-[1.125rem]">
          <div className={`flex w-full ${corner ? "pt-8" : ""}`}>{picture}</div>
          {corner && <div className="absolute end-2.5 top-2.5">{corner}</div>}
        </div>
      )}
    </div>
  </section>
);

/**
 * A headline from a translation key, where `<em>…</em>` in the string is set
 * in the page's colour ("Every call, <em>written down.</em>"). A translation
 * without the tag simply renders plain.
 */
export const HeroTitle: React.FC<{ i18nKey: string }> = ({ i18nKey }) => (
  <Trans i18nKey={i18nKey} components={{ em: <em /> }} />
);

/**
 * A labelled shortcut on a stage: "Ask" over its keys. `hint` is how to use
 * it, one quiet line; `status` is something the user needs to know right now
 * ("Turn on AI cleanup to use this shortcut") and replaces the hint.
 */
export const HeroShortcut: React.FC<{
  label: string;
  hint?: React.ReactNode;
  status?: React.ReactNode;
  icon?: React.ReactNode;
  children: React.ReactNode;
}> = ({ label, hint, status, icon, children }) => (
  <div className="flex min-w-0 flex-col items-start">
    <span className="mb-2 inline-flex items-center gap-1.5 text-[0.8125rem] font-medium text-ink">
      {icon}
      {label}
    </span>
    {children}
    {status ? (
      <span className="mt-2 text-xs text-ink/85">{status}</span>
    ) : (
      hint && <span className="mt-2 text-xs text-muted">{hint}</span>
    )}
  </div>
);

/**
 * Bars a stage feeds with levels (0–1). With no levels they rest at a slow
 * breathing wave, so a picture never looks dead; under reduced motion they
 * hold still.
 */
export const LevelBars: React.FC<{
  levels: readonly number[];
  idle?: boolean;
  className?: string;
}> = ({ levels, idle = false, className = "" }) => (
  <span
    aria-hidden="true"
    className={`level-bars ${idle ? "is-idle" : ""} ${className}`}
  >
    {levels.map((level, index) => (
      <i
        key={index}
        style={{ "--l": level, "--i": index } as React.CSSProperties}
      />
    ))}
  </span>
);

/** A resting voice: a symmetric hump of levels with a little texture, the
 *  shape a sentence makes, for a stage that has nothing live to show. */
export const restingVoice = (count: number, seed = 1): number[] =>
  Array.from({ length: count }, (_, index) => {
    const x = index / Math.max(1, count - 1);
    const envelope = Math.sin(Math.PI * x) ** 1.4;
    const texture =
      0.55 +
      0.45 *
        Math.abs(Math.sin(index * 1.7 * seed + seed) * Math.cos(index * 0.6));
    return Math.max(0.06, Math.min(1, envelope * texture));
  });
