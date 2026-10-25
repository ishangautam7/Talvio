import React, { useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  BookA,
  ChartColumn,
  CircleQuestionMark,
  Cloud,
  Cpu,
  History,
  House,
  MessageCircle,
  Mic,
  Settings,
  Users,
  Wand2,
  Zap,
} from "lucide-react";
import DownloadIndicator from "./model-selector/DownloadIndicator";
import { useFeedbackDialog } from "./feedback/feedbackStore";
import { useNavigation, type PageId } from "./shell/navigation";
import { TONE_DOT, useListeningStatus } from "./shell/useListeningStatus";
import { UpdatePill } from "./update-checker/UpdatePill";

type NavIcon = React.ComponentType<{
  className?: string;
  size?: number | string;
  strokeWidth?: number | string;
}>;

interface NavItem {
  id: PageId;
  labelKey: string;
  icon: NavIcon;
}

/**
 * Primary navigation: the rail. Pages are the things you *do* (and look back
 * at); the one page that configures models sits below a rule, and everything
 * else — the long tail of preferences — lives behind Settings at the bottom.
 * Every page is also one search away in the command palette (Ctrl+K), which
 * is why the rail can be this narrow.
 *
 * Insights closes the list: it is where you look back, not where you work, so
 * the pages used every day come first.
 */
export const NAV_ITEMS: NavItem[] = [
  { id: "home", labelKey: "nav.home", icon: House },
  { id: "history", labelKey: "nav.history", icon: History },
  { id: "assistant", labelKey: "nav.assistant", icon: MessageCircle },
  { id: "commands", labelKey: "nav.commands", icon: Zap },
  { id: "meetings", labelKey: "nav.meetings", icon: Users },
  { id: "cleanup", labelKey: "nav.cleanup", icon: Wand2 },
  { id: "dictionary", labelKey: "nav.dictionary", icon: BookA },
  { id: "insights", labelKey: "nav.insights", icon: ChartColumn },
];

export const MODELS_ITEM: NavItem = {
  id: "models",
  labelKey: "nav.models",
  icon: Cpu,
};

/**
 * One rail item: the page's glyph in a squircle with its name under it. The
 * page you are on wears the brand's coral gradient, the one bright object on
 * the frame. The name is always visible (truncated if a translation is long,
 * with the whole name as the tooltip), so the rail never relies on icons
 * alone.
 */
const RailButton: React.FC<{
  item: NavItem;
  active: boolean;
  onClick: () => void;
  buttonRef?: (element: HTMLButtonElement | null) => void;
}> = ({ item, active, onClick, buttonRef }) => {
  const { t } = useTranslation();
  const Icon = item.icon;
  const label = t(item.labelKey);
  return (
    <button
      ref={buttonRef}
      type="button"
      aria-current={active ? "page" : undefined}
      title={label}
      onClick={onClick}
      className="rail-item group relative z-10 flex w-full cursor-pointer flex-col items-center gap-1 rounded-xl py-1.5 focus-visible:outline-none"
      data-active={active}
    >
      <span className="rail-tile grid h-9 w-9 place-items-center rounded-[0.8125rem]">
        <Icon size={18} strokeWidth={active ? 2.1 : 1.8} aria-hidden="true" />
      </span>
      <span className="rail-caption w-full truncate px-0.5 text-center text-[0.6875rem] leading-tight">
        {label}
      </span>
    </button>
  );
};

/**
 * The bar along the rail's inline-start edge that marks the page you are on.
 * One element, moved rather than redrawn, so changing page slides it.
 */
const useRailIndicator = (activeId: PageId) => {
  const buttons = useRef<Partial<Record<PageId, HTMLButtonElement | null>>>({});
  const [box, setBox] = useState<{ top: number; height: number } | null>(null);
  const [animated, setAnimated] = useState(false);

  useLayoutEffect(() => {
    const element = buttons.current[activeId];
    if (!element) {
      setBox(null);
      return;
    }
    const measure = () =>
      setBox((current) =>
        current &&
        current.top === element.offsetTop &&
        current.height === element.offsetHeight
          ? current
          : { top: element.offsetTop, height: element.offsetHeight },
      );
    measure();
    // The UI scales with the display (rem), which moves every item.
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [activeId]);

  // The first placement is instant; only later moves slide.
  useLayoutEffect(() => {
    if (!box || animated) return;
    const frame = window.requestAnimationFrame(() => setAnimated(true));
    return () => window.cancelAnimationFrame(frame);
  }, [box, animated]);

  const refFor = (id: PageId) => (element: HTMLButtonElement | null) => {
    buttons.current[id] = element;
  };

  const indicator = box ? (
    <span
      aria-hidden="true"
      className={`rail-indicator ${animated ? "is-animated" : ""}`}
      style={{ top: box.top }}
    />
  ) : null;

  return { refFor, indicator };
};

/** The speech model's state as a dot on the rail; the model and its state
 *  are the tooltip and the accessible name, and a click opens its settings. */
const ListeningStatus: React.FC = () => {
  const { openModelSlot } = useNavigation();
  const { tone, isCloud, label } = useListeningStatus();
  const Glyph = isCloud ? Cloud : Mic;
  return (
    <button
      type="button"
      onClick={() => openModelSlot("stt")}
      title={label}
      aria-label={label}
      className="rail-plain relative grid h-9 w-9 cursor-pointer place-items-center rounded-[0.8125rem] focus-visible:outline-none"
    >
      <Glyph className="h-[1.0625rem] w-[1.0625rem]" aria-hidden="true" />
      <span
        aria-hidden="true"
        className={`absolute end-1.5 top-1.5 h-2 w-2 rounded-full ring-2 ring-frame ${TONE_DOT[tone]}`}
      />
    </button>
  );
};

export const Sidebar: React.FC = () => {
  const { t } = useTranslation();
  const { page, navigateRoot, openSettings, settingsTab } = useNavigation();
  const showFeedback = useFeedbackDialog((s) => s.show);
  const { refFor, indicator } = useRailIndicator(page);

  return (
    <aside className="frame-scope relative z-20 flex h-full w-[5.25rem] shrink-0 flex-col items-center px-2 pt-1 pb-3">
      {/* The pages scroll on their own when the window is short; the foot
          does not, so its popovers are never clipped. */}
      <div className="no-scrollbar -mx-2 min-h-0 w-[calc(100%+1rem)] flex-1 overflow-y-auto px-2">
        <nav
          className="relative flex w-full flex-col gap-0.5"
          aria-label={t("nav.label")}
        >
          {indicator}
          {NAV_ITEMS.map((item) => (
            <RailButton
              key={item.id}
              item={item}
              active={page === item.id}
              onClick={() => navigateRoot(item.id)}
              buttonRef={refFor(item.id)}
            />
          ))}
          <div className="mx-3 my-1.5 h-px bg-white/10" />
          <RailButton
            item={MODELS_ITEM}
            active={page === "models"}
            onClick={() => navigateRoot("models")}
            buttonRef={refFor("models")}
          />
        </nav>
      </div>

      <div className="mt-2 flex w-full shrink-0 flex-col items-center gap-1.5">
        <UpdatePill collapsed />
        <DownloadIndicator placement="start" compact />
        <ListeningStatus />
        <button
          type="button"
          onClick={() => showFeedback()}
          aria-haspopup="dialog"
          aria-label={t("feedback.button")}
          title={t("feedback.button")}
          className="rail-plain grid h-9 w-9 cursor-pointer place-items-center rounded-[0.8125rem] focus-visible:outline-none"
        >
          <CircleQuestionMark className="h-[1.0625rem] w-[1.0625rem]" />
        </button>
        <button
          type="button"
          onClick={() => openSettings()}
          aria-haspopup="dialog"
          aria-expanded={settingsTab !== null}
          aria-label={t("nav.settings")}
          title={t("nav.settings")}
          className="rail-plain group grid h-9 w-9 cursor-pointer place-items-center rounded-[0.8125rem] focus-visible:outline-none"
        >
          <Settings
            className="h-[1.0625rem] w-[1.0625rem] transition-transform duration-300 group-hover:rotate-45 motion-reduce:transition-none"
            strokeWidth={1.8}
          />
        </button>
      </div>
    </aside>
  );
};
