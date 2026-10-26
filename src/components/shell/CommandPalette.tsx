import React, {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { CornerDownLeft, HelpGlyph, Search } from "@/components/icons/glyphs";
import { MODELS_ITEM, NAV_ITEMS } from "@/components/Sidebar";
import { TITLEBAR_CENTER_ID } from "@/components/TitleBar";
import { Keycaps } from "@/components/ui/Keycaps";
import { SETTINGS_TAB_META } from "@/components/settings-dialog/tabMeta";
import { useFeedbackDialog } from "@/components/feedback/feedbackStore";
import { useOsType } from "@/hooks/useOsType";
import { useSettings } from "@/hooks/useSettings";
import { MODEL_SLOTS, useNavigation } from "./navigation";
import { SLOT_HUES, SLOT_META } from "./SlotVisuals";
import { matchScore } from "./paletteSearch";

type Glyph = React.ComponentType<{ className?: string }>;

interface PaletteEntry {
  id: string;
  group: "pages" | "settings" | "models" | "actions";
  label: string;
  icon: Glyph;
  /** Whose colour the entry's tile wears (a page, a model job), or none. */
  hue: string;
  run: () => void;
}

const GROUP_ORDER: PaletteEntry["group"][] = [
  "pages",
  "models",
  "settings",
  "actions",
];

/** Ctrl+K, or ⌘K on a Mac: the one key that opens the palette. */
const isPaletteKey = (event: KeyboardEvent, mac: boolean) =>
  event.key.toLowerCase() === "k" &&
  !event.altKey &&
  !event.shiftKey &&
  (mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey);

/**
 * The command palette: every page, every Settings tab and every model job,
 * one search away. The rail is narrow because of it.
 *
 * It opens from the search button in the title bar or with Ctrl+K (⌘K), and
 * follows the WAI-ARIA combobox pattern: the field keeps the keyboard, the
 * arrow keys move through the list (`aria-activedescendant`), Enter opens the
 * highlighted entry and Escape closes. Nothing here acts on the user's other
 * apps; it only moves around this window.
 */
export const CommandPalette: React.FC = () => {
  const { t } = useTranslation();
  const os = useOsType();
  const mac = os === "macos";
  const [open, setOpen] = useState(false);
  const [slot, setSlot] = useState<HTMLElement | null>(null);

  useEffect(() => {
    setSlot(document.getElementById(TITLEBAR_CENTER_ID));
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!isPaletteKey(event, mac)) return;
      // A shortcut field that is recording owns every key.
      if (document.querySelector("[data-shortcut-recording]")) return;
      event.preventDefault();
      setOpen((value) => !value);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [mac]);

  const trigger = (
    <button
      type="button"
      onClick={() => setOpen(true)}
      aria-haspopup="dialog"
      aria-expanded={open}
      className="palette-trigger flex h-8 w-[min(26rem,40vw)] min-w-0 cursor-pointer items-center gap-2 rounded-lg ps-2.5 pe-1.5 text-start text-[0.8125rem] focus-visible:outline-none"
    >
      <Search className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
      <span className="min-w-0 flex-1 truncate">{t("palette.trigger")}</span>
      <Keycaps
        binding={mac ? "command+k" : "ctrl+k"}
        size="sm"
        className="palette-trigger-keys"
      />
    </button>
  );

  return (
    <>
      {slot && createPortal(trigger, slot)}
      {open && <PalettePanel onClose={() => setOpen(false)} />}
    </>
  );
};

const PalettePanel: React.FC<{ onClose: () => void }> = ({ onClose }) => {
  const { t } = useTranslation();
  const id = useId();
  const listId = `${id}-list`;
  const { navigateRoot, openSettings, openModelSlot } = useNavigation();
  const showFeedback = useFeedbackDialog((s) => s.show);
  const { settings } = useSettings();
  const debugMode = settings?.debug_mode ?? false;
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);

  const entries = useMemo<PaletteEntry[]>(
    () => [
      ...[...NAV_ITEMS, MODELS_ITEM].map((item) => ({
        id: `page-${item.id}`,
        group: "pages" as const,
        label: t(item.labelKey),
        icon: item.icon,
        hue: item.id,
        run: () => navigateRoot(item.id),
      })),
      ...MODEL_SLOTS.map((slot) => ({
        id: `model-${slot}`,
        group: "models" as const,
        label: t(`modelsHub.slots.${slot}.name`),
        icon: SLOT_META[slot].icon,
        hue: SLOT_HUES[slot],
        run: () => openModelSlot(slot),
      })),
      ...SETTINGS_TAB_META.filter((tab) => tab.id !== "debug" || debugMode).map(
        (tab) => ({
          id: `settings-${tab.id}`,
          group: "settings" as const,
          label: t(`settingsDialog.tabs.${tab.id}`),
          icon: tab.icon,
          hue: "neutral",
          run: () => openSettings(tab.id),
        }),
      ),
      {
        id: "action-feedback",
        group: "actions" as const,
        label: t("feedback.button"),
        icon: HelpGlyph,
        hue: "neutral",
        run: () => showFeedback(),
      },
    ],
    [t, navigateRoot, openModelSlot, openSettings, showFeedback, debugMode],
  );

  // Matches, best first within each group, and the group holding the best
  // match first, so the top entry is always the best one ("sh" opens
  // Shortcuts, not a page that happens to contain an s and an h).
  const { results, groups } = useMemo(() => {
    const scored = entries
      .map((entry) => ({ entry, score: matchScore(entry.label, query) }))
      .filter((item) => item.score > 0);
    const best = (group: PaletteEntry["group"]) =>
      Math.max(
        0,
        ...scored
          .filter((item) => item.entry.group === group)
          .map((item) => item.score),
      );
    const order = [...GROUP_ORDER].sort((a, b) => best(b) - best(a));
    return {
      groups: order,
      results: order.flatMap((group) =>
        scored
          .filter((item) => item.entry.group === group)
          .sort((a, b) => b.score - a.score)
          .map((item) => item.entry),
      ),
    };
  }, [entries, query]);

  useEffect(() => setActive(0), [query]);

  useLayoutEffect(() => {
    previousFocus.current = document.activeElement as HTMLElement | null;
    inputRef.current?.focus();
    return () => {
      const previous = previousFocus.current;
      if (previous && document.contains(previous)) {
        previous.focus({ preventScroll: true });
      }
    };
  }, []);

  // Keep the highlighted entry in view.
  useEffect(() => {
    listRef.current
      ?.querySelector<HTMLElement>(`[data-index="${active}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const run = useCallback(
    (entry: PaletteEntry | undefined) => {
      if (!entry) return;
      onClose();
      entry.run();
    },
    [onClose],
  );

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActive((index) => (results.length ? (index + 1) % results.length : 0));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActive((index) =>
        results.length ? (index - 1 + results.length) % results.length : 0,
      );
    } else if (event.key === "Enter") {
      event.preventDefault();
      run(results[active]);
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
    } else if (event.key === "Tab") {
      // The field is the only stop; the list is reached with the arrows.
      event.preventDefault();
    }
  };

  let index = -1;
  const optionId = (n: number) => `${id}-option-${n}`;

  return createPortal(
    <div
      className="fixed inset-0 z-[80] flex items-start justify-center px-4 pt-[11vh]"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        aria-hidden="true"
        className="dialog-scrim pointer-events-none absolute inset-0 bg-[rgb(12_12_16/0.38)] backdrop-blur-[8px]"
      />
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t("palette.label")}
        onKeyDown={onKeyDown}
        className="palette-panel relative flex max-h-[min(30rem,72vh)] w-[min(38rem,100%)] flex-col overflow-hidden rounded-3xl"
      >
        <div className="flex shrink-0 items-center gap-3 border-b border-hairline px-4">
          <Search className="h-[1.125rem] w-[1.125rem] shrink-0 text-muted" />
          <input
            ref={inputRef}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t("palette.placeholder")}
            role="combobox"
            aria-expanded="true"
            aria-controls={listId}
            aria-autocomplete="list"
            aria-activedescendant={
              results.length ? optionId(active) : undefined
            }
            spellCheck={false}
            className="h-14 min-w-0 flex-1 bg-transparent text-[1rem] text-ink placeholder:text-muted-soft focus:outline-none"
          />
          <Keycaps binding="escape" size="sm" />
        </div>

        <ul
          ref={listRef}
          id={listId}
          role="listbox"
          aria-label={t("palette.label")}
          className="min-h-0 shrink overflow-y-auto p-2"
        >
          {results.length === 0 && (
            <li
              role="presentation"
              className="px-3 py-10 text-center text-sm text-muted"
            >
              {t("palette.empty", { query })}
            </li>
          )}
          {groups.map((group) => {
            const inGroup = results.filter((entry) => entry.group === group);
            if (inGroup.length === 0) return null;
            return (
              <li key={group} role="presentation">
                <p
                  role="presentation"
                  className="px-3 pt-2.5 pb-1.5 text-xs font-semibold text-muted"
                >
                  {t(`palette.groups.${group}`)}
                </p>
                <ul role="presentation">
                  {inGroup.map((entry) => {
                    index += 1;
                    const n = index;
                    const selected = n === active;
                    const Icon = entry.icon;
                    return (
                      <li
                        key={entry.id}
                        id={optionId(n)}
                        role="option"
                        aria-selected={selected}
                        data-index={n}
                        data-hue={entry.hue}
                        onMouseMove={() => setActive(n)}
                        onClick={() => run(entry)}
                        className="palette-option flex h-10 cursor-pointer items-center gap-3 rounded-lg px-3 text-sm text-ink"
                      >
                        <span className="palette-glyph grid h-5 w-5 shrink-0 place-items-center">
                          <Icon className="h-4 w-4" aria-hidden="true" />
                        </span>
                        <span className="min-w-0 flex-1 truncate font-medium">
                          {entry.label}
                        </span>
                        {selected && (
                          <CornerDownLeft
                            className="h-3.5 w-3.5 shrink-0 text-muted rtl:-scale-x-100"
                            aria-hidden="true"
                          />
                        )}
                      </li>
                    );
                  })}
                </ul>
              </li>
            );
          })}
        </ul>

        <div className="flex shrink-0 items-center gap-4 border-t border-hairline px-4 py-2.5 text-xs text-muted">
          <span className="inline-flex items-center gap-1.5">
            <Keycaps binding="↑+↓" size="sm" />
            {t("palette.hints.move")}
          </span>
          <span className="inline-flex items-center gap-1.5">
            <Keycaps binding="enter" size="sm" />
            {t("palette.hints.open")}
          </span>
          <span className="inline-flex items-center gap-1.5">
            <Keycaps binding="escape" size="sm" />
            {t("palette.hints.close")}
          </span>
        </div>
      </div>
    </div>,
    document.body,
  );
};
