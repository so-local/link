// What counts as a translation, for every website and script: a page's
// `<page>.<language>.md` beside its English `<page>.md`, its language one
// of packages/component/languages.toml's, alone or with a region in
// capitals (`pt-BR`). That list is the one Link's Rust reads too
// (`link_component::languages`); English is never a translation.

import { readFileSync } from "node:fs";

let listed;

/** The languages a page may be translated into, read once from the list. */
export function languages() {
  if (!listed) {
    const text = readFileSync(new URL("../../../../packages/component/languages.toml", import.meta.url), "utf8");
    const list = text.replace(/#.*$/gm, "").match(/^languages\s*=\s*\[([^\]]*)\]/m);
    if (!list) throw new Error("languages.toml lists no languages");
    listed = [...list[1].matchAll(/"([^"]+)"/g)].map((match) => match[1]);
  }
  return listed;
}

/** Whether `name` is a translation's language: `es`, or `pt-BR`. */
export function isLanguage(name) {
  const [base, region, ...rest] = name.split("-");
  return rest.length === 0 && languages().includes(base) && (region === undefined || /^[A-Z]{2}$/.test(region));
}

/** A translation's language, from its file name: guide.es.md is es, foo.js.md none. */
export function translationLanguage(name) {
  const language = name.match(/\.([^./]+)\.md$/)?.[1];
  return language !== undefined && isLanguage(language) ? language : undefined;
}
