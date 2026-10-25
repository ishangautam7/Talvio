import React, { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import {
  CircleUserRound,
  FlaskConical,
  Headphones,
  Info,
  Keyboard,
  ShieldCheck,
  SlidersHorizontal,
  TextCursorInput,
  Wrench,
} from "lucide-react";
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

type TabIcon = React.ComponentType<{
  className?: string;
  size?: number | string;
  strokeWidth?: number | string;
}>;

interface TabDef {
  id: SettingsTab;
  icon: TabIcon;
  component: React.ComponentType;
  /** One sentence of context, behind the (i) next to the tab's title. */
  intro?: string;
}

const TABS: TabDef[] = [
  { id: "general", icon: SlidersHorizontal, component: GeneralTab },
  { id: "account", icon: CircleUserRound, component: AccountTab },
  {
    id: "shortcuts",
    icon: Keyboard,
    component: ShortcutsTab,
    intro: "settingsDialog.shortcuts.intro",
  },
  { id: "audio", icon: Headphones, component: AudioTab },
  {
    id: "dictation",
    icon: TextCursorInput,
    component: DictationTab,
    intro: "settingsDialog.dictation.intro",
  },
  {
    id: "privacy",
    icon: ShieldCheck,
    component: PrivacyTab,
    intro: "settingsDialog.privacy.intro",
  },
  { id: "advanced", icon: Wrench, component: AdvancedTab },
  { id: "about", icon: Info, component: AboutTab },
  {
    id: "debug",
    icon: FlaskConical,
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
      <header className="shrink-0 border-b border-hairline bg-surface px-7 pt-6 pb-4">
        <div className="flex items-center justify-between gap-4">
          <h2 id={titleId} className="page-title text-[1.625rem] text-ink">
            {t("settingsDialog.title")}
          </h2>
          <DialogCloseButton
            onClick={closeSettings}
            label={t("common.close")}
          />
        </div>
        {/* Every tab in view at once, as chips: the one you are in is ink. */}
        <nav
          aria-label={t("settingsDialog.title")}
          className="mt-4 flex flex-wrap gap-1.5"
        >
          {tabs.map((tab) => {
            const Icon = tab.icon;
            const selected = tab.id === active.id;
            return (
              <button
                key={tab.id}
                type="button"
                aria-current={selected ? "page" : undefined}
                data-autofocus={selected ? "" : undefined}
                onClick={() => openSettings(tab.id)}
                className="settings-chip inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-full px-3 text-[0.8125rem] font-medium focus-visible:outline-none"
                data-selected={selected}
              >
                <Icon size={14} strokeWidth={selected ? 2.2 : 1.9} />
                <span className="whitespace-nowrap">
                  {t(`settingsDialog.tabs.${tab.id}`)}
                </span>
              </button>
            );
          })}
        </nav>
      </header>

      <div
        ref={bodyRef}
        className="settings-scroll min-h-0 flex-1 space-y-7 overflow-y-auto bg-canvas px-7 pt-6 pb-10"
      >
        <div className="flex min-w-0 items-center gap-1.5">
          <h3 className="font-display text-[1.25rem] text-ink">
            {t(`settingsDialog.tabs.${active.id}`)}
          </h3>
          {active.intro && (
            <InfoTip text={t(active.intro)} size="md" className="mt-1" />
          )}
        </div>
        <ActiveTab key={active.id} />
      </div>
    </Dialog>
  );
};
