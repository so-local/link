// The look every Local Link interface shares, written once in Tailwind's
// own words: which color, radius, shadow and blur each part is. The
// websites' classes (card.ts) spell these out for Tailwind, and an app
// that can't run Tailwind (a React Native one) styles itself from the
// same parts through `css`, so a card, a pill or a notice is the same to
// the pixel on both. look.test.mts holds every value here to Tailwind's
// theme and every recipe to its classes in card.ts.
//
// It imports nothing, so any app can read it.

/** Tailwind's colors this look uses, in sRGB: Tailwind writes them in OKLCH, which React Native can't read. */
export const palette = {
  white: "#ffffff",
  black: "#000000",
  "zinc-100": "#f4f4f5",
  "zinc-200": "#e4e4e7",
  "zinc-400": "#9f9fa9",
  "zinc-500": "#71717b",
  "zinc-600": "#52525c",
  "zinc-700": "#3f3f46",
  "zinc-800": "#27272a",
  "zinc-900": "#18181b",
  "emerald-50": "#ecfdf5",
  "emerald-200": "#a4f4cf",
  "emerald-900": "#004f3b",
  "red-50": "#fef2f2",
  "red-200": "#ffc9c9",
  "red-800": "#9f0712",
  // The sunflower's petals, flowers' own (`flowerDefaults.petals`): the
  // inner yellow and the darker outer one.
  sunflower: "#f5bf2b",
  "sunflower-dark": "#d9962b",
} as const;
export type Color = keyof typeof palette;

/** A color, or a color at an opacity as Tailwind's `bg-white/90` says it. */
export type Paint = Color | readonly [Color, number];

/**
 * The two sizes of type, Tailwind's: a card's heading, and everything
 * else. Each size in pixels and its line height. No button, pill, tab,
 * notice or note has a size of its own.
 */
export const text = {
  base: { size: 16, line: 24 },
  "2xl": { size: 24, line: 32 },
} as const;
export type Size = keyof typeof text;

/** Font weights by Tailwind's names. */
export const weight = { normal: 400, medium: 500, semibold: 600 } as const;
export type Weight = keyof typeof weight;

/** Letter spacing, in ems: headings are tight. */
export const tracking = { tight: -0.025 } as const;

/** Corner radii in pixels: an inset, a card, and a pill's round ends. */
export const radius = { xl: 12, "2xl": 16, full: 9999 } as const;

/** Tailwind's shadows, each in its default black at its default opacity. */
export const shadow = {
  sm: [
    [0, 1, 3, 0],
    [0, 1, 2, -1],
  ],
  md: [
    [0, 4, 6, -1],
    [0, 2, 4, -2],
  ],
} as const;
export const shadowOpacity = 0.1;

/** Background blurs in pixels. */
export const blur = { md: 12, xl: 24 } as const;

/** A surface: what's under its text, its one-pixel ring, its shadow (and the shadow's color, when not the default) and its blur. */
export type Surface = {
  fill: Paint;
  ink: Color;
  radius: keyof typeof radius;
  ring?: Paint;
  shadow?: keyof typeof shadow;
  shadowColor?: Paint;
  blur?: keyof typeof blur;
};

/** Frosted white cards: nearly solid and strongly blurred, so text reads over the flowers. */
export const frost: Surface = { fill: ["white", 0.9], ink: "zinc-900", radius: "2xl", ring: ["black", 0.05], shadow: "sm", blur: "xl" };

/** White pills: tabs and buttons, solid white with darker text on hover. */
export const pill: Surface = { fill: ["white", 0.8], ink: "zinc-700", radius: "full", ring: ["black", 0.05], shadow: "md", shadowColor: ["black", 0.15], blur: "md" };
export const pillHover: Pick<Surface, "fill" | "ink"> = { fill: "white", ink: "zinc-900" };

/** The dark pill: the tab you're on and a form's main button. */
export const current: Surface = { fill: "zinc-900", ink: "white", radius: "full", ring: "zinc-900", shadow: "md", shadowColor: ["black", 0.15] };
export const currentHover: Pick<Surface, "fill"> = { fill: "zinc-700" };

/** A box inside a card: an item in its list, or a pick among several, its ring dark when picked. */
export const inset: Surface = { fill: ["white", 0.7], ink: "zinc-900", radius: "xl", ring: ["zinc-900", 0.1] };
export const insetPicked: Surface = { ...inset, ring: "zinc-900" };
export const insetRoom = 16;

/** How faint a control is while it can't be used. */
export const disabled = 0.6;

/** A field to type in: white, ringed, a pill on one line and a card's corners when it takes several. */
export const field: Surface = { fill: "white", ink: "zinc-900", radius: "full", ring: ["zinc-900", 0.15] };
export const fieldLines: Surface = { ...field, radius: "2xl" };
export const fieldRoom = { size: "base", padX: 16, padY: 10 } as const;

/**
 * The one yellow: the sunflower's, see-through, so a tag in it reads as
 * a label over the card, never a button. What's on offer and something
 * to see to both wear it, so a page never shows two yellows.
 */
const sunflower = { fill: ["sunflower", 0.3], ink: "zinc-900", ring: ["sunflower-dark", 0.5] } as const;

/**
 * The tones a tag takes, and the only colors besides the zinc greys:
 * what's on offer (`offer`), news (`note`), something that went well
 * (`good`), something to see to (`warn`) and something that failed
 * (`bad`).
 */
export const tones = {
  offer: sunflower,
  note: { fill: "zinc-100", ink: "zinc-800", ring: "zinc-200" },
  good: { fill: "emerald-50", ink: "emerald-900", ring: "emerald-200" },
  warn: sunflower,
  bad: { fill: "red-50", ink: "red-800", ring: "red-200" },
} as const satisfies Record<string, { fill: Paint; ink: Color; ring: Paint }>;
export type Tone = keyof typeof tones;

/**
 * Two kinds of pill, by what a thing does. What you press, a button or
 * a tab, is raised and solid (`shape`). What you read, a tag in a tone
 * (what's on sale, what a form answered), is flat, see-through and
 * snugger (`tag`), so it never looks pressable. Both at the copy's
 * size, medium; room in pixels.
 */
export const shape = { size: "base", weight: "medium", padX: 20, padY: 10 } as const;
export const tag = { size: "base", weight: "medium", padX: 12, padY: 4, radius: "full" } as const;

/** Text by its part: a card's heading, its copy, a quieter line, and a term over its value. */
export const words = {
  heading: { size: "2xl", weight: "semibold", ink: "zinc-900", tight: true },
  subheading: { size: "base", weight: "semibold", ink: "zinc-900", tight: false },
  copy: { size: "base", weight: "normal", ink: "zinc-700", tight: false },
  quiet: { size: "base", weight: "normal", ink: "zinc-600", tight: false },
  term: { size: "base", weight: "normal", ink: "zinc-500", tight: false },
  label: { size: "base", weight: "normal", ink: "zinc-900", tight: false },
} as const satisfies Record<string, { size: Size; weight: Weight; ink: Color; tight: boolean }>;

/**
 * The column every page sits in, under the wordmark, and a card's
 * padding. Narrow, the column is at most `column` pixels with a `gutter`
 * each side, wider from `wide` pixels up (Tailwind's `sm`). From `three`
 * pixels up (Tailwind's `lg`) it is at most `most` pixels with a
 * `gutterThree` each side, holding three columns `gapThree` apart: a side
 * column each side, `side.share` of the screen's width between
 * `side.least` and `side.most` pixels, and the cards between them, at most
 * `column` pixels.
 */
export const layout = {
  column: 768, wide: 640, gutter: 16, gutterWide: 40, top: 16, bottom: 24, bottomWide: 40, gap: 16, card: 24, cardWide: 32,
  three: 1024, most: 1504, gutterThree: 24, gapThree: 24, side: { least: 160, share: 0.18, most: 256 },
} as const;

/** How wide the cards are: the screen's width and the page's (the screen's less any scrollbar). */
export function cardsWidth(screen: number, page: number): number {
  const l = layout;
  if (screen < l.three) return Math.min(l.column, page) - 2 * (screen < l.wide ? l.gutter : l.gutterWide);
  const side = Math.min(l.side.most, Math.max(l.side.least, screen * l.side.share));
  return Math.min(l.column, Math.min(page, l.most) - 2 * l.gutterThree - 2 * side - 2 * l.gapThree);
}

/**
 * The wordmark above every page: its type grows with the screen, 3% of
 * its width, from `least` to `most` pixels, with `top` pixels above it
 * and `below` ems under it.
 */
export const brand = { name: "local.link", least: 26, most: 32, share: 0.03, line: 1.2, tracking: -0.055, weight: "semibold", ink: "zinc-900", flower: 1.125, gap: 8, top: 24, below: 0.225 } as const;

/** A paint as CSS: a hex color, or `rgba()` at its opacity. */
export function paint(p: Paint): string {
  if (typeof p === "string") return palette[p];
  const [color, opacity] = p;
  const hex = palette[color];
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
  return `rgba(${r}, ${g}, ${b}, ${opacity})`;
}

/** A surface's box shadow as CSS, its ring first, as Tailwind stacks them. */
export function boxShadow(s: Surface): string | undefined {
  const layers: string[] = [];
  if (s.ring) layers.push(`0 0 0 1px ${paint(s.ring)}`);
  if (s.shadow) {
    const color = paint(s.shadowColor ?? ["black", shadowOpacity]);
    for (const [x, y, b, spread] of shadow[s.shadow]) layers.push(`${x}px ${y}px ${b}px ${spread}px ${color}`);
  }
  return layers.length ? layers.join(", ") : undefined;
}

/** A surface as plain style values any renderer reads. */
export function css(s: Surface) {
  return {
    backgroundColor: paint(s.fill),
    color: paint(s.ink),
    borderRadius: radius[s.radius],
    boxShadow: boxShadow(s),
    backdropFilter: s.blur ? `blur(${blur[s.blur]}px)` : undefined,
  };
}
