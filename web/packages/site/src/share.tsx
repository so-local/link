// Each website's share card (share-card.tsx), its tagline in the page's
// language, and the alt text what the home page is shared as.

import { shareCard, shareSize } from "./share-card";
import { defaultLanguage, isLanguage, site, shareTitle, word } from "./docs";

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
  return [{ id: "share", alt: shareTitle(lang), size: shareSize, contentType: "image/png" }];
}

export default async function Image({ params }: { params: Promise<{ lang: string }> }) {
  const lang = await language(params);
  return shareCard(word(site(lang), "Tagline"));
}
