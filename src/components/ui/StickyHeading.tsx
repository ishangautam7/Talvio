import React, { useEffect, useRef, useState } from "react";

/**
 * A list's day heading that sticks under the page's compact title while its
 * day scrolls past (`.sticky-day` in App.css). It is frosted only while stuck,
 * so at rest it draws no band across the sheet's light.
 *
 * Whether it is stuck is measured, not queried: CSS `scroll-state` container
 * queries would do it, but the build's CSS optimizer drops them. The usual
 * trick: shrink the observer's root by the sticky offset plus a pixel, so a
 * stuck heading is never wholly inside it.
 */
export const StickyHeading: React.FC<{
  as?: "h2" | "h3";
  children: React.ReactNode;
  className?: string;
}> = ({ as: Tag = "h2", children, className = "" }) => {
  const ref = useRef<HTMLHeadingElement>(null);
  const [stuck, setStuck] = useState(false);

  useEffect(() => {
    const element = ref.current;
    const root = element?.closest<HTMLElement>("[data-page]");
    if (!element || !root || typeof IntersectionObserver === "undefined") {
      return;
    }
    const offset = parseFloat(getComputedStyle(element).top) || 0;
    const observer = new IntersectionObserver(
      ([entry]) => {
        const top = (entry.rootBounds?.top ?? 0) + 1;
        setStuck(
          entry.intersectionRatio < 1 && entry.boundingClientRect.top <= top,
        );
      },
      {
        root,
        threshold: [1],
        rootMargin: `-${Math.ceil(offset) + 1}px 0px 0px 0px`,
      },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  return (
    <Tag ref={ref} data-stuck={stuck} className={`sticky-day ${className}`}>
      <span className="sticky-day-label px-1 py-1.5">{children}</span>
    </Tag>
  );
};
