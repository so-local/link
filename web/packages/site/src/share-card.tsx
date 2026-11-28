// The share card: what a link shows when it's posted, on X, in Messages
// or anywhere else that reads Open Graph. It's the header's wordmark,
// large, on white: the sunflower, as tall as both lines, beside
// "local.link" (look.ts's `brand`), and the tagline centered under the
// name. Nothing else, so the
// card still reads as a thumbnail a few hundred pixels wide. Every
// website's card and every app's is this one; share.tsx gives a website's
// its tagline in the page's language.

import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { ImageResponse } from "next/og";
import { HeadArt } from "@link-flowers/browser/art";
import { brand, palette } from "./look";
import words from "../locales/en.json";

/** A share card's size, the one Open Graph and X's large card both show whole. */
export const shareSize = { width: 1200, height: 630 };

/** The websites' tagline, in English: what the card says under the wordmark. */
export const shareTagline = words.Tagline;

/** The wordmark's size on the card, in pixels: the name's type. */
const WORDMARK = 168;

/**
 * The tagline's size, as a share of the wordmark's: what sets "Own Your
 * Intelligence" exactly as wide as "local.link" in Geist, measured on the
 * rendered card. Another tagline needs its own.
 */
const TAGLINE = 0.38;

/** The flower's size, as a share of the wordmark's: larger than the header's, standing beside both lines. */
const FLOWER = 1.55;

/** Geist, the websites' typeface, from the nearest installed `geist`; none when it isn't found, and the renderer's own font draws the card. */
async function geist(): Promise<{ name: string; data: Buffer; weight: 600; style: "normal" }[]> {
  for (let dir = process.cwd(); dir !== dirname(dir); dir = dirname(dir)) {
    const file = join(dir, "node_modules/geist/dist/fonts/geist-sans/Geist-SemiBold.ttf");
    if (existsSync(file)) return [{ name: "Geist", data: await readFile(file), weight: 600, style: "normal" }];
  }
  return [];
}

/** The card: the wordmark over `tagline`. */
export async function shareCard(tagline: string = shareTagline): Promise<ImageResponse> {
  return new ImageResponse(
    (
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          gap: Math.round(WORDMARK * 0.25),
          width: "100%",
          height: "100%",
          background: "#ffffff",
          fontFamily: "Geist",
          fontWeight: 600,
          color: palette[brand.ink],
        }}
      >
        <HeadArt size={Math.round(WORDMARK * FLOWER)} />
        {/* Two lines of one width, flush left: the renderer draws the name's first letter its tracking in, so the tagline starts there too. */}
        <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", gap: 20 }}>
          <div style={{ fontSize: WORDMARK, lineHeight: 1, letterSpacing: `${brand.tracking}em`, paddingBottom: Math.round(WORDMARK * 0.08) }}>{brand.name}</div>
          <div style={{ marginLeft: Math.round(-brand.tracking * WORDMARK), fontSize: WORDMARK * TAGLINE, lineHeight: 1.1, letterSpacing: "-0.03em", color: palette["zinc-500"] }}>{tagline}</div>
        </div>
      </div>
    ),
    { ...shareSize, fonts: await geist() },
  );
}
