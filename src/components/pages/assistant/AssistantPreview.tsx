import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

const EXAMPLES = ["translate", "decline", "explain"] as const;
const CYCLE_MS = 5200;

const reducedMotion = () =>
  typeof window !== "undefined" &&
  window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;

/**
 * The Assistant page's picture: the assistant's orb beside one exchange — a
 * question asked out loud and the answer it got — turning over between three
 * real examples (translate, write a reply, explain), the same ones the
 * onboarding tour shows. It says what the feature does without a paragraph.
 * Under reduced motion it stays on the first example.
 */
export const AssistantPreview: React.FC = () => {
  const { t } = useTranslation();
  const [index, setIndex] = useState(0);

  useEffect(() => {
    if (reducedMotion()) return;
    const id = window.setInterval(
      () => setIndex((value) => (value + 1) % EXAMPLES.length),
      CYCLE_MS,
    );
    return () => window.clearInterval(id);
  }, []);

  const example = EXAMPLES[index];
  const base = `onboarding.tour.ask.examples.${example}`;

  return (
    <div
      className="flex w-full items-center gap-5 px-5 py-5"
      aria-hidden="true"
    >
      <span className="assistant-orb h-16 w-16 shrink-0" />
      <div key={example} className="style-swap min-w-0 flex-1 space-y-2.5">
        <div className="flex flex-wrap gap-1.5">
          {EXAMPLES.map((name) => (
            <span
              key={name}
              className={`rounded-md px-2 py-0.5 text-[0.6875rem] font-medium transition-colors ${
                name === example ? "bg-ink text-surface" : "hue-tile"
              }`}
            >
              {t(`onboarding.tour.ask.chips.${name}`)}
            </span>
          ))}
        </div>
        <p className="w-fit max-w-full rounded-2xl rounded-ss-md bg-surface px-3 py-2 text-[0.8125rem] font-medium text-ink shadow-[var(--shadow-raised)]">
          “{t(`${base}.question`)}”
        </p>
        <p className="line-clamp-3 text-[0.8125rem] leading-relaxed text-body">
          {t(`${base}.answer`)}
        </p>
      </div>
    </div>
  );
};
