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
import { isLanguage, languages, locale, site, siteUrl, shareTitle } from "./docs";
import { FlowerBackground } from "@link-flowers/browser";
import { column } from "./card";


export function generateStaticParams() {
  return languages().map((lang) => ({ lang }));
}

// The share image comes from opengraph-image.tsx; Next adds its tags.
export async function generateMetadata({ params }: LayoutProps<"/[lang]">): Promise<Metadata> {
  const { lang } = await params;
  if (!isLanguage(lang)) return {};
  const s = site(lang);
  const share = shareTitle(s);
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
    },
    twitter: { card: "summary_large_image", title: share, description: s.description },
  };
}

export default async function RootLayout({ children, params, background, header }: LayoutProps<"/[lang]"> & { background?: ReactNode; header?: ReactNode }) {
  const { lang } = await params;
  if (!isLanguage(lang)) notFound();
  return (
    <html
      lang={lang}
      data-scroll-behavior="smooth"
      className={`${GeistSans.variable} ${GeistMono.variable} h-full antialiased`}
    >
      <body className="min-h-dvh flex flex-col">
        {background ?? <FlowerBackground />}
        {header}
        <main className={column}>{children}</main>
      </body>
    </html>
  );
}
