"use client";

// The site's navigation, around every page (docs.ts's tree, from
// navigation.md's "Pages"):
//
// - The tabs, one row of pills that sticks to the top as you scroll. On
//   a wide screen, pointing at a tab, or tabbing to it, drops down its
//   pages, so any page a level down is one click from anywhere.
// - The whole site's tree, beside every page on a wide screen, the home
//   page's too, so it stays the same from tab to tab: the page you're on a dark pill, the sections above it in bold, the
//   other sections folded with their number of pages, and a section's
//   reference pages (its specs, Decisions, Security) after a line. Under
//   the page you're on, its headings, so the tree is also "On this page":
//   the one you're reading lit as you scroll, and one you pick glides
//   into view (globals.css). A heading is marked with a #, so a place on
//   this page never passes for a page; a section of pages has its count
//   and an arrow that opens it.
// - On a narrow screen, the hamburger floats the same tree over the page,
//   just under the tabs, so nothing beneath it moves. Picking a section
//   goes to its page underneath and leaves the tree open, with the
//   section's pages unfolded, so the next pick is one tap away. Picking a
//   page with no pages under it, a heading, or another site's link, or
//   tapping outside, closes it.
// - On a wide screen, the tree folds away and back with a pill before the
//   tabs, leaving the page alone in the middle: folded to start where the
//   site says so (www.local.link, with few pages), and as the reader last
//   left it after that, in this browser.
//
// A tab to the other website (home's Documentation, to docs.local.link)
// opens it in a new tab, with an arrow to say so. Last in
// the tabs comes the language pill when the docs have more than one
// language: it opens in place, adding a pill per other language that goes
// to this page in it (or to its home page when it has no such page).

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import type { Entry, Node } from "./docs";
import { current as dark, frost, middle, pill, spread } from "./card";
import { treeKey as kept } from "./tree";

type Language = { lang: string; name: string; home: string };

/** The tree as the reader last left it, in this browser: "open", "folded" or none (tree.ts). */
function keptTree(): string | null {
  try {
    return localStorage.getItem(kept);
  } catch {
    return null;
  }
}
function watchTree(change: () => void) {
  window.addEventListener("storage", change);
  window.addEventListener(kept, change);
  return () => {
    window.removeEventListener("storage", change);
    window.removeEventListener(kept, change);
  };
}
function keepTree(value: "open" | "folded") {
  document.documentElement.dataset.tree = value;
  try {
    localStorage.setItem(kept, value);
  } catch {}
  window.dispatchEvent(new Event(kept));
}

/** Where the tabs' row was scrolled on the last page, so the next starts there; null before any page has drawn it. */
let rowLeft: number | null = null;

/**
 * Whether the bar's tree opens again on the next page: a section was
 * picked in it, so its pages are the next choice. Each page renders its
 * own navigation, so the choice is carried across the change here.
 */
let reopen = false;

/** How many pages are under a section, its own included. */
function count(node: Node): number {
  return node.children.reduce((n, child) => n + (child.href === node.href ? 0 : count(child)), 1);
}

/**
 * A tab's pages, dropped down from it on a wide screen: what's above the
 * line. It stays while you click the tab itself, so a page under it is
 * still one more click, and it waits a moment after the pointer leaves,
 * so a path that strays on the way down doesn't close it.
 */
function Menu({ tab, shown, done }: { tab: Node; shown: boolean; done: () => void }) {
  const pages = tab.children.filter((n) => !n.ref);
  return (
    <div className={`absolute top-full left-0 z-20 min-w-56 pt-2 ${shown ? "hidden lg:block" : "hidden"}`}>
      <ul className={`${frost} p-2 shadow-lg`}>
        {pages.map((page) => (
          <li key={page.href}>
            {page.group && (
              <>
                <div role="separator" className="mx-3 my-1.5 h-px bg-zinc-900/10" />
                <div className="px-3 py-1.5 font-semibold text-zinc-500">{page.group}</div>
              </>
            )}
            <Link
              href={page.href}
              onClick={done}
              className="block rounded-full px-3 py-1.5 text-zinc-700 hover:bg-zinc-900/5 hover:text-zinc-900"
            >
              {page.label}
            </Link>
          </li>
        ))}
      </ul>
    </div>
  );
}

function HeadingLinks({ headings, reading, pick }: { headings: Entry[]; reading?: string; pick: (href: string) => void }) {
  if (!headings.length) return null;
  return (
    <ul className="mt-0.5 mb-1 ml-3 border-l border-zinc-900/10 pl-1.5">
      {headings.map((heading) => (
        <li key={heading.href}>
          <a href={heading.href} onClick={() => pick(heading.href)} aria-current={heading.href === reading ? "location" : undefined}
            className={`block rounded-full px-3 py-1.5 transition-colors duration-200 hover:bg-zinc-900/5 hover:text-zinc-900 ${heading.href === reading ? "bg-zinc-900/5 text-zinc-900" : "text-zinc-500"}`}>
            <span aria-hidden className="mr-1.5 text-zinc-400">#</span>
            {heading.label}
          </a>
        </li>
      ))}
    </ul>
  );
}

/** One level of the tree, and the open levels under it. */
function Branch({
  nodes,
  path,
  depth,
  open,
  toggle,
  headings,
  reading,
  pick,
  leave,
}: {
  nodes: Node[];
  path: number[];
  depth: number;
  open: Record<string, boolean>;
  toggle: (href: string, now: boolean) => void;
  headings: Entry[];
  reading?: string;
  pick: (href: string) => void;
  /** A pick that ends the visit to the tree: a page with none under it, or a section's. */
  leave: (stay: boolean) => void;
}) {
  return (
    <ul className={depth > 0 ? "mt-0.5 mb-1 ml-3 border-l border-zinc-900/10 pl-1.5" : ""}>
      {nodes.map((node, i) => {
        const on = path[depth] === i;
        const line = node.ref && !nodes[i - 1]?.ref;
        const row = "block min-w-0 flex-1 rounded-full px-3 py-1.5";
        const group = node.group && (
          <>
            <div role="separator" className="mx-3 my-1.5 h-px bg-zinc-900/10" />
            <div className="px-3 py-1.5 font-semibold text-zinc-500">{node.group}</div>
          </>
        );
        if (!node.children.length) {
          const lit = on && path.length === depth + 1;
          return (
            <li key={`${node.href} ${i}`}>
              {line && <div role="separator" className="mx-3 my-1.5 h-px bg-zinc-900/10" />}
              {group}
              {node.href.startsWith("/") ? (
                <Link
                  href={node.href}
                  onClick={() => leave(false)}
                  aria-current={lit ? "page" : undefined}
                  className={`${row} ${lit ? dark : "text-zinc-700 hover:bg-zinc-900/5 hover:text-zinc-900"}`}
                >
                  {node.label}
                </Link>
              ) : (
                <a href={node.href} target="_blank" rel="noopener" data-away onClick={() => leave(false)} className={`${row} text-zinc-700 hover:bg-zinc-900/5 hover:text-zinc-900`}>
                  {node.label}
                </a>
              )}
              {lit && <HeadingLinks headings={headings} reading={reading} pick={pick} />}
            </li>
          );
        }
        const expanded = open[node.href] ?? on;
        return (
          <li key={`${node.href} ${i}`}>
            {line && <div role="separator" className="mx-3 my-1.5 h-px bg-zinc-900/10" />}
            {group}
            <div className="flex items-center">
              <Link
                href={node.href}
                onClick={() => {
                  toggle(node.href, true);
                  leave(true);
                }}
                aria-current={on && path.length === depth + 1 ? "page" : undefined}
                className={`${row} ${on && path.length === depth + 1 ? dark : `hover:bg-zinc-900/5 hover:text-zinc-900 ${on ? "font-semibold text-zinc-900" : "text-zinc-700"}`}`}
              >
                {node.label} <span className="font-normal text-zinc-400 tabular-nums">{count(node)}</span>
              </Link>
              <button
                type="button"
                aria-expanded={expanded}
                aria-label={node.label}
                onClick={() => toggle(node.href, !expanded)}
                className="grid size-8 shrink-0 place-items-center rounded-full text-zinc-400 hover:bg-zinc-900/5 hover:text-zinc-900"
              >
                <span aria-hidden className={`inline-block transition-transform ${expanded ? "rotate-90" : ""}`}>
                  ›
                </span>
              </button>
            </div>
            {expanded && on && path.length === depth + 1 && <HeadingLinks headings={headings} reading={reading} pick={pick} />}
            {expanded && (
              <Branch
                nodes={node.children}
                path={on ? path : []}
                depth={depth + 1}
                open={open}
                toggle={toggle}
                headings={headings}
                reading={reading}
                pick={pick}
                leave={leave}
              />
            )}
          </li>
        );
      })}
    </ul>
  );
}

export default function Nav({
  tree,
  places,
  headings,
  label,
  lang,
  languages,
  routes,
  folded,
  landing = false,
  fold: foldWords,
  children,
}: {
  tree: Node[];
  places: Record<string, number[]>;
  headings: Entry[];
  label: string;
  lang: string;
  languages: Language[];
  routes: Record<string, Record<string, string>>;
  /** Whether the tree starts folded on a wide screen. */
  folded: boolean;
  /** Center the landing content while keeping the same navigation controls. */
  landing?: boolean;
  /** The fold pill's words: to show the pages, and to hide them. */
  fold: { show: string; hide: string };
  children: ReactNode;
}) {
  const left = useSyncExternalStore(watchTree, keptTree, () => null);
  const away = left ? left === "folded" : folded;
  const pathname = usePathname();
  const [languagesOpen, setLanguagesOpen] = useState(false);
  // English is served from /en, so its pages are known without it.
  const at = routes[pathname] ? pathname : pathname.replace(/^\/en(?=\/|$)/, "") || "/";
  const here = routes[at] ?? {};
  const path = places[at] ?? [];

  // Sections opened or closed by hand, until you go to another page; the
  // bar's tree, open until then too.
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const [barOpen, setBarOpen] = useState(() => reopen);
  // Reopened after a section was picked, the bar shows the section's
  // pages first, without the headings of the page it went to.
  const [picked, setPicked] = useState(() => reopen);
  useEffect(() => {
    reopen = false;
  }, []);
  const [seen, setSeen] = useState(pathname);
  if (seen !== pathname) {
    setSeen(pathname);
    setOpen({});
    if (!reopen) setBarOpen(false);
  }
  const leave = (keep: boolean) => {
    reopen = keep;
    if (!keep) setBarOpen(false);
  };
  const bar_ = (open: boolean) => {
    setPicked(false);
    setBarOpen(open);
  };
  const toggle = (href: string, now: boolean) => setOpen((o) => ({ ...o, [href]: now }));

  // The heading you're reading: the last whose top has passed under the
  // tabs, or the last of all at the page's end. The tree lights it, so
  // it follows you down the page.
  const [reading, setReading] = useState<string>();
  useEffect(() => {
    const ids = headings.map((h) => h.href.slice(1));
    let frame = 0;
    const look = () => {
      frame = 0;
      let found: string | undefined;
      for (const id of ids) {
        const el = document.getElementById(id);
        if (el && el.getBoundingClientRect().top <= 96) found = id;
      }
      const end = window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 2;
      if (end && ids.length) found = ids[ids.length - 1];
      setReading(found && `#${found}`);
    };
    const soon = () => {
      if (!frame) frame = requestAnimationFrame(look);
    };
    soon();
    window.addEventListener("scroll", soon, { passive: true });
    window.addEventListener("resize", soon);
    return () => {
      window.removeEventListener("scroll", soon);
      window.removeEventListener("resize", soon);
      cancelAnimationFrame(frame);
    };
  }, [headings]);
  // A heading picked in the tree is lit at once, and the bar folds away.
  const pick = (href: string) => {
    setReading(href);
    setBarOpen(false);
  };

  // A long tree scrolls on its own, keeping the lit heading in it.
  const side = useRef<HTMLElement>(null);
  useEffect(() => {
    const box = side.current;
    const lit = box?.querySelector<HTMLElement>('[aria-current="location"]');
    if (!box || !lit) return;
    const top = lit.getBoundingClientRect().top - box.getBoundingClientRect().top + box.scrollTop;
    if (top < box.scrollTop || top + lit.offsetHeight > box.scrollTop + box.clientHeight) {
      box.scrollTo({ top: top - box.clientHeight / 3 });
    }
  }, [reading]);

  // The tab whose menu is down, and the wait before it goes up.
  const [menu, setMenu] = useState<string>();
  const wait = useRef<ReturnType<typeof setTimeout>>(undefined);
  const show = (href: string) => {
    clearTimeout(wait.current);
    setMenu(href);
  };
  const hide = (ms: number) => {
    clearTimeout(wait.current);
    wait.current = setTimeout(() => setMenu(undefined), ms);
  };
  useEffect(() => () => clearTimeout(wait.current), []);

  // On a narrow screen the tabs show the dark one, even one far along,
  // and stay where they are when it's already in view, so a tab you
  // scrolled to and picked never jumps away. Each page renders its own
  // tabs, so the row's place is carried across the change (rowLeft) and
  // set before it paints; then, after a tab is picked, the row glides to
  // it, unless the reader asks for less motion. A page that has just
  // loaded starts there. The tab's place is measured on screen: each tab
  // sits in a positioned item, so its offsetLeft is from that item, not
  // from the row.
  const tabs = useRef<HTMLElement>(null);
  useLayoutEffect(() => {
    const row = tabs.current;
    if (!row) return;
    const glide = rowLeft !== null && !window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (rowLeft !== null) row.scrollLeft = rowLeft;
    const keep = () => { rowLeft = row.scrollLeft; };
    row.addEventListener("scroll", keep, { passive: true });
    keep();
    const show = () => {
      const lit = row.querySelector<HTMLElement>('[aria-current="page"]');
      if (!lit || row.scrollWidth <= row.clientWidth) return;
      const box = row.getBoundingClientRect();
      const tab = lit.getBoundingClientRect();
      if (tab.left < box.left || tab.right > box.right) row.scrollBy({ left: tab.left - box.left - 64, behavior: glide ? "smooth" : "auto" });
    };
    // A glide starts two frames later: the router shows the new page and
    // scrolls it into view after this runs, which would stop a glide
    // already under way.
    if (!glide) show();
    let frame = glide ? requestAnimationFrame(() => { frame = requestAnimationFrame(show); }) : 0;
    return () => {
      cancelAnimationFrame(frame);
      row.removeEventListener("scroll", keep);
    };
  }, [pathname]);

  // The bar's tree floats just under the tabs, wherever they are as the
  // page scrolls, and keeps the page you're on in view inside it.
  const bar = useRef<HTMLElement>(null);
  const [barTop, setBarTop] = useState(0);
  useEffect(() => {
    if (!barOpen) return;
    const place = () => setBarTop(tabs.current?.getBoundingClientRect().bottom ?? 0);
    const close = (e: KeyboardEvent) => e.key === "Escape" && setBarOpen(false);
    place();
    window.addEventListener("scroll", place, { passive: true });
    window.addEventListener("resize", place);
    window.addEventListener("keydown", close);
    return () => {
      window.removeEventListener("scroll", place);
      window.removeEventListener("resize", place);
      window.removeEventListener("keydown", close);
    };
  }, [barOpen]);
  useEffect(() => {
    const box = bar.current;
    const lit = box?.querySelector<HTMLElement>('[aria-current="page"]');
    if (!box || !lit) return;
    const top = lit.getBoundingClientRect().top - box.getBoundingClientRect().top + box.scrollTop;
    if (top < box.scrollTop || top + lit.offsetHeight > box.scrollTop + box.clientHeight) box.scrollTo({ top: top - 8 });
  }, [barOpen, pathname]);

  const current = languages.find((l) => l.lang === lang);
  const others = languages.filter((l) => l.lang !== lang);
  const branch = (shown: Entry[]) => (
    <Branch
      nodes={tree}
      path={path}
      depth={0}
      open={open}
      toggle={toggle}
      headings={shown}
      reading={reading}
      pick={pick}
      leave={leave}
    />
  );
  const tree_ = branch(headings);
  return (
    <>
      <nav
        ref={tabs}
        aria-label={label}
        className="sticky top-2 z-10 -mx-4 -mb-2 overflow-x-auto px-4 [scrollbar-width:none] sm:-mx-10 sm:px-10 lg:mx-0 lg:overflow-visible lg:px-0"
      >
        <ul className="flex w-max gap-2 pb-2 text-sm lg:mx-auto">
          {tree.length > 0 && (
            <li className="sticky left-0 z-10 shrink-0 lg:static">
              <button
                type="button"
                aria-expanded={!away}
                aria-label={away ? foldWords.show : foldWords.hide}
                title={away ? foldWords.show : foldWords.hide}
                onClick={() => keepTree(away ? "open" : "folded")}
                className={`hidden px-4 py-2 lg:block ${pill}`}
              >
                <span aria-hidden>☰</span>
              </button>
              <button
                type="button"
                aria-expanded={barOpen}
                aria-label={barOpen ? foldWords.hide : foldWords.show}
                onClick={() => bar_(!barOpen)}
                className={`block px-4 py-2 lg:hidden ${pill}`}
              >
                <span aria-hidden>☰</span>
              </button>
            </li>
          )}
          {tree.map((t, i) => {
            const lit = i === path[0];
            return (
              <li
                key={t.href}
                className="relative"
                onPointerEnter={(e) => e.pointerType === "mouse" && show(t.href)}
                onPointerLeave={(e) => e.pointerType === "mouse" && hide(400)}
                onFocus={() => show(t.href)}
                onBlur={(e) => !e.currentTarget.contains(e.relatedTarget) && hide(0)}
                onKeyDown={(e) => e.key === "Escape" && hide(0)}
              >
                {t.href.startsWith("/") ? (
                  <Link
                    href={t.href}
                    aria-current={lit ? "page" : undefined}
                    className={`block px-4 py-2 ${lit ? dark : pill}`}
                  >
                    {t.label}
                  </Link>
                ) : (
                  <a href={t.href} target="_blank" rel="noopener" data-away className={`block px-4 py-2 ${pill}`}>
                    {t.label}
                  </a>
                )}
                {t.children.length > 0 && <Menu tab={t} shown={menu === t.href} done={() => hide(0)} />}
              </li>
            );
          })}
          {current && others.length > 0 && (
            <>
              <li>
                <button
                  type="button"
                  aria-expanded={languagesOpen}
                  onClick={() => setLanguagesOpen(!languagesOpen)}
                  className={`block px-4 py-2 ${pill}`}
                >
                  {current.name} <span aria-hidden>{languagesOpen ? "‹" : "›"}</span>
                </button>
              </li>
              {languagesOpen &&
                others.map((other) => (
                  <li key={other.lang}>
                    <Link href={here[other.lang] ?? other.home} hrefLang={other.lang} className={`block px-4 py-2 ${pill}`}>
                      {other.name}
                    </Link>
                  </li>
                ))}
            </>
          )}
        </ul>
      </nav>
      {tree.length > 0 ? (
        <div
          className={`${spread} ${landing ? "lg:items-stretch" : "lg:items-start"}`}
        >
          {barOpen && (
            <>
              <button
                type="button"
                aria-label={foldWords.hide}
                onClick={() => bar_(false)}
                className="fixed inset-0 z-[5] cursor-default bg-zinc-900/10 lg:hidden"
              />
              <nav
                ref={bar}
                aria-label={label}
                style={{ top: barTop, maxHeight: `min(70svh, calc(100svh - ${barTop}px - 1rem))` }}
                className={`${frost} fixed inset-x-4 z-20 overflow-y-auto overscroll-contain p-2 text-sm shadow-lg sm:inset-x-10 lg:hidden`}
              >
                {branch(picked ? [] : headings)}
              </nav>
            </>
          )}
          <nav
            ref={side}
            aria-label={label}
            className={`${frost} sticky top-16 hidden max-h-[calc(100vh-5rem)] self-start overflow-y-auto p-2 text-sm ${folded ? "" : "lg:block"} lg:[html[data-tree=open]_&]:block lg:[html[data-tree=folded]_&]:hidden`}
          >
            {tree_}
          </nav>
          <div className={`${middle} ${landing ? "flex-1" : ""}`}>
            {children}
          </div>
        </div>
      ) : (
        children
      )}
    </>
  );
}
