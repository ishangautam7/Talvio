import React from "react";

/**
 * A bare on/off switch, for page headers and cards where a full settings row
 * (`ToggleSwitch`) would be too much. Drawn by `.switch` in App.css, the same
 * track and knob as `ToggleSwitch`, so the two read as one control. `onHero`
 * is for a feature banner's stage.
 */
export const Switch: React.FC<{
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  /** Accessible name. Required: the switch has no visible label of its own. */
  label: string;
  className?: string;
  tone?: "default" | "onHero";
}> = ({
  checked,
  onChange,
  disabled = false,
  label,
  className = "",
  tone = "default",
}) => (
  <label
    className={`inline-flex shrink-0 items-center ${disabled ? "cursor-not-allowed" : "cursor-pointer"} ${className}`}
  >
    <input
      type="checkbox"
      role="switch"
      className="peer sr-only"
      checked={checked}
      disabled={disabled}
      aria-label={label}
      onChange={(event) => onChange(event.target.checked)}
    />
    <span
      aria-hidden="true"
      className={`switch ${tone === "onHero" ? "switch-hero" : ""}`}
    />
  </label>
);
