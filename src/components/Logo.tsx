import React from "react";

interface LogoProps {
  className?: string;
  title?: string;
}

export const Logo: React.FC<LogoProps> = ({ className = "", title }) => (
  <svg
    viewBox="0 0 256 256"
    fill="none"
    className={className}
    role="img"
    aria-hidden={title ? undefined : true}
    aria-label={title}
    xmlns="http://www.w3.org/2000/svg"
  >
    <rect
      x="12"
      y="12"
      width="232"
      height="232"
      rx="58"
      stroke="currentColor"
      strokeWidth="16"
    />
    <path
      d="M64 70h128l-88 116h88"
      stroke="currentColor"
      strokeWidth="20"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
    <path
      d="M68 112v32m120-32v32"
      stroke="currentColor"
      strokeWidth="12"
      strokeLinecap="round"
    />
  </svg>
);

export default Logo;
