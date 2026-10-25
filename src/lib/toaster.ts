import type { ToasterProps } from "sonner";

/**
 * How every toast in the main window is drawn: a small floating card in the
 * app's own surface, with the float shadow, a status icon in its colour and
 * an action that is an ordinary secondary button. Shared by the app and its
 * browser preview so the two cannot drift.
 */
export const TOAST_OPTIONS: ToasterProps["toastOptions"] = {
  unstyled: true,
  classNames: {
    toast:
      "elev-float flex w-[var(--width)] items-center gap-3 rounded-xl bg-surface px-4 py-3 text-sm text-ink",
    title: "font-medium leading-snug",
    description: "mt-0.5 text-[0.8125rem] leading-snug text-muted",
    icon: "shrink-0 [&_svg]:h-4 [&_svg]:w-4",
    success: "[&_[data-icon]]:text-success",
    error: "[&_[data-icon]]:text-error",
    warning: "[&_[data-icon]]:text-warning",
    info: "[&_[data-icon]]:text-muted",
    actionButton:
      "btn btn-secondary ms-auto h-7 shrink-0 cursor-pointer rounded-lg px-2.5 text-[0.8125rem] font-medium",
    cancelButton:
      "btn btn-ghost h-7 shrink-0 cursor-pointer rounded-lg px-2.5 text-[0.8125rem] font-medium text-muted",
  },
};
