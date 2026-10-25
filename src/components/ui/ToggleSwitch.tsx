import React from "react";
import { SettingContainer } from "./SettingContainer";
import type { SettingIcon, SettingTone } from "./tones";

interface ToggleSwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  isUpdating?: boolean;
  label: string;
  description?: string;
  /** Deep-dive help behind the (i) hint. */
  info?: string;
  /** Optional leading icon tile (see SettingContainer). */
  icon?: SettingIcon;
  tone?: SettingTone;
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  tooltipPosition?: "top" | "bottom";
}

export const ToggleSwitch: React.FC<ToggleSwitchProps> = ({
  checked,
  onChange,
  disabled = false,
  isUpdating = false,
  label,
  description,
  info,
  icon,
  tone,
  descriptionMode = "tooltip",
  grouped = false,
  tooltipPosition = "top",
}) => {
  return (
    <SettingContainer
      title={label}
      description={description}
      info={info}
      icon={icon}
      tone={tone}
      descriptionMode={descriptionMode}
      grouped={grouped}
      disabled={disabled}
      tooltipPosition={tooltipPosition}
    >
      <label
        className={`inline-flex items-center ${disabled || isUpdating ? "cursor-not-allowed" : "cursor-pointer"}`}
      >
        <input
          type="checkbox"
          role="switch"
          aria-label={label}
          value=""
          className="sr-only peer"
          checked={checked}
          disabled={disabled || isUpdating}
          onChange={(e) => onChange(e.target.checked)}
        />
        <span aria-hidden="true" className="switch" />
      </label>
      {isUpdating && (
        <div className="absolute inset-0 flex items-center justify-center">
          <div className="w-4 h-4 border-2 border-ink border-t-transparent rounded-full animate-spin"></div>
        </div>
      )}
    </SettingContainer>
  );
};
