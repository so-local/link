#!/usr/bin/env node
// Translates the docs: each folder's docs/ English pages are the one set
// people write, and this writes each page's <page>.<language>.md beside
// it with a model, through OpenAI's Batch API (half the price of calling
// it directly; results within a day). The folders are the website's, as
// it reads them (folders.mjs), run from its app's folder; a page is named
// by its path in its folder's docs/.
//
//   node scripts/translate.mjs plan              what's missing, behind or left over
//   node scripts/translate.mjs submit [--out f]  send one batch (or write its requests to f)
//   node scripts/translate.mjs status            this language's batches
//   node scripts/translate.mjs collect [--batch id | --file f]
//                                                apply the newest finished batch (or f)
//
// Needs OPENAI_API_KEY to submit, see batches or collect from them.
// LNK_TRANSLATE_LANG picks the language (es), LNK_TRANSLATE_MODEL the
// model. Every page it writes starts with a comment naming its English
// file and that file's hash, so only what changed is sent again, and a
// result for English that has changed since is never applied.

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, normalize, posix, relative, sep } from "node:path";
import GithubSlugger from "github-slugger";
import { websiteFolders, websiteHomepage } from "./folders.mjs";
import { translationLanguage } from "../src/languages.mjs";

const web = process.cwd();
const repo = join(web, "../../..");
const docs = join(web, "docs");
const uiRoot = join(repo, "web/packages/site/locales");
const uiEnglish = join(uiRoot, "en.json");

/** The languages this translates into, and how the model should write each. */
const LANGUAGES = {
  es: "neutral Latin American Spanish, addressing the reader as tú",
};
const lang = process.env.LNK_TRANSLATE_LANG ?? "es";
const uiTranslated = join(uiRoot, `${lang}.json`);
const model = process.env.LNK_TRANSLATE_MODEL ?? "gpt-6-luna";
const api = "https://api.openai.com/v1";
const app = "local-link-docs";


/** Names kept exactly as written, in every language. */
const GLOSSARY = [
  "Local Link", "Link", "Link Harness", "Link Memory", "lnk", "local.link",
  "OpenClaw", "Hermes", "Hermes Agent", "DeepSeek Harness", "Ollama", "LM Studio",
  "rclone", "Telegram", "Slack", "Discord", "Signal", "GitHub",
  "AWS", "Google Cloud", "OpenAI", "Anthropic", "OpenRouter",
];

if (!LANGUAGES[lang]) fail(`no such language: ${lang} (one of ${Object.keys(LANGUAGES).join(", ")})`);

function fail(message) {
  console.error(`translate: ${message}`);
  process.exit(1);
}

/** The folders whose docs are the site, as the website reads them. */
function folders() {
  try {
    return websiteFolders(web);
  } catch (e) {
    fail(e.message);
  }
}

/** Every English page, by its path in its folder's docs/, to that folder. */
let owners;
function englishPages() {
  if (!owners) {
    owners = new Map();
    for (const folder of folders()) {
      for (const file of files(join(folder, "docs")).filter((f) => !translation(f))) {
        if (owners.has(file)) fail(`two folders hold ${file}`);
        owners.set(file, folder);
      }
    }
    if (websiteHomepage(web)) owners.set("README.md", web);
  }
  return owners;
}

/** A page's English, and its translation beside it. */
const en = (file) => file === "README.md" && websiteHomepage(web) || join(englishPages().get(file) ?? web, "docs", file);
const tr = (file) => join(englishPages().get(file) ?? web, "docs", file.replace(/\.md$/, `.${lang}.md`));
/** A translation's language, from its name: guide.es.md is es. */
const translation = translationLanguage;
const shown = (path) => relative(repo, path).split(sep).join("/");

const hash = (text) => createHash("sha256").update(text).digest("hex").slice(0, 16);
const read = (path) => readFileSync(path, "utf8");
const navigation = (file) => file === "navigation.md" || /^navigation\/[^/]+\.md$/.test(file);

/** Every Markdown file under a folder, as paths relative to it. */
function files(root) {
  if (!existsSync(root)) return [];
  const found = [];
  const walk = (dir) => {
    for (const name of readdirSync(dir).sort()) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (name.endsWith(".md")) found.push(relative(root, path).split(sep).join("/"));
    }
  };
  walk(root);
  return found;
}

const HEADER = /^<!-- Translated from (\S+) \(([0-9a-f]{16})\)[^\n]*-->\n/;

function header(file, sourceHash) {
  return `<!-- Translated from ${posix.basename(file)} (${sourceHash}) by link-public/web/packages/site/scripts/translate.mjs: edit the English, not this. -->\n`;
}

/** Each file's state: missing, behind or current; and the left-over ones. */
function plan() {
  const pages = [...englishPages().keys()].map((file) => {
    const sourceHash = hash(read(en(file)));
    const path = tr(file);
    const recorded = existsSync(path) ? read(path).match(HEADER)?.[2] : undefined;
    const state = !recorded ? "missing" : recorded === sourceHash ? "current" : "behind";
    return { file, sourceHash, state };
  });
  const known = new Set(pages.map((p) => p.file));
  // Translations whose English is gone, as paths on disk.
  const leftOver = folders().flatMap((folder) => {
    const root = join(folder, "docs");
    return files(root)
      .filter((f) => translation(f) === lang)
      .filter((f) => {
        const english = f.replace(/\.[^.]+\.md$/, ".md");
        return !known.has(english) || englishPages().get(english) !== folder;
      })
      .map((f) => join(root, f));
  });
  return { pages, leftOver };
}

// ---- Markdown, line by line: enough to check and relink a translation.

/** Lines outside fenced code blocks, and the blocks' bodies. */
function split(text) {
  const prose = [];
  const blocks = [];
  let block = null;
  for (const line of text.split("\n")) {
    if (line.trimStart().startsWith("```")) {
      if (block === null) block = [];
      else {
        blocks.push(block.join("\n"));
        block = null;
      }
      continue;
    }
    if (block === null) prose.push(line);
    else block.push(line);
  }
  return { prose, blocks };
}

function headings(text) {
  return split(text).prose.filter((l) => /^#{1,6} /.test(l));
}

/** A heading's text as GitHub sees it, for its anchor. */
function plain(heading) {
  return heading
    .replace(/^#+\s*/, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[`*_]/g, "")
    .trim();
}

function anchors(text) {
  const slugger = new GithubSlugger();
  return headings(text).map((h) => slugger.slug(plain(h)));
}

/** The link targets outside code, in order. */
function links(text) {
  const found = [];
  for (const line of split(text).prose) {
    const outside = line.replace(/`[^`]*`/g, "");
    for (const m of outside.matchAll(/\]\(([^)\s]+)/g)) found.push(m[1]);
  }
  return found;
}

// ---- Requests.

function system(file) {
  const rules = [
    `You translate Markdown documentation for Local Link, a command-line tool (\`lnk\`), from English into ${LANGUAGES[lang]}.`,
    "Answer with the translated Markdown only, nothing before or after it, and not wrapped in a code block.",
    "Keep exactly as they are: everything inside code blocks and inline code; every link target (the part in parentheses), anchors included; HTML tags, attributes and comments; the Markdown structure, with the same headings at the same levels in the same order, the same lists and tables, and every `---` line.",
    `Keep these names as they are: ${GLOSSARY.join(", ")}.`,
    "Translate everything else, headings, link text and table cells included. Write short, plain sentences, as the English does, and keep its bold.",
  ];
  if (navigation(file)) {
    rules.push(
      "This is structural website navigation. Keep the ## Pages and ## URLs headings, link targets, list nesting and order exactly as they are. Translate the site heading and page labels only; page labels must match the translated target page titles. Add no introduction, description or UI labels.",
    );
  }
  return rules.join("\n");
}

/** Every folder and file name a URL is made of, for the language's slugs. */
function segments() {
  const names = new Set();
  for (const file of englishPages().keys()) {
    if (navigation(file)) continue;
    for (const part of file.replace(/\.md$/, "").split("/")) if (part !== "README") names.add(part);
  }
  return [...names].sort();
}

function slugsPrompt(names) {
  return [
    `Give a URL slug in ${LANGUAGES[lang]} for each English path segment below.`,
    "Use lowercase letters a to z, digits and hyphens only, with no accents; keep it short and natural, keep product names (such as link) as they are, and make each slug different from the others.",
    "Answer with a JSON object from each English segment to its slug, and nothing else.",
    "",
    ...names,
  ].join("\n");
}

function request(id, messages, json = false) {
  const body = { model, messages };
  if (json) body.response_format = { type: "json_object" };
  return { custom_id: id, method: "POST", url: "/v1/chat/completions", body };
}

/** The shared UI catalog is translated once for all apps. */
function uiPlan() {
  const sourceHash = hash(read(uiEnglish));
  const recorded = existsSync(uiTranslated) ? JSON.parse(read(uiTranslated)).$source : undefined;
  return { sourceHash, state: recorded === sourceHash ? "current" : recorded ? "behind" : "missing" };
}

function uiRequest() {
  const { sourceHash, state } = uiPlan();
  if (state === "current") return;
  return request(`ui:${sourceHash}`, [
    { role: "system", content: `Translate these shared UI labels into ${LANGUAGES[lang]}. Return a JSON object with exactly the same keys, translating only the values. Language must name the target language in its own language. Keep product names unchanged.` },
    { role: "user", content: read(uiEnglish) },
  ], true);
}

/** The batch's requests: each page that's missing or behind, and the slugs. */
function requests() {
  const { pages } = plan();
  const out = [];
  for (const { file, sourceHash, state } of pages) {
    if (state === "current") continue;
    const text = read(en(file));
    out.push(
      request(`doc:${sourceHash}:${file}`, [
        { role: "system", content: system(file) },
        { role: "user", content: text },
      ]),
    );
  }
  const ui = uiRequest();
  if (ui) out.push(ui);
  const names = segments();
  const known = urls();
  if (out.length || names.some((n) => !known[n])) {
    out.push(request(`slugs:${hash(names.join("\n"))}`, [{ role: "user", content: slugsPrompt(names) }], true));
  }
  return out;
}

// ---- The language's URLs, in its navigation.<language>.md.

function urls() {
  const path = join(docs, `navigation.${lang}.md`);
  if (!existsSync(path)) return {};
  const text = read(path);
  const at = text.indexOf("\n## URLs\n");
  if (at < 0) return {};
  const map = {};
  for (const line of text.slice(at + 9).split("\n")) {
    if (line.startsWith("## ")) break;
    const m = line.match(/^- ([^:]+): (\S+)$/);
    if (m) map[m[1].trim()] = m[2].trim();
  }
  return map;
}

function withUrls(text, map) {
  const at = text.indexOf("\n## URLs\n");
  const body = at < 0 ? text.trimEnd() : text.slice(0, at).trimEnd();
  const lines = Object.keys(map)
    .sort()
    .map((k) => `- ${k}: ${map[k]}`);
  return `${body}\n\n## URLs\n\n${lines.join("\n")}\n`;
}

// ---- The API.

async function call(path, init = {}) {
  const key = process.env.OPENAI_API_KEY;
  if (!key) fail("set OPENAI_API_KEY");
  const res = await fetch(`${api}${path}`, {
    ...init,
    headers: { Authorization: `Bearer ${key}`, ...(init.headers ?? {}) },
  });
  if (!res.ok) fail(`${init.method ?? "GET"} ${path}: ${res.status} ${await res.text()}`);
  return res;
}

async function batches() {
  const res = await call("/batches?limit=100");
  const { data } = await res.json();
  return data
    .filter((b) => b.metadata?.app === app && b.metadata?.lang === lang)
    .sort((a, b) => b.created_at - a.created_at);
}

// ---- Applying results.

function strip(content) {
  const m = content.trim().match(/^```(?:markdown|md)?\n([\s\S]*)\n```$/);
  return `${(m ? m[1] : content).trim()}\n`;
}

/** Why a translation can't stand for its English, or nothing. */
function check(en, translated) {
  const [a, b] = [headings(en).length, headings(translated).length];
  if (a !== b) return `${b} headings, not ${a}`;
  const [x, y] = [split(en).blocks, split(translated).blocks];
  if (x.length !== y.length || x.some((block, i) => block !== y[i])) return "its code blocks differ from the English";
  return undefined;
}

/** Navigation translations change labels, never structure or targets. */
function checkNavigation(english, translated) {
  const shape = (text) => text.split("\n").filter((line) => line.trim()).map((line) => {
    if (/^# [^#]/.test(line)) return "#";
    if (/^## (Pages|URLs)$/.test(line)) return line;
    const item = line.match(/^(\s*)- \[[^\]]+\]\(([^)]+)\)\s*$/);
    return item ? `${item[1]}- (${item[2]})` : undefined;
  });
  const a = shape(english);
  const b = shape(translated);
  return b.includes(undefined) || JSON.stringify(a) !== JSON.stringify(b)
    ? "navigation structure or link targets differ from the English" : undefined;
}

/**
 * Point every link's anchor at the translated heading: the n-th link of a
 * page pairs with the n-th of its English, whose anchor names a heading of
 * the English target; the same heading in the translated target gives the
 * anchor.
 */
function relink() {
  const current = new Set(plan().pages.filter((p) => p.state === "current").map((p) => p.file));
  const pairs = {};
  const pairing = (file) => {
    if (!(file in pairs)) {
      const a = englishPages().has(file) ? anchors(read(en(file))) : [];
      const b = current.has(file) ? anchors(read(tr(file))) : [];
      pairs[file] = a.length === b.length ? Object.fromEntries(a.map((x, i) => [x, b[i]])) : {};
    }
    return pairs[file];
  };
  for (const file of current) {
    const path = tr(file);
    const text = read(path);
    const source = links(read(en(file)));
    const translated = links(text);
    if (source.length !== translated.length) continue;
    let i = 0;
    const out = text.split("\n").map((line) => line);
    let inBlock = false;
    for (let n = 0; n < out.length; n++) {
      if (out[n].trimStart().startsWith("```")) {
        inBlock = !inBlock;
        continue;
      }
      if (inBlock) continue;
      out[n] = out[n].replace(/(`[^`]*`)|\]\(([^)\s]+)/g, (whole, code, url) => {
        if (code) return whole;
        const original = source[i++];
        const [path0, anchor] = original.split("#", 2);
        if (anchor === undefined || /^[a-z][a-z+.-]*:/i.test(original)) return `](${url}`;
        const page = path0 ? normalize(join(dirname(file), path0)).split(sep).join("/") : file;
        const md = page.endsWith(".md") ? page : `${page.replace(/\/$/, "")}/README.md`;
        const mapped = pairing(md)[anchor];
        return mapped ? `](${url.split("#")[0]}#${mapped}` : `](${url}`;
      });
    }
    writeFileSync(path, out.join("\n"));
  }
}

function apply(lines) {
  const sources = new Map(plan().pages.map((p) => [p.file, p.sourceHash]));
  let written = 0;
  const problems = [];
  let slugs;
  for (const line of lines) {
    const result = JSON.parse(line);
    const id = result.custom_id;
    const content = result.response?.body?.choices?.[0]?.message?.content;
    if (result.error || result.response?.status_code !== 200 || typeof content !== "string") {
      problems.push(`${id}: ${JSON.stringify(result.error ?? result.response?.body?.error ?? "no answer")}`);
      continue;
    }
    if (id.startsWith("slugs:")) {
      slugs = content;
      continue;
    }
    if (id.startsWith("ui:")) {
      try {
        if (id !== `ui:${uiPlan().sourceHash}`) throw new Error("the English UI catalog has changed since");
        const translated = JSON.parse(content);
        const english = JSON.parse(read(uiEnglish));
        const keys = Object.keys(english).sort();
        if (!translated || typeof translated !== "object" || Array.isArray(translated) || JSON.stringify(Object.keys(translated).sort()) !== JSON.stringify(keys)) throw new Error("UI keys differ from the English catalog");
        if (Object.values(translated).some((value) => typeof value !== "string" || !value.trim())) throw new Error("UI labels must be nonempty strings");
        mkdirSync(uiRoot, { recursive: true });
        writeFileSync(uiTranslated, JSON.stringify({ $source: uiPlan().sourceHash, ...translated }, null, 2) + "\n");
      } catch (error) {
        problems.push(`${id}: ${error.message}; UI catalog not written`);
      }
      continue;
    }
    const [, sourceHash, file] = id.match(/^doc:([0-9a-f]{16}):(.+)$/) ?? [];
    if (!file || sources.get(file) !== sourceHash) {
      problems.push(`${id}: its English has changed since; left for the next batch`);
      continue;
    }
    const english = read(en(file));
    let translated = strip(content);
    if (file === "navigation.md") translated = withUrls(translated, urls());
    const body = translated.replace(/\n## URLs\n[\s\S]*$/, "\n");
    const wrong = navigation(file) ? checkNavigation(english, body) : check(english, body);
    if (wrong) {
      problems.push(`${shown(tr(file))}: ${wrong}; not written`);
      continue;
    }
    mkdirSync(dirname(tr(file)), { recursive: true });
    writeFileSync(tr(file), header(file, sourceHash) + translated);
    written++;
  }
  if (slugs) {
    try {
      const map = JSON.parse(slugs);
      const ok = {};
      const used = new Set();
      for (const name of segments()) {
        const slug = String(map[name] ?? "");
        if (/^[a-z0-9]+(-[a-z0-9]+)*$/.test(slug) && !used.has(slug)) {
          ok[name] = slug;
          used.add(slug);
        } else problems.push(`slug for ${name}: "${slug}" isn't one; the English is kept`);
      }
      const path = join(docs, `navigation.${lang}.md`);
      if (existsSync(path)) writeFileSync(path, withUrls(read(path), { ...urls(), ...ok }));
      else problems.push(`no docs/navigation.${lang}.md yet to keep the slugs in`);
    } catch {
      problems.push("the slugs weren't JSON; the English names are kept");
    }
  }
  for (const path of plan().leftOver) rmSync(path);
  relink();
  console.log(`translate: wrote ${written} ${lang} pages beside their English in each folder's docs/`);
  for (const p of problems) console.log(`  ${p}`);
}

// ---- Commands.

const [command, ...args] = process.argv.slice(2);
const option = (name) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};

switch (command) {
  case "plan": {
    const { pages, leftOver } = plan();
    for (const state of ["missing", "behind", "current"]) {
      const list = pages.filter((p) => p.state === state);
      console.log(`${state}: ${list.length}`);
      if (state !== "current") for (const p of list) console.log(`  ${shown(en(p.file))}`);
    }
    console.log(`shared UI (${lang}): ${uiPlan().state}`);
    if (leftOver.length) console.log(`left over (deleted on collect): ${leftOver.map(shown).join(", ")}`);
    break;
  }
  case "submit": {
    const reqs = requests();
    if (!reqs.length) {
      console.log(`translate: every ${lang} page is current in every folder; nothing to send`);
      break;
    }
    const jsonl = reqs.map((r) => JSON.stringify(r)).join("\n") + "\n";
    const out = option("--out");
    if (out) {
      writeFileSync(out, jsonl);
      console.log(`translate: ${reqs.length} requests in ${out}`);
      break;
    }
    const form = new FormData();
    form.append("purpose", "batch");
    form.append("file", new Blob([jsonl], { type: "application/jsonl" }), `docs-${lang}.jsonl`);
    const file = await (await call("/files", { method: "POST", body: form })).json();
    const batch = await (
      await call("/batches", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          input_file_id: file.id,
          endpoint: "/v1/chat/completions",
          completion_window: "24h",
          metadata: { app, lang, commit: process.env.GITHUB_SHA ?? "" },
        }),
      })
    ).json();
    console.log(`translate: batch ${batch.id}, ${reqs.length} requests, ${model}; collect it once it's done`);
    break;
  }
  case "status": {
    for (const b of await batches()) {
      const done = b.request_counts ? ` ${b.request_counts.completed}/${b.request_counts.total}` : "";
      console.log(`${b.id} ${b.status}${done} ${new Date(b.created_at * 1000).toISOString()}`);
    }
    break;
  }
  case "collect": {
    const local = option("--file");
    if (local) {
      apply(read(local).split("\n").filter(Boolean));
      break;
    }
    const wanted = option("--batch");
    const list = await batches();
    const batch = wanted ? list.find((b) => b.id === wanted) : list.find((b) => b.status === "completed");
    if (!batch || batch.status !== "completed") {
      console.log(`translate: no finished batch for ${lang}${list[0] ? ` (newest: ${list[0].id}, ${list[0].status})` : ""}`);
      break;
    }
    const text = await (await call(`/files/${batch.output_file_id}/content`)).text();
    apply(text.split("\n").filter(Boolean));
    break;
  }
  default:
    console.log(read(new URL(import.meta.url)).split("\n").slice(1, 13).join("\n").replace(/^\/\/ ?/gm, ""));
    process.exit(command ? 1 : 0);
}
