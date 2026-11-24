// The look's values (look.ts) are Tailwind's theme, and the websites'
// classes (card.ts) say each of its recipes, so an app styled from the
// values looks as the websites do.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import * as look from "./look.ts";
import * as card from "./card.ts";
import { flowerDefaults } from "@link-flowers/shared/settings";

const theme = readFileSync(createRequire(import.meta.url).resolve("tailwindcss/theme.css"), "utf8");

/** A variable of Tailwind's theme, as written. */
function variable(name: string): string {
  const found = theme.match(new RegExp(`--${name}:\\s*([^;]+);`));
  assert.ok(found, `Tailwind's theme has --${name}`);
  return found[1].trim();
}

/** OKLCH, as Tailwind writes its colors, in sRGB hex, as a browser paints it. */
function hex(oklch: string): string {
  const [l, c, h] = oklch.replace(/^oklch\(|\)$/g, "").split(/\s+/).map((v) => (v === "none" ? 0 : parseFloat(v)));
  const L = l / 100, a = c * Math.cos((h * Math.PI) / 180), b = c * Math.sin((h * Math.PI) / 180);
  const lms = [L + 0.3963377774 * a + 0.2158037573 * b, L - 0.1055613458 * a - 0.0638541728 * b, L - 0.0894841775 * a - 1.291485548 * b].map((v) => v ** 3);
  const rgb = [
    4.0767416621 * lms[0] - 3.3077115913 * lms[1] + 0.2309699292 * lms[2],
    -1.2684380046 * lms[0] + 2.6097574011 * lms[1] - 0.3413193965 * lms[2],
    -0.0041960863 * lms[0] - 0.7034186147 * lms[1] + 1.707614701 * lms[2],
  ];
  return `#${rgb
    .map((x) => Math.min(1, Math.max(0, x)))
    .map((x) => (x <= 0.0031308 ? 12.92 * x : 1.055 * x ** (1 / 2.4) - 0.055))
    .map((x) => Math.round(x * 255).toString(16).padStart(2, "0"))
    .join("")}`;
}

const rem = (v: string) => parseFloat(v) * 16;

/** The sunflower's colors: the flowers' own petals, and the site's theme says them for Tailwind. */
const sunflower = { sunflower: flowerDefaults.petals.innerColor, "sunflower-dark": flowerDefaults.petals.outerColor } as const;

test("the palette is Tailwind's colors, and the sunflower's", () => {
  const styles = readFileSync(new URL("./globals.css", import.meta.url), "utf8");
  for (const [name, value] of Object.entries(look.palette)) {
    if (name in sunflower) {
      assert.equal(value, sunflower[name as keyof typeof sunflower].toLowerCase(), `${name} is the flowers' petal`);
      assert.match(styles, new RegExp(`--color-${name}: ${value};`), `globals.css's theme has ${name}`);
      continue;
    }
    const want = name === "white" ? "#ffffff" : name === "black" ? "#000000" : hex(variable(`color-${name}`));
    assert.equal(value, want, name);
  }
});

test("sizes, radii, shadows and blurs are Tailwind's", () => {
  for (const [name, { size, line }] of Object.entries(look.text)) {
    assert.equal(size, rem(variable(`text-${name}`)), `text-${name}`);
    const ratio = variable(`text-${name}--line-height`).match(/calc\(([\d.]+) \/ ([\d.]+)\)|^([\d.]+)$/)!;
    const lineHeight = ratio[3] ? parseFloat(ratio[3]) * size : (parseFloat(ratio[1]) / parseFloat(ratio[2])) * size;
    assert.equal(line, lineHeight, `text-${name} line height`);
  }
  for (const [name, px] of Object.entries(look.radius)) if (name !== "full") assert.equal(px, rem(variable(`radius-${name}`)), `radius-${name}`);
  for (const [name, px] of Object.entries(look.blur)) assert.equal(px, parseFloat(variable(`blur-${name}`)), `blur-${name}`);
  assert.equal(look.tracking.tight, parseFloat(variable("tracking-tight")));
  for (const [name, layers] of Object.entries(look.shadow)) {
    const want = layers.map(([x, y, b, s]) => `${x}${x ? "px" : ""} ${y}px ${b}px ${s}${s ? "px" : ""} rgb(0 0 0 / ${look.shadowOpacity})`).join(", ");
    assert.equal(variable(`shadow-${name}`), want, `shadow-${name}`);
  }
});

/** A paint as Tailwind's color class suffix: `white/90`. */
const color = (p: look.Paint) => (typeof p === "string" ? p : `${p[0]}/${Math.round(p[1] * 100)}`);

/** The classes a surface's recipe takes. */
function classes(s: look.Surface): string[] {
  return [
    `rounded-${s.radius}`,
    `bg-${color(s.fill)}`,
    `text-${s.ink}`,
    ...(s.ring ? ["ring-1", `ring-${color(s.ring)}`] : []),
    ...(s.shadow ? [`shadow-${s.shadow}`] : []),
    ...(s.shadowColor ? [`shadow-${color(s.shadowColor)}`] : []),
    ...(s.blur ? [`backdrop-blur-${s.blur}`] : []),
  ];
}

/** Tailwind's spacing class for pixels: `px-5` for 20. */
const space = (axis: "x" | "y", px: number) => `p${axis}-${px / 4}`;

function has(className: string, want: string[], what: string) {
  const got = new Set(className.split(/\s+/));
  for (const c of want) assert.ok(got.has(c), `${what} has ${c}: ${className}`);
}

test("card.ts says each recipe of the look", () => {
  has(card.frost, classes(look.frost), "frost");
  has(card.pill, [...classes(look.pill), `hover:bg-${look.pillHover.fill}`, `hover:text-${look.pillHover.ink}`], "pill");
  has(card.dark, [...classes(look.current), `hover:bg-${look.currentHover.fill}`], "dark");
  has(card.field, [...classes(look.field), `text-${look.fieldRoom.size}`, space("x", look.fieldRoom.padX), space("y", look.fieldRoom.padY)], "field");
  has(card.inset, [...classes(look.inset), `p-${look.insetRoom / 4}`], "inset");
  has(card.insetPicked, [...classes(look.insetPicked), `p-${look.insetRoom / 4}`], "picked inset");
  has(card.disabled, [`disabled:opacity-${look.disabled * 100}`], "disabled");
  for (const name of ["card", "cardWide"] as const) assert.ok(card.card.includes(name === "card" ? `p-${look.layout.card / 4}` : `sm:p-${look.layout.cardWide / 4}`), `card padding ${name}`);
  const l = look.layout;
  has(card.column, ["max-w-3xl", space("x", l.gutter), `sm:${space("x", l.gutterWide)}`, `gap-${l.gap / 4}`, `pb-${l.bottom / 4}`, `sm:pb-${l.bottomWide / 4}`, `lg:max-w-[${l.most / 16}rem]`, `lg:${space("x", l.gutterThree)}`], "the column");
  const side = `clamp(${l.side.least / 16}rem,${l.side.share * 100}vw,${l.side.most / 16}rem)`;
  has(card.spread, [`lg:grid-cols-[${side}_minmax(0,${l.column / 16}rem)_${side}]`, `lg:gap-${l.gapThree / 4}`, "lg:justify-center"], "the three columns");
  has(card.middle, ["lg:col-start-2"], "the cards' column");
  assert.equal(l.column, 768, "the column is Tailwind's 3xl");
  assert.equal(l.wide, 640, "wide is Tailwind's sm");
  assert.equal(l.three, 1024, "three columns is Tailwind's lg");
});

test("card.ts says each shape and tone", () => {
  for (const [name, t] of Object.entries(look.tones)) has(card.tones[name as look.Tone], [`bg-${color(t.fill)}`, `text-${t.ink}`, `ring-${color(t.ring)}`], `tone ${name}`);
  const shape = [`text-${look.shape.size}`, `font-${look.shape.weight}`, space("x", look.shape.padX), space("y", look.shape.padY)];
  for (const which of ["button", "primary", "tab", "action"] as const) has(card[which], shape, which);
  const t = look.tag;
  has(card.tag, [`text-${t.size}`, `font-${t.weight}`, space("x", t.padX), space("y", t.padY), `rounded-${t.radius}`, "ring-1"], "tag");
  assert.doesNotMatch(card.tag, /shadow/, "a tag is flat: nothing you read looks pressable");
  for (const [name, w] of Object.entries(look.words)) {
    const want = [`text-${w.size}`, `text-${w.ink}`, ...(w.weight === "normal" ? [] : [`font-${w.weight}`]), ...(w.tight ? ["tracking-tight"] : [])];
    has(card[name as keyof typeof look.words], want, name);
  }
});

test("there is one yellow, the sunflower's, see-through, so its pill never reads as a button", () => {
  assert.deepEqual(look.tones.warn, look.tones.offer);
  assert.equal(card.tones.warn, card.tones.offer);
  for (const name of Object.keys(look.palette)) assert.doesNotMatch(name, /amber|yellow|orange/, `${name}: no second yellow`);
  const fill = look.tones.offer.fill;
  assert.ok(Array.isArray(fill) && fill[0] === "sunflower" && fill[1] < 0.5, "the sunflower's fill is see-through");
});

test("type comes in two sizes: a card's heading, and everything else", () => {
  assert.deepEqual(Object.keys(look.text).sort(), ["2xl", "base"]);
  const sizes = /\btext-(xs|sm|lg|xl|[3-9]xl)\b/;
  for (const [name, value] of Object.entries(card)) {
    if (typeof value === "string" && !["headline", "section"].includes(name)) assert.doesNotMatch(value, sizes, `${name} has no size of its own`);
  }
});

test("a surface is CSS any renderer reads", () => {
  assert.deepEqual(look.css(look.pill), {
    backgroundColor: "rgba(255, 255, 255, 0.8)",
    color: "#3f3f46",
    borderRadius: 9999,
    boxShadow: "0 0 0 1px rgba(0, 0, 0, 0.05), 0px 4px 6px -1px rgba(0, 0, 0, 0.15), 0px 2px 4px -2px rgba(0, 0, 0, 0.15)",
    backdropFilter: "blur(12px)",
  });
});

test("the wordmark's CSS is the look's", () => {
  const styles = readFileSync(new URL("./globals.css", import.meta.url), "utf8");
  const rule = (selector: string) => styles.match(new RegExp(`${selector.replace(/[.+]/g, "\\$&")} \\{([^}]*)\\}`))![1];
  const b = look.brand;
  assert.match(rule(".site-brand"), new RegExp(`font-size: clamp\\(${b.least / 16}rem, ${b.share * 100}vw, ${b.most / 16}rem\\);`));
  assert.match(rule(".site-brand"), new RegExp(`font-weight: ${look.weight[b.weight]};`));
  assert.match(rule(".site-brand"), new RegExp(`line-height: ${b.line};`));
  assert.match(rule(".site-brand"), new RegExp(`letter-spacing: ${b.tracking}em;`));
  assert.match(rule(".site-brand"), new RegExp(`padding: ${b.top / 16}rem 1rem ${b.below}em;`));
  assert.match(rule(".site-brand + main"), new RegExp(`padding-top: ${look.layout.top / 16}rem;`));
  assert.match(rule(".site-brand a"), new RegExp(`gap: ${b.gap / 16}rem;`));
  assert.match(rule(".site-brand a"), new RegExp(`color: ${look.palette[b.ink]};`));
  assert.match(rule(".site-brand-flower"), new RegExp(`width: ${b.flower}em;`));
});

test("the cards' width is the column's, at every screen", () => {
  // As the websites measure it in a browser whose scrollbar takes 15 pixels.
  for (const [screen, want] of [[390, 343], [768, 673], [1280, 708], [1680, 768]] as const) {
    assert.equal(Math.round(look.cardsWidth(screen, screen - 15)), want, `${screen} pixels`);
  }
});
