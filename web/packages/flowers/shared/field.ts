import { arc, randomSource, reach, svgNumber, type Pose } from "./geometry";
import { motionConstants as constants, type SceneSettings, type FlowerSettings } from "./settings";

export interface Plant {
  x: number; y: number; scale: number; depth: number; length: number; headSize: number; thickness: number;
  restBend: number; phase: number; secondPhase: number; frequency: number; secondFrequency: number;
  amplitude: number; windResponse: number; leafPositions: number[]; leafScales: number[]; leafAngles: number[];
  homeX: number; homeY: number; pushX: number; pushY: number;
}
export interface Pointer { active: boolean; x: number; y: number; skyX: number; skyY: number }

export function pose(plant: Plant, time: number, settings: SceneSettings): Pose {
  const t = time * settings.windSpeed / 100;
  const sharedWind = Math.sin(t * .00037) * .065 + Math.sin(t * .00083 + 1.7) * .025;
  const envelope = .75 + .25 * Math.sin(t * .00013 + plant.secondPhase);
  const individual = Math.sin(t * plant.frequency + plant.phase) * .72 + Math.sin(t * plant.secondFrequency + plant.secondPhase) * .28;
  const bend = plant.restBend + (plant.amplitude * envelope * individual + sharedWind * plant.windResponse) * settings.windStrength / 100;
  const tip = arc(plant.length, bend);
  if (plant.pushX || plant.pushY) return reach(tip.x + plant.pushX / plant.scale, tip.y + plant.pushY / plant.scale);
  return { length: plant.length, bend, angle: bend / constants.radians, tip };
}

export function createField(width: number, height: number, settings: SceneSettings): Plant[] {
  const random = randomSource(settings.seed), between = (a: number, b: number) => a + (b - a) * random();
  const plants: Plant[] = [];
  for (let i = 0; i < settings.flowerCount; i++) {
    const depth = (i % 4 + random()) / 4;
    const fraction = (i + .5 + between(-.22, .22)) / settings.flowerCount;
    const scale = between(1.185 * (1 - settings.sizeVariation / 100), 1.185 * (1 + settings.sizeVariation / 100));
    plants.push({
      depth, x: width * fraction, y: height * (1.105 + .045 * depth) + settings.fieldOffset, scale,
      pushX: 0, pushY: 0, homeX: 0, homeY: 0,
      length: between(180, 232), headSize: between(99, 120) * settings.bloomScale / 100,
      thickness: between(settings.stemWidth * .9, settings.stemWidth * 1.1), restBend: between(-.13, .15),
      phase: between(0, Math.PI * 2), secondPhase: between(0, Math.PI * 2),
      frequency: between(.00055, .00135), secondFrequency: between(.00021, .00062),
      amplitude: between(.065, .14), windResponse: between(.25, 1),
      leafPositions: [between(.22, .35), between(.48, .62)],
      leafScales: [between(1.3, 1.55) * settings.leafScale / 100, between(1.3, 1.55) * settings.leafScale / 100],
      leafAngles: [between(-settings.leafAngle * .9, settings.leafAngle), between(-settings.leafAngle, settings.leafAngle * .9)],
    });
    // The study also sampled its rigid-sway comparison here. Preserve its seed sequence.
    between(7800, 15100); between(1.1, 2.8);
  }
  const layoutRandom = randomSource(734191 ^ settings.seed ^ 42761), top = height * .575, bottom = height * .805;
  const points = plants.map(f => ({ f, rootX: f.x, x: f.x, y: top + layoutRandom() * (bottom - top), top: top + Math.sin(f.x * .008) * 12 + (layoutRandom() - .5) * 16, bottom: bottom + Math.sin(f.x * .006 + 1.2) * 9 + (layoutRandom() - .5) * 14, r: f.headSize * f.scale * .49 }));
  for (let pass = 0; pass < 140; pass++) {
    const cooling = 1 - pass / 180;
    for (let i = 0; i < points.length; i++) for (let j = i + 1; j < points.length; j++) {
      const a = points[i], b = points[j], dx = b.x - a.x, dy = b.y - a.y;
      const distance = Math.hypot(dx, dy) || .001, clearance = (a.r + b.r) * (1 - settings.petalOverlap / 100) + 5;
      if (distance >= clearance) continue;
      const push = (clearance - distance) * .32 * cooling;
      a.x -= dx / distance * push; b.x += dx / distance * push;
      a.y -= dy / distance * push; b.y += dy / distance * push;
    }
    for (const p of points) {
      p.x += (p.rootX - p.x) * .004;
      p.x = Math.max(-p.r * .12, Math.min(width + p.r * .12, Math.max(p.rootX - 78, Math.min(p.rootX + 78, p.x))));
      p.y = Math.max(p.top, Math.min(p.bottom, p.y));
    }
  }
  for (const p of points) {
    const f = p.f, middle = (top + bottom) / 2;
    p.y = middle + (p.y - middle) * (1 - settings.packing / 100);
    f.depth = Math.max(0, Math.min(1, (p.y - top) / (bottom - top)));
    f.headSize *= 1 + .12 * Math.sin(Math.PI * f.depth);
    f.y = height * (1.105 + .045 * f.depth) + settings.fieldOffset;
    const solved = reach((p.x - f.x) / f.scale, (p.y - f.y) / f.scale);
    f.length = solved.length;
    const initial = pose(f, 0, settings);
    f.restBend += solved.bend - initial.bend;
    const at = pose(f, 0, settings);
    f.homeX = f.x + at.tip.x * f.scale; f.homeY = f.y + at.tip.y * f.scale;
  }
  return plants.sort((a, b) => a.y - b.y);
}

export function createSingle(flower: FlowerSettings, settings: SceneSettings): Plant[] {
  const [plant] = createField(constants.initialWidth, constants.initialHeight, { ...settings, flowerCount: 1 });
  const s = flower.stem, l = flower.leaves;
  const radius = flower.petals.centerRadius + flower.petals.innerLength + flower.petals.outerLength;
  plant.x = 0; plant.y = constants.singleRootY; plant.scale = 1;
  plant.length = radius + s.length; plant.headSize = radius * 2; plant.thickness = s.thickness;
  plant.restBend = Math.min(s.length / s.arcRadius, s.sweepAngle * constants.radians) * (s.direction === "right" ? 1 : -1) + s.startAngle * constants.radians;
  plant.leafPositions = [l.verticalOffset - l.separation / 2, l.verticalOffset + l.separation / 2].map(distance => Math.max(.04, Math.min(.96, (s.length - distance) / plant.length)));
  plant.leafAngles = [l.leftAngle - 45, l.rightAngle - 45]; plant.leafScales = [1, 1];
  const at = pose(plant, 0, settings);
  plant.homeX = plant.x + at.tip.x; plant.homeY = plant.y + at.tip.y;
  return [plant];
}

export function groundPath(plants: Plant[], width: number, height: number, settings: SceneSettings): { d: string; horizon: number } {
  let d = "", horizon = height;
  for (let x = -16; x <= width + 16; x += 12) {
    let y = height * .72;
    for (const f of plants) {
      const r = f.headSize * f.scale * .49, dx = (x - f.homeX) / (r * 1.35);
      if (Math.abs(dx) < 1) y = Math.min(y, f.homeY - r * .94 * Math.sqrt(1 - dx * dx) + settings.hillDrop);
    }
    horizon = Math.min(horizon, y); d += `${d ? " L" : "M"} ${svgNumber(x)} ${svgNumber(y)}`;
  }
  return { d: `${d} L ${width + 16} ${height} L -16 ${height} Z`, horizon };
}

export function updateInteraction(plants: Plant[], pointer: Pointer, dt: number, width: number, height: number, settings: SceneSettings): void {
  const mix = 1 - Math.exp(-dt / (settings.responseTime * (pointer.active ? 1 : constants.returnEase)));
  for (const f of plants) {
    let x = 0, y = 0;
    if (pointer.active) {
      const dx = f.homeX - pointer.x, dy = f.homeY - pointer.y, distance = Math.hypot(dx, dy);
      if (distance < settings.pointerRadius) {
        const falloff = (1 - distance / settings.pointerRadius) ** 2;
        const normal = Math.sqrt(dx * dx + dy * dy + constants.pointerSoftness ** 2);
        x = dx / normal * settings.flowerStrength * falloff;
        y = dy / normal * settings.flowerStrength * constants.verticalPush * falloff;
      }
    }
    f.pushX += (x - f.pushX) * mix; f.pushY += (y - f.pushY) * mix;
  }
  pointer.skyX += ((pointer.active ? (pointer.x / width - .5) * settings.skyStrength * 2 : 0) - pointer.skyX) * mix;
  pointer.skyY += ((pointer.active ? (pointer.y / height - .5) * settings.skyStrength * constants.skyVertical : 0) - pointer.skyY) * mix;
}
