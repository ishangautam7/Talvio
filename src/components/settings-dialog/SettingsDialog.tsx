import React, { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Dialog, DialogCloseButton } from "@/components/ui/Dialog";
import { InfoTip } from "@/components/ui/InfoTip";
import { useSettings } from "@/hooks/useSettings";
import { useNavigation, type SettingsTab } from "@/components/shell/navigation";
import {
  AboutTab,
  AccountTab,
  AdvancedTab,
  AudioTab,
  DebugTab,
  DictationTab,
  GeneralTab,
  PrivacyTab,
  ShortcutsTab,
} from "./tabs";
import { SETTINGS_TAB_META } from "./tabMeta";

const ICONS = Object.fromEntries(
  SETTINGS_TAB_META.map((tab) => [tab.id, tab.icon]),
) as Record<SettingsTab, (typeof SETTINGS_TAB_META)[number]["icon"]>;

interface TabDef {
  id: SettingsTab;
  component: React.ComponentType;
  /** One sentence of context, behind the (i) next to the tab's title. */
  intro?: string;
}

const TABS: TabDef[] = [
  { id: "general", component: GeneralTab },
  { id: "account", component: AccountTab },
  {
    id: "shortcuts",
    component: ShortcutsTab,
    intro: "settingsDialog.shortcuts.intro",
  },
  { id: "audio", component: AudioTab },
  {
    id: "dictation",
    component: DictationTab,
    intro: "settingsDialog.dictation.intro",
  },
  {
    id: "privacy",
    component: PrivacyTab,
    intro: "settingsDialog.privacy.intro",
  },
  { id: "advanced", component: AdvancedTab },
  { id: "about", component: AboutTab },
  {
    id: "debug",
    component: DebugTab,
    intro: "sectionSubtitles.debug",
  },
];

/**
 * Settings, as a window over the app rather than a page in it.
 *
 * Everything that configures the *app* — shortcuts, microphone, how text is
 * typed, storage, updates — lives here, grouped into a short list of tabs, so
 * the sidebar can be about the things you actually do. Opened from the gear at
 * the bottom of the sidebar, or deep-linked to a tab from anywhere
 * (`openSettings("shortcuts")`).
 */
export const SettingsDialog: React.FC = () => {
  const { t } = useTranslation();
  const { settingsTab, openSettings, closeSettings } = useNavigation();
  const { settings } = useSettings();
  const bodyRef = useRef<HTMLDivElement>(null);
  const debugMode = settings?.debug_mode ?? false;

  const tabs = TABS.filter((tab) => tab.id !== "debug" || debugMode);
  const active =
    tabs.find((tab) => tab.id === settingsTab) ?? tabs[0] ?? TABS[0];
  const ActiveTab = active.component;

  useEffect(() => {
    bodyRef.current?.scrollTo({ top: 0 });
  }, [active.id]);

  // Leaving debug mode while its tab is open falls back to General.
  useEffect(() => {
    if (settingsTab === "debug" && !debugMode) openSettings("general");
  }, [settingsTab, debugMode, openSettings]);

  const titleId = "settings-dialog-title";

  return (
    <Dialog
      open={settingsTab !== null}
      onClose={closeSettings}
      size="drawer"
      bare
      labelledBy={titleId}
      bodyClassName=""
    >
      <div className="flex min-h-0 flex-1 max-sm:flex-col">
        {/* The categories down the start edge, drawn like the page
            navigation: the one you are in is a raised chip with its glyph
            in ember. */}
        <aside className="flex w-[14.5rem] shrink-0 flex-col border-e border-hairline bg-canvas-soft/60 px-3 pt-5 pb-4 max-sm:w-full max-sm:border-e-0 max-sm:border-b">
          <h2
            id={titleId}
            className="page-title mb-4 px-2.5 text-[1.5rem] text-ink"
          >
            {t("settingsDialog.title")}
          </h2>
          <nav
            aria-label={t("settingsDialog.title")}
            className="no-scrollbar flex min-h-0 flex-col gap-0.5 overflow-y-auto max-sm:flex-row max-sm:flex-wrap"
          >
            {tabs.map((tab) => {
              const Icon = ICONS[tab.id];
              const selected = tab.id === active.id;
              return (
                <button
                  key={tab.id}
                  type="button"
                  aria-current={selected ? "page" : undefined}
                  data-autofocus={selected ? "" : undefined}
                  onClick={() => openSettings(tab.id)}
                  className="settings-tab flex h-9 cursor-pointer items-center gap-2.5 rounded-[0.75rem] px-2.5 text-[0.875rem]"
                  data-selected={selected}
                >
                  <Icon className="settings-tab-glyph h-[1.125rem] w-[1.125rem] shrink-0" />
                  <span className="truncate">
                    {t(`settingsDialog.tabs.${tab.id}`)}
                  </span>
                </button>
              );
            })}
          </nav>
        </aside>

        <div className="flex min-w-0 flex-1 flex-col bg-canvas">
          <div className="flex shrink-0 items-center justify-between gap-4 px-8 pt-6 pb-2">
            <div className="flex min-w-0 items-center gap-1.5">
              <h3 className="font-display text-[1.375rem] text-ink">
                {t(`settingsDialog.tabs.${active.id}`)}
              </h3>
              {active.intro && (
                <InfoTip text={t(active.intro)} size="md" className="mt-1" />
              )}
            </div>
            <DialogCloseButton
              onClick={closeSettings}
              label={t("common.close")}
              className="mt-0"
            />
          </div>
          <div
            ref={bodyRef}
            className="settings-scroll min-h-0 flex-1 space-y-7 overflow-y-auto px-8 pt-3 pb-10"
          >
            <ActiveTab key={active.id} />
          </div>
        </div>
      </div>
    </Dialog>
  );
};
