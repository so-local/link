// The scene's vector artwork at one moment, with no state and no
// effects: what FlowerScene draws and then animates in a browser, and
// what a server draws once (HeadArt, a share card's sunflower).

import type { CSSProperties } from "react";
import { arc, arcPath, headRadius, leafPath, petalPaths, pose, resolveFlower, svgNumber as n, type FlowerSettings, type Plant, type SceneSettings } from "@link-flowers/shared";
import { motionConstants as constants } from "@link-flowers/shared/settings";

export interface SceneArtProps {
  id: string;
  flower: FlowerSettings;
  settings: SceneSettings;
  plants: Plant[];
  ground: { d: string; horizon: number };
  width: number;
  height: number;
  single: boolean;
  headOnly: boolean;
  /** The ground's and the plants' style: FlowerScene hides a field's until it has measured its size, then fades them in. */
  fieldStyle?: CSSProperties;
}

/**
 * The scene's definitions, its sky and ground for a field, and each plant
 * at rest: a list of plain SVG elements, no fragment or component among
 * them, since a share card's renderer takes nothing else inside an <svg>.
 */
export function SceneArt({ id, flower, settings, plants, ground, width, height, single, headOnly, fieldStyle }: SceneArtProps) {
  const size = { width, height };
  const petals = petalPaths(flower);
  const radius = headRadius(flower);
  const overscan = settings.skyStrength + constants.skyDriftX + 45;
  return [
    <defs key="defs">
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
    </defs>,
    !single && <g key="sky" data-sky="">
        <rect x={-overscan} y={-overscan} width={size.width + overscan * 2} height={size.height + overscan * 2} fill={`url(#${id}-sky)`} />
        <ellipse cx={size.width * .25} cy={size.height * .35} rx={size.width * .49} ry={size.height * .6} fill={`url(#${id}-cloud)`} />
        <ellipse cx={size.width * .8} cy={size.height * .6} rx={size.width * .52} ry={size.height * .65} fill={`url(#${id}-cloud)`} />
      </g>,
    !single && <path key="ground" data-ground="" d={ground.d} fill={`url(#${id}-ground)`} style={fieldStyle} />,
    single && !flower.canvas.transparent && <rect key="canvas" x="-10000" y="-10000" width="20000" height="20000" fill={flower.canvas.background} />,
    <g key="plants" data-plants="" style={fieldStyle}>{plants.map((f, i) => {
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
    })}</g>,
  ];
}

/** One sunflower's head, `size` pixels square, at rest: a whole SVG a server can draw, as a share card does. */
export function HeadArt({ size }: { size: number }) {
  const flower = resolveFlower();
  const petals = petalPaths(flower);
  const half = headRadius(flower) + flower.canvas.padding;
  return <svg xmlns="http://www.w3.org/2000/svg" viewBox={`${n(-half)} ${n(-half)} ${n(half * 2)} ${n(half * 2)}`} width={size} height={size}>
    {petals.map((p, i) => <path key={i} d={p.d} fill={p.fill} />)}
    <circle r={flower.petals.centerRadius} fill={flower.center.color} stroke={flower.center.outlineColor} strokeWidth={flower.center.outlineWidth} />
  </svg>;
}
