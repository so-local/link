"use client";

import { useEffect, useId, useMemo, useRef, useState, type CSSProperties, type RefObject } from "react";
import { arc, arcPath, createField, createSingle, groundPath, headRadius, leafPath, petalPaths, pose, resolveFlower, resolveScene, svgNumber as n, type FlowerOptions, type SceneSettings } from "@link-flowers/shared";
import { motionConstants as constants } from "@link-flowers/shared/settings";
import { animateScene } from "./controller";

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
  const petals = useMemo(() => petalPaths(flower), [flower]);
  const radius = headRadius(flower), headOnly = single && (framing === "head" || !flower.stem.enabled);
  const overscan = settings.skyStrength + constants.skyDriftX + 45;
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
    <defs>
      <g id={`${id}-head`}>
        {petals.map((p, i) => <path key={i} d={p.d} fill={p.fill} />)}
        <circle r={flower.petals.centerRadius} fill={flower.center.color} stroke={flower.center.outlineColor} strokeWidth={flower.center.outlineWidth} />
      </g>
      {[0, 1].map(side => <path key={side} id={`${id}-leaf-${side}`} d={leafPath(flower, side)} fill={flower.leaves.color} />)}
      <linearGradient id={`${id}-sky`} x2="0" y2="1"><stop stopColor={constants.skyTop} /><stop offset="1" stopColor={constants.skyBottom} /></linearGradient>
      <radialGradient id={`${id}-cloud`}><stop stopColor="white" stopOpacity=".93" /><stop offset="1" stopColor="white" stopOpacity="0" /></radialGradient>
      <linearGradient id={`${id}-ground`} gradientUnits="userSpaceOnUse" x1="0" x2="0" y1={n(ground.horizon)} y2={size.height}>
        <stop stopColor={constants.groundTop} /><stop offset=".48" stopColor={constants.groundMiddle} /><stop offset="1" stopColor={flower.stem.color} />
      </linearGradient>
    </defs>
    {!single && <>
      <g data-sky="">
        <rect x={-overscan} y={-overscan} width={size.width + overscan * 2} height={size.height + overscan * 2} fill={`url(#${id}-sky)`} />
        <ellipse cx={size.width * .25} cy={size.height * .35} rx={size.width * .49} ry={size.height * .6} fill={`url(#${id}-cloud)`} />
        <ellipse cx={size.width * .8} cy={size.height * .6} rx={size.width * .52} ry={size.height * .65} fill={`url(#${id}-cloud)`} />
      </g>
      <path data-ground="" d={ground.d} fill={`url(#${id}-ground)`} />
    </>}
    {single && !flower.canvas.transparent && <rect x="-10000" y="-10000" width="20000" height="20000" fill={flower.canvas.background} />}
    {plants.map((f, i) => {
      const at = pose(f, 0, settings), ratio = f.headSize / (radius * 2);
      return <g key={i} data-plant="" transform={headOnly ? undefined : `translate(${n(f.x)} ${n(f.y)}) scale(${n(f.scale)})`}>
        <path data-stem="" d={arcPath(at.length, at.bend)} fill="none" stroke={flower.stem.color} strokeWidth={f.thickness} strokeLinecap="round" display={headOnly || !flower.stem.enabled ? "none" : undefined} />
        {flower.leaves.enabled && [0, 1].map(j => {
          const u = f.leafPositions[j], p = arc(at.length, at.bend, u);
          return <use key={j} data-leaf="" href={`#${id}-leaf-${j}`} display={headOnly ? "none" : undefined} transform={`translate(${n(p.x)} ${n(p.y)}) rotate(${n(at.bend * u / constants.radians)}) scale(${j ? -1 : 1} 1) rotate(${n(f.leafAngles[j])}) scale(${n(f.leafScales[j])})`} />;
        })}
        <g data-head="" transform={headOnly ? undefined : `translate(${n(at.tip.x)} ${n(at.tip.y)}) rotate(${n(at.angle)})`}>
          <use href={`#${id}-head`} transform={`scale(${n(ratio)})`} />
        </g>
      </g>;
    })}
  </svg>;
}
