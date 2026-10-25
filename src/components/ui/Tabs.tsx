import React, { useId, useRef } from "react";

export interface TabItem<T extends string> {
  id: T;
  label: string;
  /** Small glyph before the label. */
  icon?: React.ComponentType<{ className?: string }>;
  /** Small trailing element (a count, a dot). */
  badge?: React.ReactNode;
}

/**
 * Words on a hairline, the view you are in underlined in ember: the lightest
 * way to split one page into views, so a page's views never look like a
 * setting (a segmented control is for choosing a value). Used
 * where the old design drilled into a sub-page and needed a Back button to get
 * out again — a tab is always one click from its siblings.
 *
 * Arrow keys move between tabs (roving tabindex), per the WAI-ARIA tabs pattern.
 */
export function Tabs<T extends string>({
  items,
  value,
  onChange,
  label,
  className = "",
  trailing,
}: {
  items: TabItem<T>[];
  value: T;
  onChange: (id: T) => void;
  /** Accessible name for the tab list. */
  label: string;
  className?: string;
  /** Controls at the right end of the tab row. */
  trailing?: React.ReactNode;
}) {
  const baseId = useId();
  const refs = useRef<Record<string, HTMLButtonElement | null>>({});

  const onKeyDown = (event: React.KeyboardEvent, index: number) => {
    if (event.key !== "ArrowRight" && event.key !== "ArrowLeft") return;
    event.preventDefault();
    const rtl =
      typeof document !== "undefined" && document.documentElement.dir === "rtl";
    const forward = (event.key === "ArrowRight") !== rtl;
    const next =
      items[(index + (forward ? 1 : items.length - 1)) % items.length];
    onChange(next.id);
    refs.current[next.id]?.focus();
  };

  return (
    <div
      className={`flex flex-wrap items-center justify-between gap-x-4 gap-y-2 ${className}`}
    >
      <div
        role="tablist"
        aria-label={label}
        className="tab-list no-scrollbar inline-flex max-w-full gap-1 overflow-x-auto"
      >
        {items.map((item, index) => {
          const selected = item.id === value;
          const Icon = item.icon;
          return (
            <button
              key={item.id}
              ref={(element) => {
                refs.current[item.id] = element;
              }}
              id={`${baseId}-${item.id}`}
              type="button"
              role="tab"
              aria-selected={selected}
              tabIndex={selected ? 0 : -1}
              onClick={() => onChange(item.id)}
              onKeyDown={(event) => onKeyDown(event, index)}
              className={`tab-pill relative inline-flex h-10 shrink-0 cursor-pointer items-center gap-1.5 rounded-md px-2.5 text-[0.875rem] whitespace-nowrap ${
                selected
                  ? "font-semibold text-ink"
                  : "font-medium text-muted hover:text-ink"
              }`}
            >
              {Icon && (
                <Icon
                  className={`h-3.5 w-3.5 shrink-0 ${selected ? "text-accent-fill" : "text-muted"}`}
                />
              )}
              {item.label}
              {item.badge}
            </button>
          );
        })}
      </div>
      {trailing && (
        <div className="flex shrink-0 items-center gap-1">{trailing}</div>
      )}
    </div>
  );
}
