import { arc, arcPath, type SceneSettings } from "@link-flowers/shared";
import { pose, updateInteraction, type Plant, type Pointer } from "@link-flowers/shared";
import { motionConstants as constants } from "@link-flowers/shared/settings";

export interface AnimationClock { time: number }

/**
 * Owns SVG attributes only. React owns the scene structure and settings.
 * A background (`settles`) eases to a stop while the page is scrolled down,
 * so it never moves behind what's being read, and eases back at the top.
 */
export function animateScene(svg: SVGSVGElement, plants: Plant[], settings: SceneSettings, clock: AnimationClock, interactive: boolean, target: Element, paused: boolean, settles = false) {
  const media = matchMedia("(prefers-reduced-motion: reduce)");
  const pointer: Pointer = { active: false, x: 0, y: 0, skyX: 0, skyY: 0 };
  const parts = [...svg.querySelectorAll<SVGGElement>("[data-plant]")].map(group => ({
    stem: group.querySelector<SVGPathElement>("[data-stem]")!,
    head: group.querySelector<SVGGElement>("[data-head]")!,
    leaves: [...group.querySelectorAll<SVGUseElement>("[data-leaf]")],
  }));
  const sky = svg.querySelector<SVGGElement>("[data-sky]");
  let frame = 0, previous: number | undefined, visible = true, disposed = false;
  // How fast time runs, 0 to 1, and whether the page is scrolled down.
  let speed = 1, down = false;
  /** Whether the page is scrolled down: the window, or an element as tall as half of it (an app's own scrolling view), but never a box inside the page. */
  const scrolledDown = (event?: Event) => {
    const el = event?.target instanceof Element ? event.target : null;
    if (el && el.clientHeight >= innerHeight / 2) return el.scrollTop > constants.scrollRest;
    return scrollY > constants.scrollRest;
  };
  let bounds = svg.getBoundingClientRect();
  const refreshBounds = () => { bounds = svg.getBoundingClientRect(); };
  const clearPointer = () => { pointer.active = false; };
  const move = (event: Event) => {
    const e = event as PointerEvent;
    if (!interactive || media.matches || paused || e.pointerType === "touch") return;
    const box = svg.viewBox.baseVal;
    const scale = Math.min(bounds.width / box.width, bounds.height / box.height);
    const left = bounds.left + (bounds.width - box.width * scale) / 2;
    const top = bounds.top + (bounds.height - box.height * scale) / 2;
    pointer.x = (e.clientX - left) / scale + box.x; pointer.y = (e.clientY - top) / scale + box.y;
    pointer.active = e.clientX >= bounds.left && e.clientX <= bounds.right && e.clientY >= bounds.top && e.clientY <= bounds.bottom;
  };
  const render = (time: number) => {
    for (let i = 0; i < plants.length; i++) {
      const f = plants[i], elements = parts[i], at = pose(f, time, settings);
      elements.stem.setAttribute("d", arcPath(at.length, at.bend));
      elements.head.setAttribute("transform", `translate(${at.tip.x} ${at.tip.y}) rotate(${at.angle})`);
      for (let j = 0; j < elements.leaves.length; j++) {
        const u = f.leafPositions[j], p = arc(at.length, at.bend, u);
        elements.leaves[j].setAttribute("transform", `translate(${p.x} ${p.y}) rotate(${at.bend * u / constants.radians}) scale(${j ? -1 : 1} 1) rotate(${f.leafAngles[j]}) scale(${f.leafScales[j]})`);
      }
    }
    if (sky) {
      const drift = (1 - Math.cos(time / constants.skyPeriod * Math.PI * 2)) / 2;
      sky.setAttribute("transform", `translate(${pointer.skyX + drift * constants.skyDriftX} ${pointer.skyY + drift * constants.skyDriftY})`);
    }
  };
  const tick = (now: number) => {
    frame = 0;
    const dt = previous === undefined ? 0 : Math.min(constants.maxFrame, now - previous);
    previous = now;
    const goal = down ? 0 : 1;
    speed += Math.max(-dt / constants.settleTime, Math.min(dt / constants.settleTime, goal - speed));
    const step = dt * speed;
    clock.time += step;
    const box = svg.viewBox.baseVal;
    updateInteraction(plants, pointer, step, box.width, box.height, settings);
    render(clock.time);
    // At rest, scrolled down: no frames until the page is back at the top.
    if (!(down && speed === 0)) frame = requestAnimationFrame(tick);
  };
  const sync = () => {
    if (frame) cancelAnimationFrame(frame);
    frame = 0; previous = undefined;
    if (disposed) return;
    if (media.matches) {
      clearPointer(); pointer.skyX = pointer.skyY = 0;
      for (const f of plants) f.pushX = f.pushY = 0;
      render(0);
    } else {
      render(clock.time);
      if (!paused && !document.hidden && visible && !(down && speed === 0)) frame = requestAnimationFrame(tick);
    }
  };
  const visibility = () => { clearPointer(); sync(); };
  const observer = new IntersectionObserver(entries => { visible = entries[0].isIntersecting; sync(); });
  observer.observe(svg);
  target.addEventListener("pointermove", move, { passive: true });
  target.addEventListener("pointerleave", clearPointer);
  target.addEventListener("pointercancel", clearPointer);
  window.addEventListener("blur", clearPointer);
  window.addEventListener("resize", refreshBounds, { passive: true });
  const scroll = (event: Event) => {
    refreshBounds();
    if (!settles) return;
    const now = scrolledDown(event);
    if (now !== down) { down = now; if (!frame) sync(); }
  };
  if (settles) { down = scrolledDown(); speed = down ? 0 : 1; }
  window.addEventListener("scroll", scroll, { passive: true, capture: true });
  document.addEventListener("visibilitychange", visibility);
  media.addEventListener("change", sync);
  sync();
  return {
    setPaused(value: boolean) { paused = value; clearPointer(); sync(); },
    dispose() {
      disposed = true; if (frame) cancelAnimationFrame(frame);
      observer.disconnect();
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerleave", clearPointer);
      target.removeEventListener("pointercancel", clearPointer);
      window.removeEventListener("blur", clearPointer);
      window.removeEventListener("resize", refreshBounds);
      window.removeEventListener("scroll", scroll, true);
      document.removeEventListener("visibilitychange", visibility);
      media.removeEventListener("change", sync);
    },
  };
}
