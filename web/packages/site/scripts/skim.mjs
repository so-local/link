// The site as a reader skims it, a level at a time: the home page's
// title and first paragraph; then each tab's; then each page's under
// them, in the tree's order; then each page's `##` headings, with the
// first sentence under each. Every level should read as a true, whole
// picture on its own (tests/docs/docs/more/developing/writing-docs.md, "Read it
// a level at a time"). Run from an app's folder (web/apps/docs):
//
//   node ../../packages/site/scripts/skim.mjs           # every level
//   node ../../packages/site/scripts/skim.mjs 2         # down to the tabs' pages
//   node ../../packages/site/scripts/skim.mjs 3 Agents  # one tab, down to its headings

import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { websiteFolders, websiteHomepage } from "./folders.mjs";

const web = process.cwd();
const site = readFileSync(join(web, "docs/navigation.md"), "utf8");
const depth = Number(process.argv[2] ?? 3);
const only = process.argv[3];

// The folders whose docs make the site.
const folders = websiteFolders(web);

/** A page's file, by its URL or a path relative to this folder's docs. */
function fileOf(href) {
  const homepage = websiteHomepage(web);
  if (homepage && href.endsWith("README.md") && existsSync(homepage)) {
    const target = join(web, "docs", href);
    if (target === homepage) return homepage;
  }
  const path = href.replace(/^https:\/\/www\.local\.link/, "").replace(/^\//, "").replace(/\.md$/, "").replace(/(^|\/)README$/, "");
  if (!path && homepage) return homepage;
  for (const folder of folders) {
    for (const f of [`${path}.md`, `${path}/README.md`, path ? null : "README.md"]) {
      if (f && existsSync(join(folder, "docs", f))) return join(folder, "docs", f);
    }
  }
}

/** The tree under `## Pages`: each line's label and link, and what's nested under it. */
function tree() {
  const start = site.indexOf("\n## Pages\n");
  const rest = site.slice(start + "\n## Pages\n".length);
  const block = rest.split(/\n## /, 1)[0];
  const root = { children: [] };
  const stack = [{ indent: -1, node: root }];
  for (const line of block.split("\n")) {
    const m = line.match(/^(\s*)- \[([^\]]+)\]\(([^)]+)\)/);
    if (!m) continue;
    const node = { label: m[2], href: m[3], children: [] };
    while (stack.at(-1).indent >= m[1].length) stack.pop();
    stack.at(-1).node.children.push(node);
    stack.push({ indent: m[1].length, node });
  }
  return root.children;
}

/** Plain text of a Markdown line: links and emphasis without their marks. */
const plain = (s) => s.replace(/\[([^\]]+)\]\([^)]*\)/g, "$1").replace(/\*\*|__/g, "").trim();

/** A page's title, its first paragraph, and each `##` with the first sentence under it. */
function read(file) {
  const lines = readFileSync(file, "utf8").split("\n");
  let fence = false;
  let comment = false;
  let title = "";
  let opening = "";
  const sections = [];
  let para = [];
  const end = () => {
    const text = plain(para.join(" "));
    para = [];
    if (!text) return;
    if (!sections.length && !opening) opening = text;
    else if (sections.length && !sections.at(-1).first) sections.at(-1).first = text.match(/^.*?[.!?](?=\s|$)/)?.[0] ?? text;
  };
  for (const line of lines) {
    if (line.startsWith("```")) {
      fence = !fence;
      end();
      continue;
    }
    if (fence) continue;
    // A comment, of one line or many, is no one's to read.
    if (comment || line.trimStart().startsWith("<!--")) {
      end();
      comment = !line.includes("-->");
      continue;
    }
    if (line.startsWith("# ")) title = line.slice(2).trim();
    else if (line.startsWith("## ")) {
      end();
      sections.push({ heading: line.slice(3).trim(), first: "" });
    } else if (!line.trim() || /^(#|\||- |\d+\. |---)/.test(line)) end();
    else para.push(line);
  }
  end();
  return { title, opening, sections };
}

const tabs = tree();
const out = [];
const words = [0, 0, 0, 0];
const say = (level, text) => {
  out.push(text);
  words[level] += text.split(/\s+/).filter(Boolean).length;
};

// Level 0: the home page.
const home = read(fileOf(tabs[0].href));
say(0, `# ${home.title}\n\n${home.opening}\n`);

// Level 1: each tab's own page. Level 2: each page under it, in order.
// Level 3: each page's headings, with their first sentences.
for (const tab of tabs.slice(1)) {
  if (only && tab.label !== only) continue;
  const file = fileOf(tab.href);
  if (!file) continue;
  const page = read(file);
  say(1, `\n## ${tab.label}\n\n${page.opening}\n`);
  if (depth >= 3) for (const s of page.sections) say(3, `  - ${s.heading}: ${s.first}`);
  if (depth < 2) continue;
  const walk = (nodes, indent) => {
    for (const node of nodes) {
      const f = fileOf(node.href);
      if (!f) continue;
      const p = read(f);
      say(2, `\n${"#".repeat(indent)} ${node.label}\n\n${p.opening}\n`);
      if (depth >= 3) for (const s of p.sections) say(3, `  - ${s.heading}: ${s.first}`);
      walk(node.children, indent + 1);
    }
  };
  walk(tab.children, 3);
}

console.log(out.join("\n"));
console.error(`\nwords by level: home ${words[0]}, tabs ${words[1]}, pages ${words[2]}, headings ${words[3]}`);
