import type React from "react";
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
  { id: "general", icon: SlidersHorizontal },
  { id: "account", icon: CircleUserRound },
  { id: "shortcuts", icon: Keyboard },
  { id: "audio", icon: Headphones },
  { id: "dictation", icon: TextCursorInput },
  { id: "privacy", icon: ShieldCheck },
  { id: "advanced", icon: Wrench },
  { id: "about", icon: Info },
  { id: "debug", icon: FlaskConical },
];
