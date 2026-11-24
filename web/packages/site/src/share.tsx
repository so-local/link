// The share card for links to the site, rendered at build time. It uses
// the site's look: the first background photo in the bucket's list
// (bucket.ts), with the home page's headline on a frosted white card to
// the left so the photo stays in view; without the bucket, the card
// alone. Its words are the docs': the headline is
// docs/README.md's, or its translation's, and the alt text comes from that title
// and the site name.

import { ImageResponse } from "next/og";
import { bucket } from "./bucket";
import { defaultLanguage, isLanguage, site, siteUrl, shareTitle } from "./docs";

const size = { width: 1200, height: 630 };

async function language(params: Promise<{ lang: string }> | { lang: string }) {
  const { lang } = await params;
  return isLanguage(lang) ? lang : defaultLanguage;
}

export async function generateImageMetadata({
  params,
}: {
  params: Promise<{ lang: string }> | { lang: string };
}) {
  const lang = await language(params);
  return [{ id: "share", alt: shareTitle(site(lang)), size, contentType: "image/png" }];
}

/** The first background's share copy, as a data URL, or none when the bucket can't be read. */
async function sharePhoto(): Promise<string | undefined> {
  try {
    const list = (await (await fetch(`${bucket}/backgrounds/backgrounds.json`)).json()) as { backgrounds?: { name: string }[] };
    const name = list.backgrounds?.[0]?.name;
    if (!name || !/^[a-z0-9-]+$/.test(name)) return;
    const photo = await fetch(`${bucket}/backgrounds/${name}/share.jpg`);
    if (!photo.ok) return;
    return `data:image/jpeg;base64,${Buffer.from(await photo.arrayBuffer()).toString("base64")}`;
  } catch {
    return;
  }
}

export default async function Image({ params }: { params: Promise<{ lang: string }> }) {
  const lang = await language(params);
  const title = site(lang).homeTitle;
  const src = await sharePhoto();

  return new ImageResponse(
    (
      <div style={{ display: "flex", width: "100%", height: "100%", background: "#0a0a0a" }}>
        {src && (
          <img
            src={src}
            alt=""
            width={size.width}
            height={size.height}
            style={{
              position: "absolute",
              inset: 0,
              objectFit: "cover",
              objectPosition: "left center",
            }}
          />
        )}
        <div
          style={{
            position: "absolute",
            left: 48,
            top: 0,
            bottom: 0,
            display: "flex",
            alignItems: "center",
          }}
        >
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 20,
              width: 520,
              padding: "40px 44px",
              borderRadius: 32,
              background: "rgba(255, 255, 255, 0.85)",
              border: "1px solid rgba(0, 0, 0, 0.05)",
              color: "#18181b",
            }}
          >
            <div style={{ fontSize: 28, color: "#52525b" }}>{new URL(siteUrl).host.replace(/^www\./, "")}</div>
            <div
              style={{ fontSize: 72, lineHeight: 1.05, letterSpacing: "-0.03em" }}
            >
              {title}
            </div>
          </div>
        </div>
      </div>
    ),
    size,
  );
}
