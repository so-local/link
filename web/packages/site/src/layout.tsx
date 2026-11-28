// Every page: fonts, metadata, <html> in the page's language, the
// flower field behind it (@link-flowers/browser), and the same column with the tabs above, the site's
// tree beside. A page gives only its cards. The
// content is Markdown and interface labels come from the shared catalog;
// English is served without its /en
// prefix (config.ts).

import type { Metadata } from "next";
import type { ReactNode } from "react";
import { GeistSans } from "geist/font/sans";
import { GeistMono } from "geist/font/mono";
import { notFound } from "next/navigation";
import { isLanguage, languages, locale, site, siteUrl, shareImage, shareTitle } from "./docs";
import { FlowerBackground } from "@link-flowers/browser";
import { column } from "./card";
import { treeScript } from "./tree";


export function generateStaticParams() {
  return languages().map((lang) => ({ lang }));
}

// The share image comes from opengraph-image.tsx; Next adds its tags.
export async function generateMetadata({ params }: LayoutProps<"/[lang]">): Promise<Metadata> {
  const { lang } = await params;
  if (!isLanguage(lang)) return {};
  const s = site(lang);
  const share = shareTitle(lang);
  return {
    metadataBase: new URL(siteUrl),
    // A page's own title becomes "<site> — <title>": its tab's name.
    title: { default: s.name, template: `${s.name} — %s` },
    description: s.description,
    openGraph: {
      title: share,
      description: s.description,
      url: "/",
      siteName: s.name,
      type: "website",
      locale: locale(lang),
      images: [{ url: shareImage(lang), width: 1200, height: 630, alt: share }],
    },
    twitter: { card: "summary_large_image", title: share, description: s.description, images: [shareImage(lang)] },
  };
}

export default async function RootLayout({ children, params, background, header, analytics }: LayoutProps<"/[lang]"> & { background?: ReactNode; header?: ReactNode; analytics?: ReactNode }) {
  const { lang } = await params;
  if (!isLanguage(lang)) notFound();
  return (
    <html
      // treeScript sets data-tree before React hydrates, on purpose.
      suppressHydrationWarning
      lang={lang}
      data-scroll-behavior="smooth"
      className={`${GeistSans.variable} ${GeistMono.variable} h-full antialiased`}
    >
      <body className="min-h-dvh flex flex-col">
        {/* The tree as the reader left it, before anything below paints (tree.ts). */}
        <script dangerouslySetInnerHTML={{ __html: treeScript }} />
        {analytics}
        {background ?? <FlowerBackground />}
        {header}
        <main className={column}>{children}</main>
      </body>
    </html>
  );
}
