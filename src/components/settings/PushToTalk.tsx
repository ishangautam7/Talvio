import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

/** Dictation, the ask and a spoken command are each held or tapped on their
 *  own. */
export type RecordingModeSetting =
  | "push_to_talk"
  | "assistant_push_to_talk"
  | "command_push_to_talk";

const LABELS: Record<RecordingModeSetting, { label: string; info: string }> = {
  push_to_talk: {
    label: "settings.general.pushToTalk.dictationLabel",
    info: "settings.general.pushToTalk.dictation",
  },
  assistant_push_to_talk: {
    label: "settings.general.pushToTalk.assistantLabel",
    info: "settings.general.pushToTalk.assistant",
  },
  command_push_to_talk: {
    label: "settings.general.pushToTalk.commandLabel",
    info: "settings.general.pushToTalk.command",
  },
};

interface PushToTalkProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  /** Whose mode: dictation (the default), the ask, or a spoken command. */
  setting?: RecordingModeSetting;
}

/**
 * Hold to talk (or Hold to ask): a plain switch on a row of its own. On, the
 * keys are held while you speak; off, one press starts and the next one stops.
 * A Hold | Tap control on the same row as the keys made every row a cluster of
 * controls, and "Recording behavior" said nothing about what it changed.
 */
export const PushToTalk: React.FC<PushToTalkProps> = React.memo(
  ({
    descriptionMode = "tooltip",
    grouped = false,
    setting = "push_to_talk",
  }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const { label, info } = LABELS[setting];
    return (
      <ToggleSwitch
        checked={getSetting(setting) ?? true}
        onChange={(hold) => void updateSetting(setting, hold)}
        isUpdating={isUpdating(setting)}
        label={t(label)}
        description={t(info)}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);

PushToTalk.displayName = "PushToTalk";
