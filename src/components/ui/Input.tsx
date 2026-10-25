import React from "react";

interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  variant?: "default" | "compact";
}

/** A text field, drawn by `.control` in App.css like every other field. */
export const Input = React.forwardRef<HTMLInputElement, InputProps>(
  ({ className = "", variant = "default", disabled, ...props }, ref) => {
    const baseClasses =
      "control text-sm rounded-lg text-ink placeholder:text-muted-soft text-start";

    const stateClasses = disabled
      ? "opacity-60 cursor-not-allowed bg-surface-strong"
      : "";

    const variantClasses = {
      default: "h-[2.125rem] px-3",
      compact: "h-8 px-2.5",
    } as const;

    return (
      <input
        ref={ref}
        className={`${baseClasses} ${variantClasses[variant]} ${stateClasses} ${className}`}
        disabled={disabled}
        {...props}
      />
    );
  },
);
Input.displayName = "Input";
