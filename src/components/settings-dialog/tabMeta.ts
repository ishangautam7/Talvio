import type React from "react";
import {
  AboutGlyph,
  AccountGlyph,
  AdvancedGlyph,
  AudioGlyph,
  DebugGlyph,
  PrivacyGlyph,
  SettingsGlyph,
  ShortcutsGlyph,
  TypingGlyph,
} from "@/components/icons/glyphs";
import type { SettingsTab } from "@/components/shell/navigation";

type TabIcon = React.ComponentType<{
  className?: string;
  size?: number | string;
  strokeWidth?: number | string;
}>;

/**
 * The Settings window's tabs, in order, with their glyphs. Shared by the
 * Settings window and the command palette, so a tab found by search looks
 * like the tab it opens. Debug is listed only while debug mode is on.
 */
export const SETTINGS_TAB_META: { id: SettingsTab; icon: TabIcon }[] = [
  { id: "general", icon: SettingsGlyph },
  { id: "account", icon: AccountGlyph },
  { id: "shortcuts", icon: ShortcutsGlyph },
  { id: "audio", icon: AudioGlyph },
  { id: "dictation", icon: TypingGlyph },
  { id: "privacy", icon: PrivacyGlyph },
  { id: "advanced", icon: AdvancedGlyph },
  { id: "about", icon: AboutGlyph },
  { id: "debug", icon: DebugGlyph },
];
