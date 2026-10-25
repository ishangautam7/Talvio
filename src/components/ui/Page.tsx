import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronLeft } from "@/components/icons/glyphs";
import { PAGE_GLYPHS } from "@/components/icons/glyphs/pages";
import {
  useNavigation,
  type NavLocation,
  type PageId,
} from "@/components/shell/navigation";
import { InfoTip } from "./InfoTip";

/**
 * Page layout primitives for the main window.
 *
 * Hierarchy is carried by three steps and nothing else: a page title, a
 * section title one step down, and labels inside cards — all Inter, separated
 * by size and weight. Explanations live behind an InfoTip next to the title
 * they explain, or in one short line where a feature needs introducing.
 */

type PageWidth = "narrow" | "default" | "wide";

const WIDTHS: Record<PageWidth, string> = {
  narrow: "max-w-3xl",
  default: "max-w-5xl",
  wide: "max-w-6xl",
};

export const Page: React.FC<{
  width?: PageWidth;
  children: React.ReactNode;
  className?: string;
}> = ({ width = "default", children, className = "" }) => (
  <div
    className={`@container mx-auto w-full ${WIDTHS[width]} px-6 pt-6 pb-16 sm:px-10 ${className}`}
  >
    {children}
  </div>
);

/** The label a back link shows for a location. */
export const useLocationLabel = () => {
  const { t } = useTranslation();
  return (location: NavLocation): string => t(`nav.${location.page}`);
};

/**
 * The page's title again, small and centred in a frosted strip along the top
 * of the sheet, once the real title has scrolled away under it — so a long
 * page never loses its name. Hidden from screen readers: the heading itself
 * is still there.
 */
const useTitleScrolledAway = (heading: React.RefObject<HTMLElement | null>) => {
  const [away, setAway] = useState(false);
  useEffect(() => {
    const element = heading.current;
    const root = element?.closest<HTMLElement>("[data-page]");
    if (!element || !root || typeof IntersectionObserver === "undefined") {
      return;
    }
    const observer = new IntersectionObserver(
      ([entry]) => {
        const top = entry.rootBounds?.top ?? 0;
        setAway(!entry.isIntersecting && entry.boundingClientRect.top < top);
      },
      { root, threshold: 0 },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [heading]);
  return away;
};

const CompactTitle: React.FC<{ title: React.ReactNode; shown: boolean }> = ({
  title,
  shown,
}) => (
  <div className="sticky top-0 z-30 h-0" aria-hidden="true">
    <div className="page-titlebar" data-shown={shown}>
      <span className="min-w-0 truncate px-6 font-display text-[0.875rem] text-ink">
        {title}
      </span>
    </div>
  </div>
);

/** "‹ Home" — the way back to wherever the user came from. */
export const BackLink: React.FC<{
  label: string;
  onClick: () => void;
  className?: string;
}> = ({ label, onClick, className = "" }) => (
  <button
    type="button"
    onClick={onClick}
    className={`group -ms-2 inline-flex cursor-pointer items-center gap-0.5 rounded-lg py-1 ps-1 pe-2.5 text-sm font-medium text-muted transition-colors hover:bg-ink/[0.05] hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 ${className}`}
  >
    <ChevronLeft
      className="h-4 w-4 transition-transform duration-150 group-hover:-translate-x-0.5 motion-reduce:transition-none rtl:rotate-180"
      aria-hidden="true"
    />
    <span>{label}</span>
  </button>
);

interface PageHeaderProps {
  title: React.ReactNode;
  /** One sentence of context, shown behind an (i) next to the title. */
  description?: React.ReactNode;
  /** Controls at the right of the title row (switches, buttons). */
  actions?: React.ReactNode;
  /** An explicit back target (a drill-down inside a page). When omitted the
   *  header offers the navigation history's back step, if there is one. */
  onBack?: () => void;
  backLabel?: string;
  /** Small element after the title (a Beta badge). */
  badge?: React.ReactNode;
  /** A quiet line above the title (Home's date). */
  eyebrow?: React.ReactNode;
  /** The page whose glyph, on a tile of its colour, leads the title. */
  feature?: PageId;
  className?: string;
}

export const PageHeader: React.FC<PageHeaderProps> = ({
  title,
  description,
  actions,
  onBack,
  backLabel,
  badge,
  eyebrow,
  feature,
  className,
}) => {
  const Glyph = feature ? PAGE_GLYPHS[feature] : null;
  const { t } = useTranslation();
  const { back, goBack } = useNavigation();
  const labelFor = useLocationLabel();
  const headingRef = useRef<HTMLHeadingElement>(null);
  const titleAway = useTitleScrolledAway(headingRef);
  const backTarget = onBack
    ? { label: backLabel ?? t("common.back"), onClick: onBack }
    : back
      ? { label: labelFor(back), onClick: goBack }
      : null;

  // The compact title is the header's sibling, not its child: a sticky
  // element sticks only within its parent, and the page is the tall one.
  return (
    <>
      <CompactTitle title={title} shown={titleAway} />
      <header className={className ?? "mb-8"}>
        {backTarget && (
          <BackLink
            label={backTarget.label}
            onClick={backTarget.onClick}
            className="mb-3"
          />
        )}
        {eyebrow && <p className="eyebrow mb-2 text-muted">{eyebrow}</p>}
        <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-3">
          <div className="flex min-w-0 flex-1 items-center gap-3">
            {Glyph && (
              <span
                aria-hidden="true"
                data-hue={feature}
                className="hue-tile-solid grid h-10 w-10 shrink-0 place-items-center rounded-[0.8125rem]"
              >
                <Glyph className="h-[1.375rem] w-[1.375rem]" />
              </span>
            )}
            <h1 ref={headingRef} className="page-title text-ink">
              {title}
            </h1>
            {badge}
            {description && (
              <InfoTip text={description} size="md" className="mt-1" />
            )}
          </div>
          {actions && (
            <div className="flex shrink-0 flex-wrap items-center gap-2">
              {actions}
            </div>
          )}
        </div>
      </header>
    </>
  );
};

/** A small tag after a page's title (Beta), in the page's colour. */
export const TitleTag: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => (
  <span className="title-tag inline-flex h-6 items-center rounded-full px-2.5 text-xs font-semibold">
    {children}
  </span>
);

/** Heading for a block of content inside a page. */
export const SectionTitle: React.FC<{
  title: React.ReactNode;
  /** Context behind an (i), not a caption. */
  description?: React.ReactNode;
  action?: React.ReactNode;
  className?: string;
  as?: "h2" | "h3";
}> = ({ title, description, action, className = "", as = "h2" }) => {
  const Heading = as;
  // A fixed minimum height, so two sections side by side line up whether one
  // carries a text link and the other a segmented control.
  return (
    <div
      className={`mb-3 flex min-h-8 items-center justify-between gap-3 ${className}`}
    >
      <div className="flex min-w-0 items-center gap-2">
        <Heading className="font-display text-[1.0625rem] text-ink">
          {title}
        </Heading>
        {description && <InfoTip text={description} />}
      </div>
      {action && <div className="shrink-0">{action}</div>}
    </div>
  );
};

/** A quiet text link in a section header or card corner. */
export const TextLink: React.FC<{
  onClick: () => void;
  children: React.ReactNode;
  className?: string;
}> = ({ onClick, children, className = "" }) => (
  <button
    type="button"
    onClick={onClick}
    className={`inline-flex cursor-pointer items-center gap-1 rounded-md px-1.5 py-0.5 text-sm font-medium text-muted transition-colors hover:bg-ink/[0.05] hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 ${className}`}
  >
    {children}
  </button>
);
