"use client";

// Moves its children up a little as the page scrolls. The layer is taller
// than the viewport by MAX_SHIFT, so its bottom edge never comes into view.
// Short pages move at SPEED; long pages move slower so the whole scroll fits
// in MAX_SHIFT. Off when the user prefers reduced motion.

import { useEffect, useRef, type ReactNode } from "react";
import settings from "./background-settings.json";

const SPEED = 0.15; // Fraction of the scroll distance.
const MAX_SHIFT = (settings.layerHeight - 100) / 100; // Fraction of the viewport height.

export default function Parallax({ children }: { children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)");
    let frame = 0;

    const update = () => {
      frame = 0;
      if (reduce.matches) {
        el.style.transform = "";
        return;
      }
      const maxShift = window.innerHeight * MAX_SHIFT;
      const maxScroll = document.documentElement.scrollHeight - window.innerHeight;
      const speed = maxScroll > 0 ? Math.min(SPEED, maxShift / maxScroll) : 0;
      const shift = Math.min(window.scrollY * speed, maxShift);
      el.style.transform = `translate3d(0, ${-shift}px, 0)`;
    };
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(update);
    };

    update();
    window.addEventListener("scroll", schedule, { passive: true });
    window.addEventListener("resize", schedule);
    reduce.addEventListener("change", schedule);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", schedule);
      reduce.removeEventListener("change", schedule);
    };
  }, []);

  return (
    <div ref={ref} style={{ height: `${settings.layerHeight}lvh` }} className="absolute inset-x-0 top-0 will-change-transform">
      {children}
    </div>
  );
}
