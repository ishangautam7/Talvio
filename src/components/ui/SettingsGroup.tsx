import React from "react";
import { InfoTip } from "./InfoTip";
import type { SettingIcon } from "./tones";

interface SettingsGroupProps {
  title?: string;
  /** Context for the whole group, behind an (i) next to the title. */
  description?: string;
  /** @deprecated Groups no longer draw a title icon. */
  icon?: SettingIcon;
  /** Control at the right end of the header (a link, a badge). */
  action?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}

/**
 * A titled card of setting rows. The title sits above the card as a small
 * label, and the rows sit inside it separated by inset hairlines (`.rows`).
 */
export const SettingsGroup: React.FC<SettingsGroupProps> = ({
  title,
  description,
  action,
  children,
  className = "",
}) => {
  return (
    <section className={`space-y-2 ${className}`}>
      {(title || action) && (
        <div className="flex items-end justify-between gap-3">
          <div className="flex min-w-0 items-center gap-1">
            {title && <h2 className="section-label">{title}</h2>}
            {description && <InfoTip text={description} />}
          </div>
          {action && <div className="shrink-0">{action}</div>}
        </div>
      )}
      <div className="overflow-visible rounded-2xl border border-hairline bg-surface elev-card">
        <div className="rows rounded-[inherit]">{children}</div>
      </div>
    </section>
  );
};
