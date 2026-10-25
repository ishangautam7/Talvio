import React from "react";

interface TextareaProps
  extends React.TextareaHTMLAttributes<HTMLTextAreaElement> {
  variant?: "default" | "compact";
}

export const Textarea: React.FC<TextareaProps> = ({
  className = "",
  variant = "default",
  ...props
}) => {
  const baseClasses =
    "control text-sm leading-relaxed rounded-lg text-ink placeholder:text-muted-soft text-start resize-y";

  const variantClasses = {
    default: "px-3 py-2 min-h-[100px]",
    compact: "px-2.5 py-1.5 min-h-[80px]",
  };

  return (
    <textarea
      className={`${baseClasses} ${variantClasses[variant]} ${className}`}
      {...props}
    />
  );
};
