import React from "react";

/**
 * What a list says when there is nothing in it yet: a quiet glyph in a soft
 * tile, one line of what will appear here, and at most one more line on how
 * to get it there. Drawn on the sheet itself (a dashed outline, no card), so
 * it reads as a place waiting to be filled rather than as content.
 */
export const EmptyState: React.FC<{
  icon: React.ComponentType<{ className?: string }>;
  title: React.ReactNode;
  hint?: React.ReactNode;
  action?: React.ReactNode;
  className?: string;
}> = ({ icon: Icon, title, hint, action, className = "" }) => (
  <div
    className={`empty-state flex flex-col items-center rounded-2xl border border-dashed border-hairline-strong px-6 py-12 text-center ${className}`}
  >
    <span className="empty-state-tile grid h-12 w-12 place-items-center rounded-[0.875rem] text-muted">
      <Icon className="h-5 w-5" aria-hidden="true" />
    </span>
    <p className="mt-4 text-[0.9375rem] font-medium text-ink text-balance">
      {title}
    </p>
    {hint && (
      <p className="mt-1.5 max-w-sm text-sm leading-relaxed text-muted text-pretty">
        {hint}
      </p>
    )}
    {action && <div className="mt-5">{action}</div>}
  </div>
);
