import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { platform } from "@tauri-apps/plugin-os";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "@/components/icons/glyphs";
import { useSafeWindowDrag } from "@/lib/useSafeWindowDrag";
import LogoLockup from "./LogoLockup";

/**
 * Custom window title bar.
 *
 * The native window chrome is disabled on Windows/Linux (see the
 * `WebviewWindowBuilder` in `src-tauri/src/lib.rs`), so this bar carries the
 * brand and window controls and doubles as the drag region. On macOS the
 * window keeps an overlay title bar, so the native traffic lights float over
 * the left side and we only reserve space for them here.
 *
 * Rendered on every screen (onboarding included) so the window can always be
 * moved, resized, maximized, and closed.
 */
/** Where the main window's command palette button is portalled to. */
export const TITLEBAR_CENTER_ID = "titlebar-center";

export const TitleBar: React.FC = () => {
  const { t } = useTranslation();
  const isMac = platform() === "macos";
  // Same Windows move-loop hazard as the assistant pill: a click on the drag
  // region can leave the window stuck to the cursor and the desktop
  // unclickable (see useSafeWindowDrag). This is a real application window, so
  // it keeps the system move loop and its edge snapping. Double-click maximize
  // is handled below on Windows/Linux; only macOS leaves it to Tauri, whose
  // handler maximizes on mouseup there.
  useSafeWindowDrag({ systemMove: true, maximizeOnDoubleClick: isMac });
  const appWindow = React.useMemo(() => getCurrentWindow(), []);
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    if (isMac) return;

    let active = true;
    const refreshMaximizedState = () => {
      void appWindow
        .isMaximized()
        .then((maximized) => {
          if (active) setIsMaximized(maximized);
        })
        .catch((e) => console.warn("Failed to read window state:", e));
    };

    refreshMaximizedState();
    const unlisten = appWindow.onResized(refreshMaximizedState);

    return () => {
      active = false;
      void unlisten.then((stop) => stop());
    };
  }, [appWindow, isMac]);

  const handleMinimize = () => {
    appWindow
      .minimize()
      .catch((e) => console.warn("Failed to minimize window:", e));
  };

  const handleToggleMaximize = async () => {
    try {
      await appWindow.toggleMaximize();
      setIsMaximized(await appWindow.isMaximized());
    } catch (e) {
      console.warn("Failed to toggle maximize window:", e);
    }
  };

  const handleTitleBarDoubleClick = (event: React.MouseEvent) => {
    if (isMac || (event.target as HTMLElement).closest("button")) return;
    void handleToggleMaximize();
  };

  const handleClose = () => {
    // The backend intercepts close and hides the window to the tray
    // (see `on_window_event` in lib.rs), matching the native button.
    appWindow.close().catch((e) => console.warn("Failed to close window:", e));
  };

  return (
    <div
      data-tauri-drag-region
      onDoubleClick={handleTitleBarDoubleClick}
      className={`titlebar relative z-20 grid h-12 shrink-0 grid-cols-[1fr_auto_1fr] items-center text-ink ${
        isMac ? "pl-[78px] pr-3" : "ps-[1.375rem] pe-2"
      }`}
    >
      {/* The Wave-T mark and the wordmark. pointer-events-none keeps the strip
       *  draggable. */}
      <div className="flex items-center pointer-events-none select-none">
        <LogoLockup
          className="gap-2 [&>svg:last-child]:h-[0.8125rem]"
          iconClassName="h-[1.375rem] w-auto"
          toneClass="text-ink"
        />
      </div>

      {/* The main window puts its command palette's button here (see
          shell/CommandPalette.tsx); onboarding leaves it empty. */}
      <div id={TITLEBAR_CENTER_ID} className="flex min-w-0 justify-center" />

      {/* Windows/Linux use these controls; macOS keeps native traffic lights. */}
      {!isMac ? (
        <div className="flex items-center justify-self-end gap-0.5">
          <button
            type="button"
            aria-label={t("window.minimize")}
            title={t("window.minimize")}
            onClick={handleMinimize}
            className="flex items-center justify-center h-8 w-9 rounded-lg text-muted hover:text-ink hover:bg-ink/[0.06] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 transition-colors cursor-pointer"
          >
            <Minus width={16} height={16} />
          </button>
          <button
            type="button"
            aria-label={t(isMaximized ? "window.restore" : "window.maximize")}
            title={t(isMaximized ? "window.restore" : "window.maximize")}
            onClick={() => void handleToggleMaximize()}
            className="flex items-center justify-center h-8 w-9 rounded-lg text-muted hover:text-ink hover:bg-ink/[0.06] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 transition-colors cursor-pointer"
          >
            {isMaximized ? (
              <Copy width={13} height={13} />
            ) : (
              <Square width={13} height={13} />
            )}
          </button>
          <button
            type="button"
            aria-label={t("window.close")}
            title={t("window.close")}
            onClick={handleClose}
            className="flex items-center justify-center h-8 w-9 rounded-lg text-muted hover:text-white hover:bg-[#c42b1c] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-error/70 transition-colors cursor-pointer"
          >
            <X width={16} height={16} />
          </button>
        </div>
      ) : (
        <div />
      )}
    </div>
  );
};

export default TitleBar;
