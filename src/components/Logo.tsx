import React, { useId } from "react";

interface LogoProps {
  className?: string;
  title?: string;
}

/**
 * The Talvio mark: the Wave-T (five waveform bars whose centre bar grows into
 * the stem of a T) in white on the coral tile, on the 1024 grid of
 * `Logo/talvio/icon.svg`.
 */
export const Logo: React.FC<LogoProps> = ({ className = "", title }) => {
  const gradient = `talvio-tile-${useId().replace(/:/g, "")}`;
  return (
    <svg
      viewBox="0 0 1024 1024"
      className={className}
      role="img"
      aria-hidden={title ? undefined : true}
      aria-label={title}
      xmlns="http://www.w3.org/2000/svg"
    >
      <defs>
        <linearGradient id={gradient} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#FF8F3F" />
          <stop offset="1" stopColor="#EE3A5C" />
        </linearGradient>
      </defs>
      <rect
        x="32"
        y="32"
        width="960"
        height="960"
        rx="224"
        fill={`url(#${gradient})`}
      />
      <g fill="#FFFFFF">
        <rect x="234" y="340" width="84" height="128" rx="42" />
        <rect x="352" y="286" width="84" height="236" rx="42" />
        <rect x="470" y="236" width="84" height="564" rx="42" />
        <rect x="588" y="286" width="84" height="236" rx="42" />
        <rect x="706" y="340" width="84" height="128" rx="42" />
      </g>
    </svg>
  );
};

export default Logo;
