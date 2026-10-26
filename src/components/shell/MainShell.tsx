import React, { useState } from "react";
import AccessibilityPermissions from "@/components/AccessibilityPermissions";
import { Sidebar } from "@/components/Sidebar";
import UpdateChecker from "@/components/update-checker";
import { FeedbackHost } from "@/components/feedback/FeedbackDialog";
import { SettingsDialog } from "@/components/settings-dialog/SettingsDialog";
import { RetiredSelectionDialog } from "@/components/settings/models/RetiredSelectionPrompt";
import {
  INITIAL_NAVIGATION,
  NavigationProvider,
  type NavigationState,
} from "./navigation";
import { PageStack } from "./PageStack";
import { CommandPalette } from "./CommandPalette";
import { useSlotDataSync } from "./slotData";
import { useSpeechUpdateRequests } from "./speechUpdate";
import { useAutoSelectDownloadedModel } from "./useSttStatus";

/** Shell-wide effects that must run exactly once. */
const ShellEffects: React.FC = () => {
  useSlotDataSync();
  // Downloading a speech model switches to it once the file is on disk.
  useAutoSelectDownloadedModel(true);
  // A dictation refused because its model is retired opens the update prompt.
  useSpeechUpdateRequests();
  return null;
};

/**
 * The main window after onboarding: sidebar, the page stack, and the Settings
 * dialog. Split out of `App` so the browser preview (`src/preview`) renders the
 * exact same tree against a mocked backend.
 */
export const MainShell: React.FC<{ initialNavigation?: NavigationState }> = ({
  initialNavigation = INITIAL_NAVIGATION,
}) => {
  const [navigation, setNavigation] =
    useState<NavigationState>(initialNavigation);

  return (
    <NavigationProvider state={navigation} setState={setNavigation}>
      <ShellEffects />
      {/* One plain canvas: the navigation sits straight on it, and the page
          is the canvas itself. */}
      <div className="app-canvas flex flex-1 overflow-hidden">
        <Sidebar />
        <main className="relative isolate flex flex-1 flex-col overflow-hidden">
          <div className="mx-auto w-full max-w-5xl px-6 pt-5 empty:hidden sm:px-10">
            <AccessibilityPermissions />
          </div>
          <PageStack />
        </main>
      </div>
      <CommandPalette />
      <SettingsDialog />
      <FeedbackHost />
      {/* After an update that retired the selected speech model. */}
      <RetiredSelectionDialog />
      {/* Background update checks and the tray's "Check for updates…". What
          they find is shown by the sidebar's UpdatePill and Settings → About. */}
      <UpdateChecker />
    </NavigationProvider>
  );
};
