export interface FlowerSettings {
  petals: {
    arrangement: "layered" | "junction";
    countPerRing: number;
    centerRadius: number;
    innerLength: number;
    outerLength: number;
    innerWidth: number;
    outerWidth: number;
    curvature: number;
    rotation: number;
    overlap: number;
    innerColor: string;
    outerColor: string;
  };
  center: { color: string; outlineWidth: number; outlineColor: string };
  stem: {
    enabled: boolean;
    arcRadius: number;
    sweepAngle: number;
    startAngle: number;
    length: number;
    thickness: number;
    direction: "left" | "right";
    color: string;
  };
  leaves: {
    enabled: boolean;
    leftLength: number;
    rightLength: number;
    leftWidth: number;
    rightWidth: number;
    leftAngle: number;
    rightAngle: number;
    curvature: number;
    verticalOffset: number;
    separation: number;
    color: string;
  };
  canvas: {
    flowerScale: number;
    offsetX: number;
    offsetY: number;
    padding: number;
    background: string;
    transparent: boolean;
  };
}

export type FlowerOptions = { [K in keyof FlowerSettings]?: Partial<FlowerSettings[K]> };

export interface SceneSettings {
  skyStrength: number;
  flowerStrength: number;
  pointerRadius: number;
  responseTime: number;
  flowerCount: number;
  bloomScale: number;
  sizeVariation: number;
  stemWidth: number;
  leafScale: number;
  leafAngle: number;
  windStrength: number;
  windSpeed: number;
  packing: number;
  petalOverlap: number;
  fieldOffset: number;
  hillDrop: number;
  seed: number;
}

export const flowerDefaults: FlowerSettings = {
  petals: { arrangement: "layered", countPerRing: 12, centerRadius: 52, innerLength: 57.5, outerLength: .5, innerWidth: 29, outerWidth: 29, curvature: .66, rotation: -105, overlap: .65, innerColor: "#F5BF2B", outerColor: "#D9962B" },
  center: { color: "#5F4429", outlineWidth: 0, outlineColor: "#472A11" },
  stem: { enabled: true, arcRadius: 190, sweepAngle: 24, startAngle: 0, length: 65, thickness: 7, direction: "right", color: "#398047" },
  leaves: { enabled: true, leftLength: 31, rightLength: 31, leftWidth: 13, rightWidth: 13, leftAngle: 45, rightAngle: 45, curvature: .5, verticalOffset: 30, separation: 13, color: "#4A9B4F" },
  canvas: { flowerScale: 1, offsetX: 0, offsetY: 0, padding: 2, background: "#FFFFFF", transparent: true },
};

export const sceneDefaults: SceneSettings = {
  skyStrength: 30, flowerStrength: 40, pointerRadius: 400, responseTime: 340,
  flowerCount: 36, bloomScale: 100, sizeVariation: 10, stemWidth: 13,
  leafScale: 100, leafAngle: 38, windStrength: 100, windSpeed: 100,
  packing: 35, petalOverlap: 31, fieldOffset: 0, hillDrop: 0, seed: 42761,
};

export const sceneBounds: Record<keyof SceneSettings, readonly [number, number]> = {
  skyStrength: [0, 200], flowerStrength: [0, 240], pointerRadius: [50, 1600], responseTime: [40, 3000],
  flowerCount: [1, 96], bloomScale: [50, 180], sizeVariation: [0, 40], stemWidth: [3, 30],
  leafScale: [30, 250], leafAngle: [0, 90], windStrength: [0, 300], windSpeed: [0, 300],
  packing: [0, 75], petalOverlap: [5, 65], fieldOffset: [-100, 180], hillDrop: [0, 150], seed: [0, 4294967295],
};

/** Inputs are props, but invalid numbers must never reach SVG geometry. */
export function resolveScene(options: Partial<SceneSettings> = {}): SceneSettings {
  const result = { ...sceneDefaults, ...options };
  for (const key of Object.keys(sceneDefaults) as (keyof SceneSettings)[]) {
    const [min, max] = sceneBounds[key], value = result[key];
    if (!Number.isFinite(value) || value < min || value > max) throw new RangeError(`Invalid flower scene setting: ${key}`);
  }
  if (!Number.isInteger(result.flowerCount) || !Number.isInteger(result.seed)) throw new RangeError("Flower count and seed must be integers");
  return result;
}

export function resolveFlower(options: FlowerOptions = {}): FlowerSettings {
  const result = Object.fromEntries(Object.entries(flowerDefaults).map(([key, defaults]) => [key, { ...defaults, ...options[key as keyof FlowerSettings] }])) as unknown as FlowerSettings;
  for (const group of Object.values(result)) for (const [key, value] of Object.entries(group)) {
    if (typeof value === "number" && (!Number.isFinite(value) || Math.abs(value) > 10000)) throw new RangeError(`Invalid flower setting: ${key}`);
    if (typeof value === "string" && /color|background/i.test(key) && !/^#[\da-f]{6}$/i.test(value)) throw new RangeError(`Invalid flower color: ${key}`);
  }
  const p = result.petals, s = result.stem, l = result.leaves;
  if (!Number.isInteger(p.countPerRing) || p.countPerRing < 1 || p.countPerRing > 64 || p.centerRadius <= 0 || p.innerLength < 0 || p.outerLength < 0 || p.innerWidth <= 0 || p.outerWidth <= 0 || s.length < 0 || s.arcRadius <= 0 || s.thickness <= 0 || l.leftLength <= 0 || l.rightLength <= 0 || l.leftWidth <= 0 || l.rightWidth <= 0 || result.canvas.flowerScale <= 0) throw new RangeError("Invalid flower dimensions");
  if (!["layered", "junction"].includes(p.arrangement) || !["left", "right"].includes(s.direction)) throw new RangeError("Invalid flower shape");
  return result;
}

export const motionConstants = {
  radians: Math.PI / 180, straight: .00001, initialWidth: 1280, initialHeight: 590,
  maxFrame: 50, returnEase: 1.53, pointerSoftness: 28, verticalPush: 8 / 18,
  skyVertical: 1.25, skyPeriod: 54000, skyDriftX: 28, skyDriftY: 12,
  skyTop: "#6db9ed", skyBottom: "#e2f2ff", groundTop: "#8EBD79", groundMiddle: "#62A45B",
  singleRootY: 105,
  // A background comes to rest over settleTime ms once the page scrolls
  // past scrollRest pixels, and starts again as gently back at the top.
  settleTime: 1200, scrollRest: 0,
  // A field fades in over revealTime ms once it has its real size.
  revealTime: 500,
} as const;
