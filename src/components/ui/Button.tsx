import React from "react";

interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?:
    | "primary"
    | "accent"
    | "primary-soft"
    | "secondary"
    | "danger"
    | "danger-ghost"
    | "ghost";
  size?: "sm" | "md" | "lg";
}

/**
 * One button vocabulary for the whole window.
 *
 * `primary` is Talvio's ember, round and lit from above (`.btn-primary` in
 * App.css): the text-safe ember with white text on the light theme, `#FF6A52`
 * with graphite text on the dark, so the one action a view asks for reads as
 * the brand. Keep it to one per view; everything else is `secondary` or `ghost`.
 * `accent` is the same fill, kept for older call sites, and `primary-soft` a
 * quiet filled button.
 */
const VARIANT_CLASSES: Record<NonNullable<ButtonProps["variant"]>, string> = {
  primary: "btn-primary",
  accent: "btn-primary",
  "primary-soft": "btn-soft",
  secondary: "btn-secondary",
  danger: "btn-danger",
  "danger-ghost": "btn-danger-ghost",
  ghost: "btn-ghost",
};

const SIZE_CLASSES = {
  sm: "h-[1.875rem] px-3.5 text-[0.8125rem] rounded-full",
  md: "h-[2.125rem] px-4 text-sm rounded-full",
  lg: "h-10 px-5 text-[0.9375rem] rounded-full",
};

export const Button: React.FC<ButtonProps> = ({
  children,
  className = "",
  variant = "primary",
  size = "md",
  ...props
}) => (
  <button
    className={`btn inline-flex shrink-0 cursor-pointer items-center justify-center gap-1.5 font-medium whitespace-nowrap ${VARIANT_CLASSES[variant]} ${SIZE_CLASSES[size]} ${className}`}
    {...props}
  >
    {children}
  </button>
);
