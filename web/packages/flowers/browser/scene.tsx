"use client";

import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type RefObject } from "react";
import { createField, createSingle, groundPath, headRadius, pose, resolveFlower, resolveScene, svgNumber as n, type FlowerOptions, type SceneSettings } from "@link-flowers/shared";
import { motionConstants as constants } from "@link-flowers/shared/settings";
import { animateScene } from "./controller";
import { SceneArt } from "./art";

export interface FlowerProps {
  flower?: FlowerOptions;
  settings?: Partial<SceneSettings>;
  paused?: boolean;
  interactive?: boolean;
  pointerTarget?: RefObject<Element | null>;
  className?: string;
  style?: CSSProperties;
  title?: string;
}
export interface SingleFlowerProps extends FlowerProps { framing?: "head" | "full" }
export interface BackgroundProps extends FlowerProps { fixed?: boolean }

export function FlowerScene({ flower: flowerOptions, settings: sceneOptions, paused = false, interactive = true, pointerTarget, className, style, title, single = false, framing = "full", fixed = false }: SingleFlowerProps & { single?: boolean; fixed?: boolean }) {
  const flowerKey = JSON.stringify(flowerOptions ?? {}), sceneKey = JSON.stringify(sceneOptions ?? {});
  const flower = useMemo(() => resolveFlower(JSON.parse(flowerKey) as FlowerOptions), [flowerKey]);
  const settings = useMemo(() => resolveScene(JSON.parse(sceneKey) as Partial<SceneSettings>), [sceneKey]);
  const id = `flower-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;
  const svg = useRef<SVGSVGElement>(null), clock = useRef({ time: 0 });
  const controller = useRef<ReturnType<typeof animateScene> | null>(null);
  const pausedRef = useRef(paused);
  const [size, setSize] = useState({ width: constants.initialWidth as number, height: constants.initialHeight as number });
  const plants = useMemo(() => single ? createSingle(flower, settings) : createField(size.width, size.height, settings), [single, flower, settings, size]);
  const ground = useMemo(() => groundPath(plants, size.width, size.height, settings), [plants, size, settings]);
  const radius = headRadius(flower), headOnly = single && (framing === "head" || !flower.stem.enabled);
  const singleViewBox = useMemo(() => {
    if (headOnly) {
      const width = (radius + flower.canvas.padding) * 2 / Math.min(1, flower.canvas.flowerScale);
      return `${n(-width / 2 - flower.canvas.offsetX)} ${n(-width / 2 - flower.canvas.offsetY)} ${n(width)} ${n(width)}`;
    }
    const f = plants[0], tip = pose(f, 0, settings).tip;
    const motionRoom = f.length * .15 * settings.windStrength / 100;
    const padding = flower.canvas.padding + motionRoom;
    const left = Math.min(-f.length * .3, tip.x - radius) - padding;
    const right = Math.max(f.length * .3, tip.x + radius) + padding;
    const top = f.y + tip.y - radius - padding, bottom = f.y + padding;
    const zoom = Math.min(1, flower.canvas.flowerScale), width = (right - left) / zoom, height = (bottom - top) / zoom;
    return `${n((left + right - width) / 2 - flower.canvas.offsetX)} ${n((top + bottom - height) / 2 - flower.canvas.offsetY)} ${n(width)} ${n(height)}`;
  }, [headOnly, radius, plants, settings, flower]);

  // The server can't know the window, so a field's first drawing is at the
  // initial size, hidden. Before the browser paints after hydrating, the
  // field takes its real size, then fades in once, so no flower is ever
  // seen stretched or jumping to its place.
  const [shown, setShown] = useState(single);
  useLayoutEffect(() => {
    if (single) return;
    const { width, height } = svg.current!.getBoundingClientRect();
    if (width > 0 && height > 0) setSize(old => old.width === width && old.height === height ? old : { width, height });
  }, [single]);
  useEffect(() => {
    if (single) return;
    const frame = requestAnimationFrame(() => setShown(true));
    return () => cancelAnimationFrame(frame);
  }, [single]);

  useEffect(() => {
    if (single) return;
    const observer = new ResizeObserver(entries => {
      const { width, height } = entries[0].contentRect;
      if (width > 0 && height > 0) setSize(old => old.width === width && old.height === height ? old : { width, height });
    });
    observer.observe(svg.current!);
    return () => observer.disconnect();
  }, [single]);

  useEffect(() => {
    if (headOnly) return;
    const element = svg.current!;
    controller.current = animateScene(element, plants, settings, clock.current, interactive && !headOnly, pointerTarget?.current ?? (fixed ? document.documentElement : element), pausedRef.current, fixed);
    return () => { controller.current?.dispose(); controller.current = null; };
  }, [plants, settings, interactive, pointerTarget, fixed, headOnly]);

  useEffect(() => { pausedRef.current = paused; controller.current?.setPaused(paused); }, [paused]);

  return <svg ref={svg} xmlns="http://www.w3.org/2000/svg" data-flower-scene={single ? "single" : "field"} className={className}
    viewBox={single ? singleViewBox : `0 0 ${size.width} ${size.height}`} preserveAspectRatio={single ? "xMidYMid meet" : "none"} role={title ? "img" : undefined} aria-labelledby={title ? `${id}-title` : undefined} aria-hidden={title ? undefined : true}
    style={{ display: "block", width: "100%", height: "100%", overflow: "hidden", ...(fixed ? { position: "fixed", top: 0, left: 0, height: "100lvh", zIndex: -10, pointerEvents: "none" } : {}), ...style }}>
    {title && <title id={`${id}-title`}>{title}</title>}
    <SceneArt id={id} flower={flower} settings={settings} plants={plants} ground={ground} width={size.width} height={size.height} single={single} headOnly={headOnly}
      fieldStyle={single ? undefined : { opacity: shown ? 1 : 0, transition: `opacity ${constants.revealTime}ms ease-out` }} />
  </svg>;
}
