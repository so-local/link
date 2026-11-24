import { motionConstants as constants, type FlowerSettings } from "./settings";

export interface Point { x: number; y: number }
export interface Pose { length: number; bend: number; angle: number; tip: Point }
/** Stable markup across server/browser implementations of trigonometric functions. */
export const svgNumber = (value: number): string => Number(value.toFixed(4)).toString();

export function arc(length: number, bend: number, u = 1): Point {
  if (u === 0) return { x: 0, y: 0 };
  if (Math.abs(bend) < constants.straight) return { x: 0, y: -length * u };
  return { x: length * (1 - Math.cos(bend * u)) / bend, y: -length * Math.sin(bend * u) / bend };
}

export function arcPath(length: number, bend: number): string {
  const tip = arc(length, bend);
  if (Math.abs(bend) < constants.straight) return `M 0 0 L 0 ${svgNumber(-length)}`;
  const radius = Math.abs(length / bend);
  return `M 0 0 A ${svgNumber(radius)} ${svgNumber(radius)} 0 ${Math.abs(bend) > Math.PI ? 1 : 0} ${bend > 0 ? 1 : 0} ${svgNumber(tip.x)} ${svgNumber(tip.y)}`;
}

export function reach(x: number, y: number): Pose {
  const up = Math.max(1, -y), bend = 2 * Math.atan2(x, up);
  const length = Math.abs(bend) < constants.straight ? up : up * bend / Math.sin(bend);
  return { length, bend, angle: bend / constants.radians, tip: arc(length, bend) };
}

function lanceolate(base: Point, tip: Point, width: number, curvature: number): string {
  const dx = tip.x - base.x, dy = tip.y - base.y, length = Math.hypot(dx, dy) || 1;
  const ux = dx / length, uy = dy / length, wing = Math.max(.1, width) * 2 / 3;
  const q = .12 + .34 * Math.max(0, Math.min(1, curvature));
  const control = (u: number, side: number) => `${svgNumber(base.x + dx * u - uy * wing * side)} ${svgNumber(base.y + dy * u + ux * wing * side)}`;
  return `M ${svgNumber(base.x)} ${svgNumber(base.y)} C ${control(q, 1)} ${control(1 - q, 1)} ${svgNumber(tip.x)} ${svgNumber(tip.y)} C ${control(1 - q, -1)} ${control(q, -1)} ${svgNumber(base.x)} ${svgNumber(base.y)} Z`;
}

export function headRadius(flower: FlowerSettings): number {
  return flower.petals.centerRadius + flower.petals.innerLength + flower.petals.outerLength;
}

export function petalPaths(flower: FlowerSettings): { d: string; fill: string }[] {
  const p = flower.petals, inner = p.centerRadius + p.innerLength, outer = headRadius(flower);
  const layered = p.arrangement === "layered", base = layered ? p.centerRadius * (1 - p.overlap) : inner;
  const result: { d: string; fill: string }[] = [];
  for (const ring of [0, 1]) for (let i = 0; i < p.countPerRing; i++) {
    const angle = p.rotation * constants.radians + (i + ring * .5) * Math.PI * 2 / p.countPerRing;
    const start = ring ? (layered ? base : p.centerRadius) : base;
    const end = ring ? (layered ? outer : inner) : (layered ? inner : outer);
    result.push({ d: lanceolate({ x: start * Math.cos(angle), y: start * Math.sin(angle) }, { x: end * Math.cos(angle), y: end * Math.sin(angle) }, ring ? p.innerWidth : p.outerWidth, p.curvature), fill: ring ? p.innerColor : p.outerColor });
  }
  return result;
}

export function leafPath(flower: FlowerSettings, side: number): string {
  const l = flower.leaves, length = side ? l.rightLength : l.leftLength, width = side ? l.rightWidth : l.leftWidth;
  // Preserve the motion study's leaf silhouette; scale its dimensions through settings.
  const sx = width / 13, sy = length / 31;
  if (l.curvature === .5) return `M 0 0 C ${-12 * sx} ${-sy} ${-32 * sx} ${-15 * sy} ${-33 * sx} ${-31 * sy} C ${-14 * sx} ${-30 * sy} ${-sx} ${-16 * sy} 0 0Z`;
  return lanceolate({ x: 0, y: 0 }, { x: -length, y: -length }, width, l.curvature);
}

export function randomSource(seed: number): () => number {
  return () => {
    seed |= 0; seed = seed + 0x6D2B79F5 | 0;
    let n = Math.imul(seed ^ seed >>> 15, 1 | seed);
    n ^= n + Math.imul(n ^ n >>> 7, 61 | n);
    return ((n ^ n >>> 14) >>> 0) / 4294967296;
  };
}
