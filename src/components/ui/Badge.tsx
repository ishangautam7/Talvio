import React from "react";

interface BadgeProps {
  children: React.ReactNode;
  variant?: "primary" | "success" | "secondary" | "active" | "outline";
  className?: string;
}

const Badge: React.FC<BadgeProps> = ({
  children,
  variant = "primary",
  className = "",
}) => {
  const variantClasses = {
    primary: "bg-surface-strong text-ink",
    success: "bg-success/15 text-success",
    secondary: "bg-surface-strong text-muted",
    // Accent-tinted — the current selection.
    active: "bg-accent/12 text-accent",
    // Quiet outline — a suggestion ("Recommended"), distinct from the
    // active state and legible on any surface.
    outline: "border border-hairline-strong text-muted bg-transparent",
  };

  return (
    <span
      className={`badge inline-flex items-center rounded-full px-2 py-px text-[0.6875rem] leading-[1.125rem] font-semibold ${variantClasses[variant]} ${className}`}
    >
      {children}
    </span>
  );
};

export default Badge;
