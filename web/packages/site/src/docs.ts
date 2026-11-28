// Markdown content and structural navigation for both websites. Each app's
// docs/navigation.md supplies its name and authored page outline, and a
// translation's docs/navigation.<language>.md its localized URLs. Shared
// locales/*.json supply interface labels. Page paths
// remain their URLs, and metadata comes from the actual homepage content.

import "server-only";
import { cache } from "react";
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, normalize, posix, relative, sep } from "node:path";
import type { Element, ElementContent, Root as Hast, RootContent } from "hast";
import type { Heading, List, Nodes, Root as Mdast } from "mdast";
import { toString } from "mdast-util-to-string";
import { actionTitle } from "./action";
import { opening, overview } from "./overview";
import { readOutline } from "./navigation";
import { translationLanguage } from "./languages.mjs";
import { format, markdown, metadata, metadataFields, type PageMetadata, type PageReference } from "./format";
import { stringify } from "yaml";


/** The default language: its pages have no prefix. */
export const defaultLanguage = "en";
export type Language = string;

/** The share card's locale, per language. */
const locales: Record<string, string> = { en: "en_US", es: "es_LA" };
export const locale = (lang: Language) => locales[lang] ?? lang;

/** Where a link to a file outside the docs goes: the repository holding every folder. */
const repository = "https://github.com/so-local/link/blob/dev";

/** The site's own file, which is no page. */
const siteFile = "navigation.md";

/** A file's translation, beside its English: guide.md in es is guide.es.md. */
const translated = (file: string, lang: Language) => (lang === defaultLanguage ? file : file.replace(/\.md$/, `.${lang}.md`));

/** A translation's English file: guide.es.md in es is guide.md. */
const englishOf = (file: string, lang: Language) => (lang === defaultLanguage ? file : file.replace(new RegExp(`\\.${lang}\\.md$`), ".md"));

/** The language a translation's name ends in: guide.es.md is es. */
const languageOf = translationLanguage;

/** The site's own file in a language. */
const siteFileOf = (lang: Language) => join(docsRoot, translated(siteFile, lang));
const localesRoot = process.env.LINK_SITE_LOCALES ?? join(/*turbopackIgnore: true*/ process.cwd(), "../../packages/site/locales");

// Read at build time only: every page is prerendered.
const web = join(/*turbopackIgnore: true*/ process.cwd());
const repo = join(web, "../../..");
const docsRoot = process.env.LINK_SITE_CONTENT_ROOT ?? join(web, "docs");

/** Only this app's content can be replaced by a prepared bucket snapshot. */
const contentRoot = (folder: string) => (folder === web ? docsRoot : join(folder, "docs"));

/**
 * One of the index's websites: its address, and its address while it's
 * developed on this machine (`npm run dev`); its app; whether its tree
 * beside a page starts folded; and whether the index's folders are its.
 */
type Website = { name: string; url: string; local: string; directory: string; tree: "open" | "folded"; roots?: boolean; homepage?: string; sharedPages?: Record<string, string>; assets?: Record<string, string>; developmentPages?: string[]; hiddenPages?: string[]; sectionPages?: "heading" };
/** An owner's docs; a preview may publish its pages while the program is held. */
type Root = { directory: string; released?: false; preview?: true };
type Index = { schema: number; sites: Website[]; roots: Root[]; includes?: string[] };

/** A folder the index names, refused when it escapes the repository. */
function inRepo(directory: string, at: string = repo): string {
  const folder = normalize(join(at, directory));
  if ((folder !== at && !folder.startsWith(at + sep)) || directory.split(/[\\/]/).includes("..")) {
    throw new Error("documentation owner escapes the repository");
  }
  return folder;
}

/** The documentation index of the repository at `at`, checked. */
function indexOf(at: string): Index {
  const index = JSON.parse(readFileSync(/*turbopackIgnore: true*/ join(at, "scripts", "documentation-index.json"), "utf8")) as Index;
  if (index.schema !== 1 || !Array.isArray(index.sites)) throw new Error("unsupported documentation index");
  for (const w of index.sites) {
    if (!["open", "folded"].includes(w.tree)) throw new Error(`${w.name}: tree is "open" or "folded"`);
    if (w.sectionPages !== undefined && w.sectionPages !== "heading") throw new Error(`${w.name}: sectionPages is heading or absent`);
    if (w.homepage !== undefined && (typeof w.homepage !== "string" || !w.homepage.endsWith(".md"))) throw new Error(`${w.name}: homepage names a Markdown file`);
    if (w.sharedPages !== undefined && (!w.sharedPages || Array.isArray(w.sharedPages) || typeof w.sharedPages !== "object" || Object.entries(w.sharedPages).some(([page, source]) => !/^(?!\/)(?!.*(?:^|\/)\.\.(?:\/|$))[^\\]+\.md$/.test(page) || page.endsWith("README.md") || typeof source !== "string" || !source.endsWith(".md")))) throw new Error(`${w.name}: sharedPages map a page of the site to a Markdown file`);
    if (w.assets !== undefined && (!w.assets || Array.isArray(w.assets) || typeof w.assets !== "object" || Object.values(w.assets).some(url => typeof url !== "string" || !/^\/(?![\\/])/.test(url)))) throw new Error(`${w.name}: assets map source paths to local website URLs`);
  }
  for (const r of index.roots) {
    if (r.released !== undefined && r.released !== false) throw new Error(`${r.directory}: released is false or absent`);
    if (r.preview !== undefined && r.preview !== true) throw new Error(`${r.directory}: preview is true or absent`);
  }
  return index;
}

type Resolved = Website & { folders: string[]; held: string[]; homeFile?: string; sharedFiles: Record<string, string>; assetFiles: Record<string, string> };

/**
 * Where a shared page's source is: a Markdown file in the repository at
 * `at` or in one its index includes, so a page authored once in the
 * public repository can also be a page of a website beside it.
 */
function sharedSource(source: string, at: string, index: Index): string {
  const file = normalize(join(at, source));
  const within = [at, ...(index.includes ?? []).map((path) => normalize(join(at, path)))];
  if (!within.some((root) => file.startsWith(root + sep))) throw new Error("shared page escapes the repository and its includes");
  return file;
}

/**
 * Each website of the repository at `at` and its folders: its app's, then
 * the index's when they're its, but those a release holds back, which are
 * `held`. A link to a held page goes to its file in the repository, and
 * the navigation drops it.
 */
function resolved(at: string, index: Index): Resolved[] {
  const apps = index.sites.map((w) => inRepo(w.directory, at));
  const roots = index.roots.map((r) => ({ folder: inRepo(r.directory, at), released: r.released !== false, preview: r.preview })).filter((r) => !apps.includes(r.folder));
  const listed = roots.filter((r) => r.released || r.preview).map((r) => r.folder);
  const held = roots.filter((r) => !r.released && !r.preview).map((r) => r.folder);
  return index.sites.map((w, i) => ({ ...w, folders: [apps[i], ...(w.roots ? listed : [])], held: w.roots ? held : [], homeFile: w.homepage ? inRepo(w.homepage, at) : undefined, sharedFiles: Object.fromEntries(Object.entries(w.sharedPages ?? {}).map(([page, source]) => [page, sharedSource(source, at, index)])), assetFiles: Object.fromEntries(Object.entries(w.assets ?? {}).map(([path, url]) => [inRepo(path, at), url])) }));
}

/**
 * The repositories an index `includes`, beside it: a website there is
 * another one to this, so a link into its docs becomes a link to it.
 * Links go one way: a public index includes nothing.
 */
function included(index: Index): string[] {
  return (index.includes ?? []).map((path) => normalize(join(repo, path)));
}

function websites(): Resolved[] {
  const index = indexOf(repo);
  return [...resolved(repo, index), ...included(index).flatMap((at) => resolved(at, indexOf(at)))];
}

/** This app's website, the one whose folder it runs in, and the others. */
function split() {
  const all = websites();
  const own = all.find((w) => w.folders[0] === web);
  if (!own) throw new Error(`${relative(repo, web)} is no website of the documentation index`);
  return { own, others: all.filter((w) => w !== own) };
}

const start = split();
let currentWebsite = start.own;

/** Hidden pages stay in source; development pages are available only locally. */
function published(file: string, website: Website = currentWebsite): boolean {
  return !website.hiddenPages?.includes(file) &&
    (process.env.NODE_ENV === "development" || !website.developmentPages?.includes(file));
}

/** The website's address. */
export const siteUrl = start.own.url;

/** Whether the tree beside a page starts folded, the page alone in the middle. */
export const folded = start.own.tree === "folded";

/** Documentation uses concise titles and the full article width. */
export const documentation = start.own.roots === true;

/** Another website's address: on this machine while it's developed, so its links work there. */
const addressOf = (w: Website, canonical = false) => (!canonical && process.env.NODE_ENV === "development" ? w.local : w.url);

/** The folders whose docs make the site, and the other websites. */
let folders: string[] = start.own.folders;
let held: string[] = start.own.held;
let elsewhere = start.others;
let sectionPages = start.own.sectionPages;


/** The languages: English, and each this folder's docs has a navigation.<language>.md for. */
function readLanguages(): Language[] {
  return readdirSync(/*turbopackIgnore: true*/ docsRoot)
    .flatMap((name) => (name === siteFile ? [defaultLanguage] : name.startsWith("navigation.") && languageOf(name) ? [languageOf(name)!] : []))
    .sort((a, b) => (a === defaultLanguage ? -1 : b === defaultLanguage ? 1 : a.localeCompare(b)));
}
let availableLanguages = readLanguages();

export function languages(): Language[] {
  refresh();
  return availableLanguages;
}

export function isLanguage(lang: string): lang is Language {
  return languages().includes(lang);
}

/** Every Markdown file of a language, by its English path in its folder's docs/, to where it is. */
const listed = new Map<Language, Map<string, string>>();
function where(lang: Language): Map<string, string> {
  refresh();
  if (!listed.has(lang)) listed.set(lang, withoutHeld(lang, walk(lang), folders, held));
  return listed.get(lang)!;
}

/**
 * `found` without the pages navigation.md places only beneath a held
 * page: their section is held, so they're no page of the site either.
 * Read from the file itself, before any page is known.
 */
function withoutHeld(lang: Language, found: Map<string, string>, roots: string[], away: string[]): Map<string, string> {
  found = new Map([...found].filter(([file]) => published(file)));
  if (!away.length) return found;
  const from = existsSync(/*turbopackIgnore: true*/ siteFileOf(lang)) ? lang : defaultLanguage;
  const tree = readOutline(siteFileOf(from), siteFileOf(defaultLanguage), (text) => markdown.parse(text));
  const at = tree.children.findIndex((n) => n.type === "heading" && n.depth === 2 && toString(n) === "Pages");
  const outline = tree.children.slice(at + 1).find((n) => n.type === "list");
  const dropped = new Set<string>();
  const visit = (list: List, under: boolean) => {
    for (const item of list.children) {
      const para = item.children[0];
      const link = para?.type === "paragraph" ? para.children.find((c) => c.type === "link") : undefined;
      let beneath = under;
      if (link?.type === "link" && !/^(?:[a-z][a-z+.-]*:|\/|#)/i.test(link.url)) {
        const target = normalize(join(docsRoot, dirname(siteFile), link.url.split("#")[0]));
        beneath ||= isHeld(away, from, target) || isHeld(away, defaultLanguage, target);
        const file = inFolders(roots, from, target) ?? inFolders(roots, defaultLanguage, target);
        if (beneath && file !== undefined) dropped.add(file);
      }
      const nested = item.children.find((c): c is List => c.type === "list");
      if (nested) visit(nested, beneath);
    }
  };
  if (at >= 0 && outline?.type === "list") visit(outline, false);
  return new Map([...found].filter(([file]) => !dropped.has(file)));
}

function files(lang: Language): string[] {
  return [...where(lang).keys()];
}

/** Where a page is on disk: its folder's docs/<file>, or its translation beside it. */
function pathOf(lang: Language, file: string): string {
  return where(lang).get(file) ?? join(docsRoot, translated(file, lang));
}

function walk(lang: Language, roots = folders, stamps?: Map<string, string>, referenceRoots = held, website = currentWebsite): Map<string, string> {
  const found = new Map<string, string>();
  for (const folder of [...roots, ...referenceRoots]) {
    const root = contentRoot(folder);
    if (!existsSync(/*turbopackIgnore: true*/ root)) continue;
    const walk = (dir: string) => {
      for (const name of readdirSync(/*turbopackIgnore: true*/ dir).sort()) {
        const path = join(dir, name);
        const stat = statSync(/*turbopackIgnore: true*/ path);
        if (stat.isDirectory()) walk(path);
        else if (name.endsWith(".md") && (languageOf(name) ?? defaultLanguage) === lang) {
          const file = englishOf(relative(root, path).split(sep).join("/"), lang);
          if (referenceRoots.includes(folder) && !file.startsWith("config/specs/")) continue;
          stamps?.set(path, `${stat.mtimeMs}:${stat.ctimeMs}:${stat.size}:${stat.ino}`);
          if (folder === web && (file === siteFile || /^navigation\/[^/]+\.md$/.test(file))) continue;
          const other = found.get(file);
          if (other) throw new Error(`two pages at one URL: ${relative(repo, other)} and ${relative(repo, path)}`);
          found.set(file, path);
        }
      }
    };
    walk(root);
  }
  if (website.homeFile && (lang === defaultLanguage || !found.has("README.md"))) {
    const stat = statSync(/*turbopackIgnore: true*/ website.homeFile);
    if (!stat.isFile()) throw new Error("homepage must name a Markdown file");
    stamps?.set(website.homeFile, `${stat.mtimeMs}:${stat.ctimeMs}:${stat.size}:${stat.ino}`);
    if (lang === defaultLanguage && found.has("README.md")) throw new Error("homepage has a second authored README.md");
    found.set("README.md", website.homeFile);
  }
  // A shared page is the site's own page, read from its one source.
  for (const [file, path] of Object.entries(website.sharedFiles)) {
    if (lang !== defaultLanguage && found.has(file)) continue;
    const stat = statSync(/*turbopackIgnore: true*/ path);
    if (!stat.isFile()) throw new Error(`shared page ${file} must name a Markdown file`);
    stamps?.set(path, `${stat.mtimeMs}:${stat.ctimeMs}:${stat.size}:${stat.ino}`);
    if (lang === defaultLanguage && found.has(file)) throw new Error(`shared page ${file} has a second authored page`);
    found.set(file, path);
  }
  return found;
}

/** The site's page a file on disk is the one source of: its homepage or a shared page. */
function sharedPage(website: Resolved, target: string): string | undefined {
  if (target === website.homeFile) return "README.md";
  return Object.entries(website.sharedFiles).find(([, path]) => path === target)?.[0];
}

/** A file's path in the docs of `lang`, if `target` (a path on disk) is in one folder's. */
function inDocs(lang: Language, target: string): string | undefined {
  return sharedPage(currentWebsite, target) ?? inFolders(folders, lang, target) ?? referenceIn(held, lang, target);
}

/** Specification references stay readable even when their module is not released. */
function referenceIn(roots: string[], lang: Language, target: string): string | undefined {
  const file = inFolders(roots, lang, target);
  return file?.startsWith("config/specs/") ? file : undefined;
}

function isHeld(roots: string[], lang: Language, target: string): boolean {
  return inFolders(roots, lang, target) !== undefined && referenceIn(roots, lang, target) === undefined;
}

/** A file's path in the docs of `lang`, if `target` is in one of these folders'. */
function inFolders(roots: string[], lang: Language, target: string): string | undefined {
  for (const folder of roots) {
    const rel = englishOf(relative(contentRoot(folder), target).split(sep).join("/"), lang);
    if (!rel.startsWith("..") && !rel.startsWith("/")) return rel;
  }
}

/** A language's own names for folders and files, from its navigation.<language>.md. */
function slugs(lang: Language): Record<string, string> {
  return keep(`slugs ${lang}`, () => {
    if (lang === defaultLanguage) return {};
    const text = readFileSync(/*turbopackIgnore: true*/ siteFileOf(lang), "utf8");
    const at = text.indexOf("\n## URLs\n");
    const map: Record<string, string> = {};
    if (at < 0) return map;
    for (const line of text.slice(at + 9).split("\n")) {
      if (line.startsWith("## ")) break;
      const m = line.match(/^- ([^:]+): (\S+)$/);
      if (m) map[m[1].trim()] = m[2].trim();
    }
    return map;
  });
}

/**
 * A page's URL segments: agents/README.md is [agents], terms.md is
 * [terms], each in the language's own words where it has them.
 */
function segmentsOf(lang: Language, file: string): string[] {
  const parts = file.replace(/\.md$/, "").split("/");
  if (parts.at(-1) === "README") parts.pop();
  const own = slugs(lang);
  return parts.map((part) => own[part] ?? part);
}

/** The URL of a page, by its segments. */
function urlOf(lang: Language, segments: string[]): string {
  const prefix = lang === defaultLanguage ? "" : `/${lang}`;
  return `${prefix}/${segments.join("/")}` || "/";
}

/** Every page of a language: its file and its URL segments. */
export function pages(lang: Language): { file: string; segments: string[] }[] {
  return keep(`pages ${lang}`, () => files(lang).map((file) => ({ file, segments: segmentsOf(lang, file) })));
}

/** The URL of a file in each language that has it. */
export function alternates(file: string): Record<Language, string> {
  return keep(`alternates ${file}`, () => {
    const found: Record<Language, string> = {};
    for (const lang of languages()) {
      if (files(lang).includes(file)) found[lang] = urlOf(lang, segmentsOf(lang, file));
    }
    return found;
  });
}

/** Every page's URL, to the same page's URL in each language. */
export function routes(): Record<string, Record<Language, string>> {
  return keep("routes", () => {
    const all: Record<string, Record<Language, string>> = {};
    for (const lang of languages()) {
      for (const { file } of pages(lang)) {
        const each = alternates(file);
        all[each[lang]] = each;
      }
    }
    return all;
  });
}

function byUrl(lang: Language): Map<string, string> {
  return keep(`urls ${lang}`, () => {
    const found = new Map<string, string>();
    for (const { file, segments } of pages(lang)) {
      const url = segments.join("/");
      if (found.has(url)) throw new Error(`two pages at one URL: ${found.get(url)} and ${file}`);
      found.set(url, file);
    }
    return found;
  });
}

function fileOf(lang: Language, segments: string[]): string | undefined {
  const key = segments.join("/");
  return byUrl(lang).get(key);
}

type Source = { text: string; tree?: Mdast };

function source(lang: Language, file: string): Source {
  refresh();
  const path = file === siteFile ? siteFileOf(lang) : pathOf(lang, file);
  const known = sources.get(path);
  if (known) return known;
  const text = readFileSync(/*turbopackIgnore: true*/ path, "utf8");
  const found = { text };
  sources.set(path, found);
  return found;
}

function parse(lang: Language, file: string): Mdast {
  const found = source(lang, file);
  if (found.tree) return found.tree;
  // Repository reading indexes duplicate the website's navigation.
  const tree = markdown.parse(found.text
    .replace(/<!-- docs-tree:start -->[\s\S]*?<!-- docs-tree:end -->/g, "")
    .replace(/<!-- inventory:specs:start -->[\s\S]*?<!-- inventory:specs:end -->/g, ""));
  return found.tree = file === "README.md" && currentWebsite.homeFile ? overview(tree) : tree;
}

/** Portable Markdown for a bucket publication, using the same link resolver as pages. */
export function contentFiles(): Record<string, string> {
  const files: Record<string, string> = {};
  for (const lang of languages()) {
    for (const file of [siteFile, ...pages(lang).map(page => page.file)]) {
      const tree = markdown.parse(source(lang, file).text);
      relink(tree, url => resolve(lang, file, url, true));
      const frontmatter = tree.children.find(node => node.type === "yaml");
      if (frontmatter) {
        const fields = metadataFields(tree);
        if (fields.image) fields.image = pageMetadata(lang, file).image;
        frontmatter.value = stringify(fields);
      }
      files[translated(file, lang)] = markdown.stringify(tree);
    }
  }
  return files;
}

/** The page of `lang` at `file`, or its English while it isn't translated. */
function pageUrl(lang: Language, file: string): string | undefined {
  for (const each of [lang, defaultLanguage]) {
    if (where(each).has(file)) return urlOf(each, segmentsOf(each, file));
  }
}

/** The page an English URL of the site names, e.g. /agents. */
function fileAt(path: string): string | undefined {
  const key = path.replace(/^\/+|\/+$/g, "");
  return byUrl(defaultLanguage).get(key);
}

/**
 * Where a link in `file` goes on the website: a page of the docs by its
 * URL, in the reader's language; a file elsewhere in the repository on
 * GitHub; anything else as it is. A page links another by a relative
 * path to its file, in any folder, so a reader on GitHub stays in the
 * repository; here it becomes a link within the site.
 */
function resolve(lang: Language, file: string, url: string, canonical = false): string | undefined {
  // A link to this site's address is to one of its pages, which must be
  // there; one to the other website's, or the bare domain's, stays.
  const site = url.match(/^(https:\/\/[a-z.]*local\.link)(\/[^#?]*)?(#.*)?$/);
  if (site) {
    if (site[1] !== siteUrl) return url;
    const found = fileAt(site[2] ?? "/");
    if (found) return pageUrl(lang, found)! + (site[3] ?? "");
    throw new Error(`${file}: ${url} is no page of the site`);
  }
  if (/^[a-z][a-z+.-]*:/i.test(url)) {
    // Only the web's and mail's: a page never links `javascript:` or
    // `data:`.
    if (!/^(https?|mailto):/i.test(url)) throw new Error(`${file}: ${url} isn't a link of the web`);
    return url;
  }
  if (url.startsWith("#") || url.startsWith("/")) return url;
  const [path, anchor] = url.split("#", 2);
  const hash = anchor ? `#${anchor}` : "";
  const from = file === siteFile ? siteFileOf(lang) : file === "README.md" && currentWebsite.homeFile ? currentWebsite.homeFile : pathOf(lang, file);
  const target = normalize(join(dirname(from), path));
  const rel = inDocs(lang, target) ?? inDocs(defaultLanguage, target);
  if (rel !== undefined) {
    const md = rel.endsWith(".md") ? rel : posix.join(rel, "README.md");
    if (!published(md)) return;
    const found = pageUrl(lang, md);
    if (found) return found + hash;
  }
  const away = elsewhere.map((w) => [w, sharedPage(w, target) ?? inFolders(w.folders, defaultLanguage, target) ?? referenceIn(w.held, defaultLanguage, target)] as const).find(([, rel]) => rel !== undefined);
  if (away) {
    const [w, rel] = away;
    const md = rel!.endsWith(".md") ? rel! : posix.join(rel!, "README.md");
    if (!published(md, w)) return;
    return `${addressOf(w, canonical)}${urlOf(defaultLanguage, segmentsOf(defaultLanguage, md))}${hash}`;
  }
  const at = included(indexOf(repo)).find((other) => target.startsWith(other + sep)) ?? repo;
  return `${repository}/${relative(at, target).split(sep).join("/")}${hash}`;
}

/** Rewrite every link in a Markdown tree for the website. */
function relink(tree: Mdast, at: (url: string) => string | undefined, asset?: (url: string) => string) {
  type Node = { type: string; url?: string; children?: Node[] };
  const visit = (node: Node) => {
    if (!node.children) return;
    node.children = node.children.flatMap((child): Node[] => {
      // Generated indexes retain every source page. Website lists omit
      // entries whose sole link names an unavailable page.
      if (child.type === "listItem") {
        const paragraph = child.children?.[0];
        const links = paragraph?.type === "paragraph" ? paragraph.children?.filter((n) => n.type === "link") : [];
        if (links?.length === 1 && links[0].url && at(links[0].url) === undefined) return [];
      }
      if ((child.type === "link" || child.type === "definition") && child.url) {
        const url = at(child.url);
        // In prose, keep the words without advertising an unavailable page.
        if (url === undefined) return child.type === "link" ? child.children ?? [] : [];
        child.url = url;
      }
      if (child.type === "image" && child.url && asset) child.url = asset(child.url);
      visit(child);
      return [child];
    });
  };
  visit(tree);
}

/** Page metadata is read without rendering its links, so grids may link each other. */
function pageMetadata(lang: Language, file: string): PageMetadata {
  return keep(`metadata ${lang} ${file}`, () => {
    const info = metadata(parse(lang, file));
    if (info.image && !/^(?:https?:\/\/|\/)/i.test(info.image)) {
      const target = normalize(join(dirname(pathOf(lang, file)), info.image));
      const declared = currentWebsite.assetFiles[target];
      if (!declared) throw new Error(`${file}: relative primary image must be a declared website asset: ${info.image}`);
      info.image = declared;
    }
    return info;
  });
}

function cardReference(lang: Language, href: string): PageReference | undefined {
  if (!href.startsWith("/")) return;
  const segments = href.split(/[?#]/)[0].split("/").filter(Boolean);
  if (lang !== defaultLanguage && segments[0] === lang) segments.shift();
  const file = fileOf(lang, segments) ?? fileOf(defaultLanguage, segments);
  if (!file) return;
  const from = where(lang).has(file) ? lang : defaultLanguage;
  return { ...pageMetadata(from, file), href };
}

/** A button: a paragraph holding only a link titled "button". */
function asButton(node: RootContent): Element | undefined {
  if (node.type !== "element" || node.tagName !== "p") return;
  const kids = node.children.filter((c) => !(c.type === "text" && !c.value.trim()));
  const [only] = kids;
  if (kids.length !== 1 || only.type !== "element" || only.tagName !== "a") return;
  const style = actionTitle(only.properties.title);
  if (style?.kind !== "button") return;
  return { ...only, properties: { ...only.properties, title: undefined, dataButton: true, dataShortAction: style.short } };
}

/** A row of pills: a list whose every item is only a link. */
function asPills(node: RootContent): Element | undefined {
  if (node.type !== "element" || node.tagName !== "ul") return;
  const items = node.children.filter((c): c is Element => c.type === "element");
  const only = (li: Element) => {
    const kids = li.children.filter((c) => !(c.type === "text" && !c.value.trim()));
    return kids.length === 1 && kids[0].type === "element" && kids[0].tagName === "a";
  };
  if (!items.length || !items.every((li) => li.tagName === "li" && only(li))) return;
  return node;
}

/** Text on one line, as a description wants it. */
function flat(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

/** A visual line break still separates words in search and share descriptions. */
function plainText(node: Nodes): string {
  if (node.type === "break") return " ";
  return "children" in node ? node.children.map(plainText).join("") : toString(node);
}

/** One authored card, a lone button or a row of links. */
export type Card = { kind: "card" | "button" | "pills" | "grid"; content: Hast };

export type Page = {
  homepage: boolean;
  file: string;
  /** Its URL, e.g. /openclaw/security. */
  url: string;
  title: string;
  /** A section's hook, the line it shows as its heading. */
  hook?: string;
  description: string;
  /** The page's primary image, shared by collection cards and social metadata. */
  image?: string;
  /** A translation made from English that has changed since. */
  behind: boolean;
  cards: Card[];
  /** Its `##` headings, for "On this page". */
  headings: Entry[];
  /** Headings and page links shared with the navigation. */
  contents: DocEntry[];
};

/** Whether a translated page was made from its English as it is now. */
function behind(lang: Language, file: string): boolean {
  if (lang === defaultLanguage) return false;
  if (pathOf(lang, file) === pathOf(defaultLanguage, file)) return false;
  const text = readFileSync(/*turbopackIgnore: true*/ pathOf(lang, file), "utf8");
  const made = text.match(/^<!-- Translated from \S+ \(([0-9a-f]{16})\)/)?.[1];
  const source = pathOf(defaultLanguage, file);
  if (!existsSync(/*turbopackIgnore: true*/ source)) return false;
  const now = createHash("sha256").update(readFileSync(/*turbopackIgnore: true*/ source, "utf8")).digest("hex").slice(0, 16);
  return made !== now;
}

/**
 * A card's nodes with each heading and what follows it, up to the next
 * heading as high, in a <section> of its own, `##` then `###` within: so
 * a link to a heading can light its whole section (globals.css).
 */
function sections(nodes: ElementContent[], depth: number): ElementContent[] {
  if (depth > 4) return nodes;
  const level = (node: ElementContent) =>
    node.type === "element" && /^h[1-6]$/.test(node.tagName) ? Number(node.tagName[1]) : 0;
  const out: ElementContent[] = [];
  let run: ElementContent[] = [];
  let open: Element | undefined;
  const end = () => {
    if (open) open.children = [open.children[0], ...sections(open.children.slice(1) as ElementContent[], depth + 1)];
    else out.push(...sections(run, depth + 1));
    open = undefined;
    run = [];
  };
  for (const node of nodes) {
    const l = level(node);
    if (l === depth) {
      end();
      open = { type: "element", tagName: "section", properties: {}, children: [node] };
      out.push(open);
    } else if (l && l < depth) {
      end();
      out.push(node);
    } else if (open) open.children.push(node);
    else run.push(node);
  }
  end();
  return out;
}

/** A page, by its URL segments; undefined when there is none. */
export function page(lang: Language, segments: string[]): Page | undefined {
  const file = fileOf(lang, segments);
  if (!file) return;
  return keep(`page ${lang} ${file}`, () => {
    // Rewriting links must not change the shared source tree.
    const tree = structuredClone(parse(lang, file));
    const info = pageMetadata(lang, file);
    const homepage = file === "README.md" && !!currentWebsite.homeFile;
    const heading = tree.children.find((n): n is Heading => n.type === "heading" && n.depth === 1);
    const name = info.title;
    if (heading && name !== toString(heading)) heading.children = [{ type: "text", value: name }];
    // Marketing pages show their hook; documentation keeps its short title.
    const at = heading ? tree.children.indexOf(heading) : -1;
    const hook = tree.children[at + 1];
    const line = hook?.type === "blockquote" && hook.children.length === 1 ? hook.children[0] : undefined;
    const hooked = heading && line?.type === "paragraph";
    if (hooked) {
      if (!start.own.roots) heading.children = line.children;
      tree.children.splice(at + 1, 1);
    }
    const paragraph = opening(tree);
    relink(tree, (url) => resolve(lang, file, url), homepage ? (url) => {
      if (/^https?:\/\//i.test(url)) return url;
      if (/^[a-z][a-z+.-]*:/i.test(url) || url.startsWith("//")) throw new Error("overview image has an unsafe URL");
      const target = normalize(join(dirname(currentWebsite.homeFile!), url));
      const asset = currentWebsite.assetFiles[target];
      if (!asset) throw new Error(`overview image is not a declared website asset: ${url}`);
      return asset;
    } : undefined);

    const rendered = format(tree, href => cardReference(lang, href));
    const sectionEntries: DocEntry[] = rendered.children.flatMap((node, i) => {
      if (node.type !== "element" || node.tagName !== "h2" || typeof node.properties.id !== "string") return [];
      const paragraph = rendered.children.slice(i + 1).find((child) => child.type === "element");
      return [{ label: hastText(node), href: `#${node.properties.id}`, description: paragraph?.type === "element" && paragraph.tagName === "p" ? summary(hastText(paragraph)) : "" }];
    });
    const headings: Entry[] = sectionEntries.map(({ label, href }) => ({ label, href }));
    const url = urlOf(lang, segments);
    const contents = documentation ? pageContents(lang, url, sectionEntries) : [];
    if (documentation && !contents.length) {
      const title = rendered.children.find((node) => node.type === "element" && node.tagName === "h1");
      if (title?.type === "element" && typeof title.properties.id === "string") {
        contents.push({ label: name, href: `#${title.properties.id}`, description: paragraph ? summary(toString(paragraph)) : "" });
      }
    }
    const contentsSection: Element = {
      type: "element", tagName: "nav", properties: { ariaLabel: word(site(lang), "Docs") }, children: [
        { type: "element", tagName: "h2", properties: { id: "page-contents" }, children: [{ type: "text", value: word(site(lang), "Docs") }] },
        { type: "element", tagName: "ul", properties: {}, children: contents.map((entry): Element => ({
          type: "element", tagName: "li", properties: {}, children: [{
            type: "element", tagName: "a", properties: { href: entry.href }, children: [{ type: "text", value: entry.label }],
          }, ...(entry.description ? [{ type: "text" as const, value: `: ${entry.description}` }] : [])],
        })) },
      ],
    };

    // `---` breaks docs into cards; a shared homepage keeps its authored rules in one card.
    const cards: Card[] = [];
    let current: ElementContent[] = [];
    const flush = () => {
      const nodes = current.filter((n) => !(n.type === "text" && !n.value.trim()));
      if (nodes.length) {
        const grid = nodes.length === 1 && nodes[0].type === "element" && Array.isArray(nodes[0].properties.className) && nodes[0].properties.className.includes("page-grid");
        const button = nodes.length === 1 ? asButton(nodes[0]) : undefined;
        const pills = nodes.length === 1 ? asPills(nodes[0]) : undefined;
        cards.push(
          grid
            ? { kind: "grid", content: { type: "root", children: nodes } }
            : button
            ? { kind: "button", content: { type: "root", children: [button] } }
            : pills
              ? { kind: "pills", content: { type: "root", children: [pills] } }
              : { kind: "card", content: { type: "root", children: sections(nodes, 2) } },
        );
      }
      current = [];
    };
    for (const node of rendered.children) {
      if (node.type === "element" && node.tagName === "hr") {
        if (node.properties.dataOverviewHeader) {
          current = [
            { type: "element", tagName: "header", properties: { className: ["overview-header"] }, children: current },
          ];
        } else if (homepage) current.push(node);
        else flush();
      }
      else if (node.type !== "doctype") current.push(node as ElementContent);
    }
    flush();
    if (documentation) {
      const last = cards.at(-1);
      if (last?.kind === "card") last.content.children.push(contentsSection);
      else cards.push({ kind: "card", content: { type: "root", children: [contentsSection] } });
    }

    // A tab's page is called what its tab is, unless the tab has a short
    // label of its own ("tab"), when the page keeps its title.
    const tab = site(lang).tabs.find((t) => t.href === url && !t.tab);
    return {
      homepage,
      file,
      url,
      title: tab && url !== "/" ? tab.label : name,
      hook: hooked && (!start.own.roots || sectionPages === "heading") ? toString(line) : undefined,
      description: info.description,
      image: info.image,
      behind: behind(lang, file),
      cards,
      headings,
      contents,
    };
  });
}

/** The text of a rendered node. */
function hastText(node: ElementContent): string {
  if (node.type === "text") return node.value;
  return node.type === "element" ? node.children.map(hastText).join("") : "";
}

/** A link the site lays out: a tab, a page of a section, a heading. */
export type Entry = { label: string; href: string };
export type DocEntry = Entry & { description: string };

/** The same first-sentence description used by repository reading indexes. */
function summary(text: string): string {
  return flat(text).split(". ")[0].replace(/\.$/, "");
}

function docDescription(lang: Language, href: string): string {
  const segments = href.split(/[?#]/)[0].split("/").filter(Boolean);
  if (lang !== defaultLanguage && segments[0] === lang) segments.shift();
  const file = href.startsWith("/") ? fileOf(lang, segments) ?? fileAt(href) : undefined;
  if (file) {
    const paragraph = parse(where(lang).has(file) ? lang : defaultLanguage, file).children.find((node) => node.type === "paragraph");
    return paragraph ? summary(toString(paragraph)) : "";
  }
  for (const website of elsewhere) {
    if (!href.startsWith(addressOf(website) + "/")) continue;
    const path = href.slice(addressOf(website).length + 1).split(/[?#]/)[0];
    for (const folder of website.folders) {
      for (const language of [lang, defaultLanguage]) {
        for (const relative of path ? [`${path}.md`, `${path}/README.md`] : ["README.md"]) {
          const root = join(folder, "docs");
          const target = normalize(join(root, translated(relative, language)));
          if (!target.startsWith(root + sep)) continue;
          if (!existsSync(/*turbopackIgnore: true*/ target)) continue;
          const tree = markdown.parse(readFileSync(/*turbopackIgnore: true*/ target, "utf8"));
          const paragraph = tree.children.find((node) => node.type === "paragraph");
          if (paragraph) return summary(toString(paragraph));
        }
      }
    }
  }
  return "";
}

/** The page's headings and child pages, in the same order as its navigation. */
function pageContents(lang: Language, url: string, headings: DocEntry[]): DocEntry[] {
  const tabs = site(lang).tabs;
  const find = (nodes: Node[]): Node | undefined => {
    for (const node of nodes) {
      if (node.href === url) return node;
      const nested = find(node.children);
      if (nested) return nested;
    }
  };
  const children = url === urlOf(lang, []) ? tabs : find(tabs)?.children ?? [];
  return [...headings, ...children.filter((child) => child.href !== url).map(({ label, href }) => ({ label, href, description: docDescription(lang, href) }))];
}

/**
 * A page of the site's tree: a tab, or a page nested under one in
 * navigation.md, with the pages nested under it in turn. A link titled
 * with a label, such as "Parts of an Agent", starts a group: its
 * `group` is that label, in the reader's language, drawn above it.
 */
export type Node = Entry & { children: Node[]; ref?: boolean; group?: string; tab?: boolean };

export type Site = {
  name: string;
  description: string;
  tabs: Node[];
  words: Record<string, string>;
};

/** Shared interface labels, English with an optional language override. */
function labels(lang: Language): Record<string, string> {
  return keep(`labels ${lang}`, () => {
    const read = (language: string): Record<string, string> => {
      const path = join(localesRoot, `${language}.json`);
      const value: unknown = JSON.parse(readFileSync(/*turbopackIgnore: true*/ path, "utf8"));
      if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error(`${path}: invalid UI catalog`);
      const entries = Object.entries(value).filter(([key]) => key !== "$source");
      if (entries.some(([, text]) => typeof text !== "string" || !text.trim())) throw new Error(`${path}: UI labels must be nonempty strings`);
      return Object.fromEntries(entries);
    };
    const english = read(defaultLanguage);
    if (lang === defaultLanguage || !existsSync(/*turbopackIgnore: true*/ join(localesRoot, `${lang}.json`))) return english;
    const own = read(lang);
    for (const key of Object.keys(own)) if (!(key in english)) throw new Error(`${lang}: unknown UI label ${key}`);
    return { ...english, ...own };
  });
}

/** A language's structural outline, with metadata read from its homepage. */
export function site(lang: Language): Site {
  return keep(`site ${lang}`, () => {
    const tree = readOutline(siteFileOf(lang), siteFileOf(defaultLanguage), (text) => markdown.parse(text));
    const heading = tree.children.find((n) => n.type === "heading" && n.depth === 1);
    if (!heading || !toString(heading).trim()) throw new Error(`${siteFile}: missing site name`);
    const at = tree.children.findIndex((n) => n.type === "heading" && n.depth === 2 && toString(n) === "Pages");
    const outline = tree.children.slice(at + 1).find((n) => n.type === "list" || n.type === "heading");
    if (at < 0 || outline?.type !== "list") throw new Error(`${siteFile}: missing Pages outline`);
    const words = labels(lang);
    const nodes = (list: List): Node[] => list.children.flatMap((item) => {
      const para = item.children[0];
      const links = para?.type === "paragraph" ? para.children.filter((c) => c.type === "link") : [];
      const link = links[0];
      if (links.length !== 1 || !link || link.type !== "link" || !toString(link).trim()) throw new Error(`${siteFile}: each page needs one labeled link`);
      const url = link.url;
      if (!/^(?:[a-z][a-z+.-]*:|\/|#)/i.test(url)) {
        const english = normalize(join(docsRoot, dirname(siteFile), url.split("#")[0]));
        const target = translated(english, lang);
        const file = inDocs(lang, target) ?? inDocs(defaultLanguage, english);
        if (file !== undefined && !published(file)) return [];
        if (!existsSync(/*turbopackIgnore: true*/ target) && !existsSync(/*turbopackIgnore: true*/ english)) throw new Error(`${siteFile}: missing page ${url}`);
        // A held page, and the pages placed under it, aren't the site's.
        if (isHeld(held, lang, target) || isHeld(held, defaultLanguage, english)) return [];
      }
      const nested = item.children.find((c): c is List => c.type === "list");
      const href = resolve(lang, siteFile, url);
      const title = link.title ?? undefined;
      const group = title && title !== "tab" && title !== "include" ? (words[title] ?? title) : undefined;
      return href === undefined ? [] : [{ label: toString(link), href, children: nested ? nodes(nested) : [], ...(group ? { group } : {}), ...(title === "tab" ? { tab: true } : {}) }];
    });
    const homeLanguage = where(lang).has("README.md") ? lang : defaultLanguage;
    const home = parse(homeLanguage, "README.md");
    const introduction = opening(home);
    return {
      name: toString(heading),
      description: metadata(home).description || (introduction ? flat(plainText(introduction)) : ""),
      tabs: nodes(outline),
      words,
    };
  });
}

/** One shared UI label, with missing keys treated as configuration errors. */
export function word(s: Site, key: string): string {
  const value = s.words[key];
  if (value === undefined) throw new Error(`missing UI label: ${key}`);
  return value;
}

/** The home page's headline, its hook, when it has one: "Sovereign Superintelligence". */
export function headline(lang: Language): string | undefined {
  return page(lang, [])?.hook;
}

/**
 * What the home page is called, in its tab and wherever a link to the
 * site is shared: the site's name and the home page's headline, or the
 * name alone.
 */
export function shareTitle(lang: Language): string {
  const name = site(lang).name;
  const hook = headline(lang);
  return hook ? `${name} — ${hook}` : name;
}

/** The share card's path, as the [lang] segment's opengraph-image serves it (share.tsx). */
export function shareImage(lang: Language): string {
  return `${lang === defaultLanguage ? "" : `/${lang}`}/opengraph-image/share`;
}

/**
 * Shared across renders. Development checks file versions once per React
 * server render, including additions and deletions. A change drops derived
 * indexes, retaining parsed Markdown for files that have not changed.
 */
const kept = new Map<string, unknown>();
const sources = new Map<string, Source>();
let versions = new Map<string, string>();
let revision = "";
const refresh = cache(() => {
  if (process.env.NODE_ENV === "production") return;
  const { own, others } = split();
  const roots = own.folders;
  const langs = readLanguages();
  const stamps = new Map<string, string>();
  const indexes = new Map(langs.map((lang) => [lang, walk(lang, roots, stamps, own.held, own)]));
  for (const name of readdirSync(/*turbopackIgnore: true*/ localesRoot).sort()) {
    if (!name.endsWith(".json")) continue;
    const path = join(localesRoot, name);
    const stat = statSync(/*turbopackIgnore: true*/ path);
    stamps.set(path, `${stat.mtimeMs}:${stat.ctimeMs}:${stat.size}:${stat.ino}`);
  }
  const next = JSON.stringify([own, others, langs, [...stamps]]);
  if (next === revision) return;
  for (const path of sources.keys()) {
    if (versions.get(path) !== stamps.get(path) || currentWebsite.homeFile !== own.homeFile) sources.delete(path);
  }
  kept.clear();
  listed.clear();
  currentWebsite = own;
  for (const [lang, index] of indexes) listed.set(lang, withoutHeld(lang, index, roots, own.held));
  folders = roots;
  held = own.held;
  elsewhere = others;
  sectionPages = own.sectionPages;
  availableLanguages = langs;
  versions = stamps;
  revision = next;
});

function keep<T>(key: string, make: () => T): T {
  refresh();
  if (!kept.has(key)) kept.set(key, make());
  return kept.get(key) as T;
}

/** A page's title: its first `#` heading, in English while it isn't translated. */
function titleOf(lang: Language, file: string): string {
  return keep(`title ${lang} ${file}`, () => {
    const from = where(lang).has(file) ? lang : defaultLanguage;
    const first = source(from, file).text.split("\n", 1)[0];
    // A first-line title needs only inline parsing. References can depend
    // on definitions later in the file, so those use the complete parser,
    // as do setext titles and documents that start with other content.
    const tree = /^ {0,3}#(?:[\t ]|$)/.test(first) && !first.includes("[") ? markdown.parse(first) : parse(from, file);
    const heading = tree.children.find((n) => n.type === "heading" && n.depth === 1);
    return metadata(tree).title || (heading ? toString(heading) : "");
  });
}

/** The authored page outline, validated without discovering implicit children. */
export function tree(lang: Language): Node[] {
  return keep(`tree ${lang}`, () => {
    const s = site(lang);
    const tabs = structuredClone(s.tabs);
    const allFiles = new Set([...files(defaultLanguage), ...files(lang)]);
    const fileAtUrl = new Map([...allFiles].map((file) => [pageUrl(lang, file)!, file]));
    const placed = new Set<string>();
    const visit = (nodes: Node[], nested: boolean) => nodes.forEach((node) => {
      if (placed.has(node.href)) throw new Error(`${siteFile}: duplicate page ${node.href}`);
      placed.add(node.href);
      const file = fileAtUrl.get(node.href);
      if (node.href.startsWith("/") && !file) throw new Error(`${siteFile}: missing page ${node.href}`);
      if ((nested || node.children.length > 0) && file && node.href !== tabs[0]?.href && !node.tab && node.label !== titleOf(lang, file)) {
        throw new Error(`${siteFile}'s ## Pages calls ${file} ${node.label}, not its title ${titleOf(lang, file)}`);
      }
      visit(node.children, true);
      // This navigation affordance points to the section's already-authored
      // page; it does not discover another document or change authored order.
      const opening = file && node.children.length ? source(where(lang).has(file) ? lang : defaultLanguage, file).text.split(/^## /m)[0] : "";
      const task = !documentation || opening.includes("<!-- quickstart -->") || opening.includes("```sh\n");
      const reference = file?.startsWith("config/") && file !== "config/README.md";
      if (file && node.children.length && node.href !== tabs[0]?.href && task && !reference) {
        const own = sectionPages === "heading" ? page(lang, segmentsOf(lang, file)) : undefined;
        node.children.unshift({ label: own?.hook ?? (own ? titleOf(lang, file) : word(s, "Get Started")), href: node.href, children: [] });
      }
    });
    visit(tabs, false);
    const missing = [...fileAtUrl.keys()].filter((url) => !placed.has(url));
    if (missing.length) throw new Error(`pages in no tab: persist them in ${siteFile}'s ## Pages: ${missing.join(", ")}`);
    return tabs;
  });
}

/**
 * Every page's place in the tree: the index of each node from its tab
 * down to it, the deepest where a page is named twice (a section's
 * README is the section, then its Get Started).
 */
export function places(lang: Language): Record<string, number[]> {
  return keep(`places ${lang}`, () => {
    const found: Record<string, number[]> = {};
    const visit = (nodes: Node[], at: number[]) =>
      nodes.forEach((node, i) => {
        const path = [...at, i];
        if (!found[node.href] || found[node.href].length < path.length) found[node.href] = path;
        visit(node.children, path);
      });
    visit(tree(lang), []);
    return found;
  });
}

/**
 * A page's name in a browser tab: its title, after the page it's under
 * when that isn't a tab, so many a Security tells which it is
 * ("OpenClaw: Security").
 */
export function fullTitle(lang: Language, url: string, title: string): string {
  const path = places(lang)[url];
  if (!path || path.length < 3) return title;
  let nodes = tree(lang);
  let parent: Node | undefined;
  for (const i of path.slice(0, -1)) {
    parent = nodes[i];
    nodes = parent.children;
  }
  return parent ? `${parent.label}: ${title}` : title;
}

/** The node at each step of a page's place, from its tab down. */
function nodesAt(lang: Language, url: string): Node[] {
  const found: Node[] = [];
  let nodes = tree(lang);
  for (const i of places(lang)[url] ?? []) {
    found.push(nodes[i]);
    nodes = nodes[i].children;
  }
  return found;
}

/**
 * Where a page is, above its title: the sections it's in, from its tab
 * down (Machines, Boxes for Box Spec, and for Boxes's own page too).
 */
export function trail(lang: Language, url: string): Entry[] {
  const at = nodesAt(lang, url);
  if (at.length < 2) return at.length && url !== urlOf(lang, []) ? [{ label: at[0].label, href: at[0].href }] : [];
  return at.slice(0, -1).map(({ label, href }) => ({ label, href }));
}

/**
 * The pages before and after one in its tab, read in the tree's order: a
 * section by its name (Boxes), every other page by its title.
 */
export function neighbors(lang: Language, url: string): { previous?: Entry; next?: Entry } {
  const at = nodesAt(lang, url);
  if (!at.length || !at[0].children.length || url === urlOf(lang, [])) return {};
  const order: Entry[] = [];
  const seen = new Set<string>();
  const walk = (node: Node) => {
    if (!seen.has(node.href)) {
      seen.add(node.href);
      order.push({ label: node.label, href: node.href });
    }
    node.children.forEach(walk);
  };
  walk(at[0]);
  const i = order.findIndex((e) => e.href === url);
  return { previous: order[i - 1], next: order[i + 1] };
}
