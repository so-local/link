// Authored documentation cards, navigation and optional rendering slots.

import type { Metadata } from "next";
import type { ReactNode } from "react";
import type { Components } from "hast-util-to-jsx-runtime";
import Link from "next/link";
import { notFound } from "next/navigation";
import {
  alternates,
  defaultLanguage,
  fullTitle,
  isLanguage,
  languages,
  locale,
  neighbors,
  page,
  pages,
  places,
  routes,
  folded,
  shareImage,
  shareTitle,
  site,
  trail,
  tree,
  word,
  type Language,
  type Card,
} from "./docs";
import { action, alone, body, headline, section } from "./card";
import Markdown from "./markdown";
import Nav from "./nav";


export function generateStaticParams() {
  return languages().flatMap((lang) => pages(lang).map(({ segments }) => ({ lang, path: segments })));
}

async function load(params: PageProps<"/[lang]/[[...path]]">["params"]) {
  const { lang, path = [] } = await params;
  if (!isLanguage(lang)) notFound();
  const found = page(lang, path);
  if (!found) notFound();
  return { lang: lang as Language, path, found };
}

export async function generateMetadata({ params }: PageProps<"/[lang]/[[...path]]">): Promise<Metadata> {
  const { lang, path, found } = await load(params);
  const s = site(lang);
  // The home page is shared by the site's name and its headline; another
  // page by its own title. A page's own image is its card, else the site's.
  const shared = path.length ? found.title : shareTitle(lang);
  const image = found.image ? { url: found.image, alt: found.title } : { url: shareImage(lang), width: 1200, height: 630, alt: shared };
  // Next replaces the layout's openGraph and twitter whole, so each is complete here.
  const each: Metadata = {
    alternates: { canonical: found.url, languages: alternates(found.file) },
    openGraph: { title: shared, description: found.description, url: found.url, siteName: s.name, type: "website", locale: locale(lang), images: [image] },
    twitter: { card: "summary_large_image", title: shared, description: found.description, images: [image.url] },
  };
  // The home page's tab says what it's shared as.
  if (!path.length) return { title: { absolute: shared }, ...each };
  // A section is called by its name and its hook, which says where it is
  // and sells it: "Exit: Home address, anywhere".
  const title = found.hook && found.hook !== found.title ? `${found.title}: ${found.hook}` : fullTitle(lang, found.url, found.title);
  return { title, description: found.description, ...each };
}

export type Rendering = {
  header?: ReactNode;
  breadcrumbs?: boolean;
  pagination?: boolean;
  components?: Components;
  card?: (card: Card, index: number) => ReactNode | undefined;
};

export default async function Page({ params, rendering = {} }: PageProps<"/[lang]/[[...path]]"> & { rendering?: Rendering }) {
  const { lang, path, found } = await load(params);
  const landing = !path.length && !!rendering.header;
  const s = site(lang);
  const words = { copy: word(s, "Copy"), copied: word(s, "Copied") };
  // A translation whose English has changed since says so, above it.
  const notice = found.behind ? (
    <p key="behind" className={`${section} text-sm`}>
      {word(s, "Behind")}{" "}
      {alternates(found.file)[defaultLanguage] && (
        <a href={alternates(found.file)[defaultLanguage]} className="underline underline-offset-2">
          {word(site(defaultLanguage), "Language")}
        </a>
      )}
    </p>
  ) : null;
  // Where the page is, from its tab down, above its title.
  const where = trail(lang, found.url);
  const crumbs = where.length ? (
    <nav aria-label={where.map((e) => e.label).join(" › ")} className="not-prose mb-4 text-sm text-zinc-500">
      {where.map((e, i) => (
        <span key={e.href}>
          {i > 0 && <span aria-hidden className="text-zinc-400"> › </span>}
          <Link href={e.href} className="hover:text-zinc-900 hover:underline hover:underline-offset-2">
            {e.label}
          </Link>
        </span>
      ))}
    </nav>
  ) : null;
  const cards = found.cards.map((card, i) => rendering.card?.(card, i) ?? (
    card.kind === "button" || card.kind === "pills" || card.kind === "grid" ? (
      <Markdown key={i} tree={card.content} words={words} components={rendering.components} pills={card.kind === "pills"} />
    ) : (
      <article key={i} className={landing ? "contents" : !path.length && i === 0 && !found.homepage ? headline : section}>
        {landing && i === 0 && rendering.header}
        {rendering.breadcrumbs !== false && !landing && i === 0 && crumbs}
        <Markdown tree={card.content} words={words} components={rendering.components} />
      </article>
    )
  ));
  // The pages before and after it in its tab, each a card on one line,
  // as wide as its words: the previous on the left, the next on the right.
  const { previous, next } = neighbors(lang, found.url);
  const step = `${action} gap-2`;
  const steps = (previous || next) && (
    <nav key="steps" className="flex flex-wrap justify-between gap-4">
      {previous && (
        <Link href={previous.href} className={step}>
          <span aria-hidden className="text-zinc-400">‹</span>
          <span className="text-zinc-500">{word(s, "Previous")}</span>
          <span className="min-w-0 truncate font-semibold">{previous.label}</span>
        </Link>
      )}
      {next && (
        <Link href={next.href} className={`${step} ml-auto`}>
          <span className="text-zinc-500">{word(s, "Next")}</span>
          <span className="min-w-0 truncate font-semibold">{next.label}</span>
          <span aria-hidden className="text-zinc-400">›</span>
        </Link>
      )}
    </nav>
  );
  return (
    <Nav
      tree={tree(lang)}
      places={places(lang)}
      headings={landing ? [] : found.headings}
      label={word(s, "Tabs")}
      lang={lang}
      languages={languages().map((l) => ({ lang: l, name: word(site(l), "Language"), home: site(l).tabs[0]?.href ?? "/" }))}
      routes={routes()}
      folded={folded}
      landing={landing}
      fold={{ show: word(s, "Show pages"), hide: word(s, "Hide pages") }}
    >
      <div className={landing ? `${section} landing w-full` : path.length ? body : alone}>{[notice, ...cards, rendering.pagination !== false && !landing && steps]}</div>
    </Nav>
  );
}
