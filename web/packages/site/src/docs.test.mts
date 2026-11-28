import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative } from "node:path";
import { after, beforeEach, test } from "node:test";
import type * as Docs from "./docs";

const previous = process.cwd();
const environment = process.env.NODE_ENV;
const root = mkdtempSync(join(tmpdir(), "link-docs-"));
const web = join(root, "web/apps/first");
mkdirSync(web, { recursive: true });
mkdirSync(join(root, "scripts"));
process.chdir(web);
Object.assign(process.env, { NODE_ENV: "development" });

const website = "# Example\n\n## Pages\n\n- [Home](https://www.local.link/)\n- [Agents](https://www.local.link/agents)\n  - [Guide](agents/guide.md)\n";
const catalogs = join(root, "web/packages/site/locales");
const englishLabels = readFileSync(new URL("../locales/en.json", import.meta.url), "utf8");
// This app is a website whose pages are the index's folders'; the other
// website is web/apps/other's alone.
function index(directories: (string | { directory: string; released: false; preview?: true })[], developmentPages: string[] = [], sectionPages?: "heading", hiddenPages: string[] = []) {
  const sites = [
    { name: "home", url: "https://www.local.link", local: "http://localhost:3000", directory: "web/apps/first", tree: "open", roots: true, developmentPages, hiddenPages, sectionPages },
    { name: "other", url: "https://docs.local.link", local: "http://localhost:3001", directory: "web/apps/other", tree: "folded" },
  ];
  writeFileSync(join(root, "scripts/documentation-index.json"), JSON.stringify({ schema: 1, sites, roots: directories.map((d) => (typeof d === "string" ? { directory: d } : d)) }));
}
function put(file: string, text: string) {
  const path = join(web, "docs", file);
  mkdirSync(join(path, ".."), { recursive: true });
  writeFileSync(path, text);
}
function reset() {
  index(["web/apps/first"]);
  rmSync(join(web, "docs"), { recursive: true, force: true });
  rmSync(join(root, "other"), { recursive: true, force: true });
  rmSync(join(root, "snapshot"), { recursive: true, force: true });
  rmSync(join(root, "web/apps/other"), { recursive: true, force: true });
  rmSync(catalogs, { recursive: true, force: true });
  mkdirSync(catalogs, { recursive: true });
  writeFileSync(join(catalogs, "en.json"), englishLabels);
  put("navigation.md", website);
  put("README.md", "# Home\n\nWelcome.\n");
  put("agents/README.md", "# Agents\n\nStart an agent. [Guide](guide.md)\n\n<!-- quickstart -->\n\n## Start\n\nChoose a model.\n\n```sh\nlnk agent start\n```\n");
  put("agents/guide.md", "# Guide\n\nFirst description.\n\n## Guide\n\nOne.\n\n## Guide\n\nTwo.\n");
}
reset();
// The real loader, with its filesystem and Markdown processors. Each call
// outside React is a new validation, as each new server render is.
const docs: typeof Docs = await import(new URL("./docs.ts", import.meta.url).href);
beforeEach(reset);
after(() => {
  process.chdir(previous);
  if (environment === undefined) Reflect.deleteProperty(process.env, "NODE_ENV");
  else Object.assign(process.env, { NODE_ENV: environment });
  rmSync(root, { recursive: true, force: true });
});

test("documentation grids resolve page metadata without rendering linked grids recursively", () => {
  put("agents/README.md", '# Agents\n\nChoose a guide.\n\n---\n\n:::cards\n- [Guide](guide.md)\n:::\n');
  put("agents/guide.md", '---\ndescription: A short card introduction.\nimage: https://images.example/guide.jpg\n---\n\n# Guide\n\nA longer introduction.\n\n:::cards\n- [Agents](README.md)\n:::\n');
  const page = docs.page("en", ["agents"]);
  const grid = page?.cards.find(card => card.kind === "grid");
  assert.ok(grid);
  assert.match(JSON.stringify(grid), /"href":"\/agents\/guide"/);
  assert.match(JSON.stringify(grid), /A short card introduction/);
  assert.match(JSON.stringify(grid), /https:\/\/images.example\/guide.jpg/);
  assert.doesNotMatch(JSON.stringify(grid), /A longer introduction/);
  assert.equal(docs.page("en", ["agents", "guide"])?.image, "https://images.example/guide.jpg");
  assert.doesNotThrow(() => docs.tree("en"));
  put("agents/guide.md", '# Guide\n\nUpdated introduction.\n');
  assert.match(JSON.stringify(docs.page("en", ["agents"])?.cards), /Updated introduction/);
});

test("portable exports use canonical links and render through an independently selected content root", () => {
  mkdirSync(join(root, "web/apps/other/docs"), { recursive: true });
  writeFileSync(join(root, "web/apps/other/docs/README.md"), "# Other\n\nOther site.");
  put("agents/guide.md", '---\nimage: /cover.jpg\n---\n\n# Guide\n\nFirst description.\n');
  put("agents/README.md", '# Agents\n\n[Other](../../../other/docs/README.md).\n\n---\n\n:::cards\n- [Guide](guide.md)\n:::\n');
  const files = docs.contentFiles();
  assert.match(files["agents/README.md"], /https:\/\/docs.local.link/);
  assert.doesNotMatch(files["agents/README.md"], /localhost/);
  assert.match(files["navigation.md"], /\/agents\/guide/);
  assert.doesNotMatch(files["agents/guide.md"], /^(?:title|description):/m);
  const snapshot = join(root, "snapshot");
  for (const [file, text] of Object.entries(files)) {
    const target = join(snapshot, file);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, text.replace("First description.", "Bucket introduction."));
  }
  execFileSync(process.execPath, ["--conditions=react-server", "--import", import.meta.resolve("tsx"), "--input-type=module", "-e", `
    import assert from "node:assert/strict";
    const docs = await import(${JSON.stringify(new URL("./docs.ts", import.meta.url).href)});
    assert.equal(docs.page("en", ["agents", "guide"]).description, "Bucket introduction.");
    assert.match(JSON.stringify(docs.page("en", ["agents"]).cards), /Bucket introduction/);
    assert.doesNotThrow(() => docs.tree("en"));
  `], { cwd: web, env: { ...process.env, LINK_SITE_CONTENT_ROOT: snapshot, NODE_ENV: "production" }, stdio: "pipe" });
});

test("section outlines preserve navigation, resolve relative pages and remain metadata", () => {
  put("navigation.md", '# Example\n\n## Pages\n\n- [Home](https://www.local.link/)\n- [Agents](navigation/agents.md "include")\n');
  put("navigation/agents.md", '# Agents Navigation\n\n## Pages\n\n- [Agents](../agents/README.md)\n  - [Guide](../agents/guide.md)\n');
  const section = docs.site("en").tabs[1];
  assert.equal(section.href, "/agents");
  assert.equal(section.children[0].href, "/agents/guide");
  assert.doesNotThrow(() => docs.tree("en"));
  assert.equal(docs.page("en", ["navigation", "agents"]), undefined);
  put("navigation/agents.md", '# Agents Navigation\n\n## Pages\n\n- [Agents](../agents/README.md)\n');
  assert.equal(docs.site("en").tabs[1].children.length, 0);
  // A translated outline includes each section's translation beside it.
  put("navigation.es.md", '# Ejemplo\n\n## Pages\n\n- [Inicio](https://www.local.link/)\n- [Agents](navigation/agents.md "include")\n');
  put("navigation/agents.es.md", '# Agentes\n\n## Pages\n\n- [Agents](../agents/README.md)\n  - [Guía](../agents/guide.md)\n');
  assert.equal(docs.site("es").tabs[1].children[0].label, "Guía");
  assert.equal(docs.site("en").tabs[1].children.length, 0);
});

test("section outlines reject duplicate and nested includes", () => {
  put("navigation.md", '# Example\n\n## Pages\n\n- [Agents](navigation/agents.md "include")\n- [Agents](navigation/agents.md "include")\n');
  put("navigation/agents.md", '# Agents Navigation\n\n## Pages\n\n- [Agents](../agents/README.md)\n');
  assert.throws(() => docs.site("en"), /duplicate outline include/);
  put("navigation.md", '# Example\n\n## Pages\n\n- [Agents](navigation/agents.md "include")\n');
  put("navigation/agents.md", '# Agents Navigation\n\n## Pages\n\n- [Agents](../agents/README.md)\n  - [Nested](agents.md "include")\n');
  assert.throws(() => docs.site("en"), /never nested/);
});

test("cached pages keep links and duplicate-heading anchors, without mutating the site's source", () => {
  const before = structuredClone(docs.site("en"));
  const page = docs.page("en", ["agents", "guide"])!;
  assert.deepEqual(page.headings, [{ label: "Guide", href: "#guide-1" }, { label: "Guide", href: "#guide-2" }]);
  assert.equal(docs.page("en", ["agents", "guide"]), page);
  assert.match(JSON.stringify(docs.page("en", ["agents"])), /"href":"\/agents\/guide"/);
  const tree = docs.tree("en");
  assert.equal(docs.tree("en"), tree);
  assert.equal(tree[1].children.filter((n) => n.href === "/agents").length, 1);
  assert.deepEqual(docs.site("en"), before);
});

test("a shared homepage reads one source and resolves links from that source in development and production", () => {
  const registry = join(root, "scripts/documentation-index.json");
  const config = JSON.parse(readFileSync(registry, "utf8"));
  config.sites[0].homepage = "README.md";
  config.sites[0].assets = { "logo.png": "/icon.png" };
  writeFileSync(registry, JSON.stringify(config));
  rmSync(join(web, "docs/README.md"));
  const source = '<div align="center">\n\n<img src="logo.png" alt="Logo" width="128" height="128" onerror="bad()">\n\n# Shared Home\n\n**Own Your Own Intelligence**\n\n[Website](https://example.test/site) · [License](LICENSE)\n\nFree and open source\n\n</div>\n\n---\n\nThe shared introduction.\n\n## Install\n\n[Agents](web/apps/first/docs/agents/README.md#start) and [source](library.rs).\n\n<!-- docs-tree:start -->\n\n## Docs\n\n- [Code](library.rs)\n\n<!-- docs-tree:end -->\n\n---\n\n[Join](https://example.test/)\n\n## Our Mission\n\nSoftware you can make your own.\n';
  writeFileSync(join(root, "README.md"), source);
  const guide = join(web, "docs/agents/guide.md");
  const back = relative(dirname(guide), join(root, "README.md")).split("\\").join("/");
  put("agents/guide.md", `# Guide\n\n[Home](${back}#install).\n`);
  const home = docs.page("en", [])!;
  assert.equal(home.title, "Shared Home");
  assert.equal(home.description, "The shared introduction.");
  assert.equal(docs.site("en").description, home.description);
  assert.equal(home.cards.length, 1, "the header and introduction stay in the same card");
  const rendered = JSON.stringify(home.cards);
  assert.match(rendered, /"href":"\/agents#start"/);
  assert.match(rendered, /"src":"\/icon.png"/);
  assert.match(rendered, /"width":128/);
  assert.match(rendered, /"href":"https:\/\/github.com\/.+\/library.rs"/);
  assert.match(rendered, /"overview-header"/);
  assert.equal(rendered.match(/"tagName":"hr"/g)?.length, 2, "authored rules keep their native Markdown rendering");
  assert.doesNotMatch(rendered, /onerror|bad\(\)|"value":"Code"/);
  assert.match(rendered, /Own Your Own Intelligence/);
  assert.doesNotMatch(rendered, /example\.test\/site|Free and open source/, "the header keeps only its brand on the website");
  assert.deepEqual(home.headings, [
    { label: "Install", href: "#install" },
    { label: "Our Mission", href: "#our-mission" },
  ]);
  assert.ok(home.contents.some(entry => entry.href === "/agents"));
  const card = home.cards.at(-1)!;
  assert.equal(card.kind, "card");
  assert.match(JSON.stringify(card.content.children.at(-1)), /"id":"page-contents"/);
  assert.ok(rendered.indexOf('"id":"our-mission"') < rendered.indexOf('"id":"page-contents"'));
  assert.match(JSON.stringify(docs.page("en", ["agents", "guide"])), /"href":"\/#install"/);
  assert.equal(readFileSync(join(root, "README.md"), "utf8"), source);
  writeFileSync(join(root, "README.md"), source.replace("The shared introduction.", "Updated at its source."));
  assert.equal(docs.page("en", [])!.description, "Updated at its source.");
  const loader = new URL("./docs.ts", import.meta.url).href;
  execFileSync(process.execPath, ["--conditions=react-server", "--import", import.meta.resolve("tsx"), "--input-type=module", "-e", `
    import assert from "node:assert/strict";
    const docs = await import(${JSON.stringify(loader)});
    assert.equal(docs.page("en", []).description, "Updated at its source.");
    assert.match(JSON.stringify(docs.page("en", []).cards), /"href":"\\/agents#start"/);
    assert.match(JSON.stringify(docs.page("en", ["agents", "guide"]).cards), /"href":"\\/#install"/);
  `], { cwd: web, env: { ...process.env, NODE_ENV: "production" }, stdio: "pipe" });
});

test("a shared page is the site's own page, read from its one source", () => {
  const registry = join(root, "scripts/documentation-index.json");
  const config = JSON.parse(readFileSync(registry, "utf8"));
  config.sites[0].sharedPages = { "mission.md": "shared/docs/mission.md" };
  writeFileSync(registry, JSON.stringify(config));
  mkdirSync(join(root, "shared/docs"), { recursive: true });
  writeFileSync(join(root, "shared/docs/mission.md"), "# Our Mission\n\nOwn your intelligence. [Source](../../library.rs)\n");
  const guide = join(web, "docs/agents/guide.md");
  const target = relative(dirname(guide), join(root, "shared/docs/mission.md")).split("\\").join("/");
  put("agents/guide.md", `# Guide\n\n[Mission](${target}#values).\n`);
  const mission = docs.page("en", ["mission"])!;
  assert.equal(mission.title, "Our Mission");
  assert.match(JSON.stringify(mission.cards), /"href":"https:\/\/github.com\/.+\/library.rs"/, "links resolve from the shared source");
  assert.match(JSON.stringify(docs.page("en", ["agents", "guide"])), /"href":"\/mission#values"/, "a link to the source is a link to the site's page");
  put("mission.md", "# Second\n\nAuthored twice.\n");
  assert.throws(() => docs.page("en", ["mission"]), /second authored page/);
});

test("shared pages cannot escape their repository or name an unsafe page", () => {
  const registry = join(root, "scripts/documentation-index.json");
  const config = JSON.parse(readFileSync(registry, "utf8"));
  config.sites[0].sharedPages = { "mission.md": "../outside.md" };
  writeFileSync(registry, JSON.stringify(config));
  assert.throws(() => docs.page("en", []), /escapes the repository/);
  config.sites[0].sharedPages = { "../mission.md": "shared/mission.md" };
  writeFileSync(registry, JSON.stringify(config));
  assert.throws(() => docs.page("en", []), /sharedPages map/);
});

test("shared homepage sources and asset declarations cannot escape their repository", () => {
  const registry = join(root, "scripts/documentation-index.json");
  const config = JSON.parse(readFileSync(registry, "utf8"));
  config.sites[0].homepage = "../outside.md";
  writeFileSync(registry, JSON.stringify(config));
  assert.throws(() => docs.page("en", []), /escapes the repository/);
  config.sites[0].homepage = "README.md";
  config.sites[0].assets = { "../logo.png": "/icon.png" };
  writeFileSync(registry, JSON.stringify(config));
  assert.throws(() => docs.page("en", []), /escapes the repository/);
});

test("links to another site's shared homepage use its local or production address", () => {
  const registry = join(root, "scripts/documentation-index.json");
  const config = JSON.parse(readFileSync(registry, "utf8"));
  config.sites[1].homepage = "other-home.md";
  writeFileSync(registry, JSON.stringify(config));
  writeFileSync(join(root, "other-home.md"), "# Other\n\nAnother website.\n");
  const guide = join(web, "docs/agents/guide.md");
  const target = relative(dirname(guide), join(root, "other-home.md")).split("\\").join("/");
  put("agents/guide.md", `# Guide\n\n[Other](${target}#install).\n`);
  assert.match(JSON.stringify(docs.page("en", ["agents", "guide"])), /"href":"http:\/\/localhost:3001\/#install"/);
  const loader = new URL("./docs.ts", import.meta.url).href;
  execFileSync(process.execPath, ["--conditions=react-server", "--import", import.meta.resolve("tsx"), "--input-type=module", "-e", `
    import assert from "node:assert/strict";
    const docs = await import(${JSON.stringify(loader)});
    assert.match(JSON.stringify(docs.page("en", ["agents", "guide"])), /"href":"https:\\/\\/docs.local.link\\/#install"/);
  `], { cwd: web, env: { ...process.env, NODE_ENV: "production" }, stdio: "pipe" });
});

test("hard line breaks render while metadata keeps word boundaries and rewritten links", () => {
  const source = "# Home\n\nKeep your **files**.\\\nEverything runs in a [sandbox](agents/guide.md).\n";
  put("README.md", source);
  const page = docs.page("en", [])!;
  assert.equal(page.description, "Keep your files. Everything runs in a sandbox.");
  assert.equal(docs.site("en").description, page.description);
  const cards = JSON.stringify(page.cards);
  assert.match(cards, /"tagName":"br"/);
  assert.match(cards, /"href":"\/agents\/guide"/);
  assert.equal(readFileSync(join(web, "docs/README.md"), "utf8"), source);
});

test("editing a doc refreshes its page, title, headings and navigation", () => {
  docs.page("en", ["agents", "guide"]);
  docs.tree("en");
  put("agents/guide.md", "# Updated\n\nChanged description.\n\n## New section\n");
  const page = docs.page("en", ["agents", "guide"])!;
  assert.equal(page.title, "Updated");
  assert.equal(page.description, "Changed description.");
  assert.deepEqual(page.headings, [{ label: "New section", href: "#new-section" }]);
  assert.throws(() => docs.tree("en"), /not its title Updated/);
  put("navigation.md", website.replace("[Guide]", "[Updated]"));
  assert.equal(docs.tree("en")[1].children.find((n) => n.href.endsWith("/guide"))?.label, "Updated");
});

test("section page labels follow their headings when enabled and refresh with content", () => {
  put("agents/README.md", "# Agents\n\n> Agent List\n\nChoose an agent.\n\n```sh\nlnk agent start\n```\n");
  assert.equal(docs.tree("en")[1].children[0].label, "Get Started");
  index(["web/apps/first"], [], "heading");
  assert.equal(docs.tree("en")[1].children[0].label, "Agent List");
  put("agents/README.md", "# Agents\n\n> Your Agents\n\nChoose an agent.\n\n```sh\nlnk agent start\n```\n");
  assert.equal(docs.tree("en")[1].children[0].label, "Your Agents");
  put("agents/README.md", "# Agents\n\nChoose an agent.\n\n```sh\nlnk agent start\n```\n");
  assert.equal(docs.tree("en")[1].children[0].label, "Agents");
  index(["web/apps/first"]);
  assert.equal(docs.tree("en")[1].children[0].label, "Get Started");
});

test("both page layouts omit repository indexes without changing authored content or source", () => {
  const source = '# Agents\n\nIntroduction.\n\n<!-- docs-tree:start -->\n\n## Docs\n\n- [Guide](guide.md)\n\n<!-- docs-tree:end -->\n';
  put("agents/README.md", source);
  assert.deepEqual(docs.page("en", ["agents"])?.headings, []);
  index(["web/apps/first"]);
  assert.deepEqual(docs.page("en", ["agents"])?.headings, []);
  assert.equal(docs.page("en", ["agents"])?.description, "Introduction.");
  assert.equal(readFileSync(join(web, "docs/agents/README.md"), "utf8"), source);
  index(["web/apps/first"]);
  assert.deepEqual(docs.page("en", ["agents"])?.headings, []);
});

test("navigation titles preserve Markdown formatting, references and non-first-line headings", () => {
  for (const text of [
    "# **Guide**\n\nContent.\n",
    "Guide\n=====\n\nContent.\n",
    "```md\n# Wrong\n```\n\n# Guide\n",
    "<!-- # Wrong -->\n\n# Guide\n",
    "   # Guide #\n\nContent.\n",
    "# [Guide][name]\n\n[name]: https://example.com\n",
  ]) {
    put("agents/guide.md", text);
    assert.equal(docs.tree("en")[1].children.find((n) => n.href.endsWith("/guide"))?.label, "Guide");
  }
});

test("adding and deleting docs refreshes route indexes and the tree", () => {
  docs.routes();
  put("agents/another.md", "# Another\n\nA new page.\n");
  assert.equal(docs.page("en", ["agents", "another"])?.title, "Another");
  assert.ok(docs.routes()["/agents/another"]);
  assert.throws(() => docs.tree("en"), /pages in no tab/);
  put("navigation.md", website + "  - [Another](agents/another.md)\n");
  assert.ok(docs.tree("en")[1].children.some((n) => n.href === "/agents/another"));
  rmSync(join(web, "docs/agents/guide.md"));
  assert.equal(docs.page("en", ["agents", "guide"]), undefined);
  assert.equal(docs.routes()["/agents/guide"], undefined);
  assert.throws(() => docs.tree("en"), /missing page/);
  put("navigation.md", website.replace("  - [Guide](agents/guide.md)\n", "") + "  - [Another](agents/another.md)\n");
  assert.ok(!docs.tree("en")[1].children.some((n) => n.href === "/agents/guide"));
});

test("index edits refresh configured source folders and still refuse duplicate URLs", () => {
  docs.tree("en");
  const other = join(root, "other/docs/agents");
  mkdirSync(other, { recursive: true });
  writeFileSync(join(other, "other.md"), "# Other\n\nAnother folder.\n");
  index(["web/apps/first", "other"]);
  assert.equal(docs.page("en", ["agents", "other"])?.title, "Other");
  writeFileSync(join(other, "guide.md"), "# Duplicate\n");
  assert.throws(() => docs.pages("en"), /two pages at one URL/);
});

test("documentation owners cannot escape the repository", () => {
  index(["web/apps/first", "../outside"]);
  assert.throws(() => docs.pages("en"), /owner escapes the repository/);
});

test("development pages are reachable locally but absent from production routes and navigation", () => {
  index(["web/apps/first"], ["agents/guide.md"]);
  assert.equal(docs.page("en", ["agents", "guide"])?.title, "Guide");
  assert.ok(docs.routes()["/agents/guide"]);
  assert.ok(docs.tree("en")[1].children.some((n) => n.href === "/agents/guide"));
  put("README.md", "# Home\n\nWelcome.\n\n- [Agents](agents/README.md)\n- [Guide](agents/guide.md): A development page.\n");
  // A fresh loader proves the production build and runtime path without
  // reusing the development process's cached route and navigation indexes.
  const source = new URL("./docs.ts", import.meta.url).href;
  execFileSync(process.execPath, ["--conditions=react-server", "--import", import.meta.resolve("tsx"), "--input-type=module", "-e", `
    import assert from "node:assert/strict";
    const docs = await import(${JSON.stringify(source)});
    assert.equal(docs.page("en", ["agents", "guide"]), undefined);
    assert.ok(!docs.pages("en").some((p) => p.file === "agents/guide.md"));
    assert.equal(docs.routes()["/agents/guide"], undefined);
    assert.deepEqual(docs.alternates("agents/guide.md"), {});
    assert.deepEqual(docs.tree("en")[1].children, []);
    assert.equal(docs.neighbors("en", "/agents").next, undefined);
    const prose = JSON.stringify(docs.page("en", ["agents"]));
    assert.match(prose, /Guide/);
    assert.ok(!docs.page("en", ["agents"]).contents.some(entry => entry.href.includes("/guide")));
    const home = JSON.stringify(docs.page("en", []));
    assert.match(home, /Agents/);
    assert.doesNotMatch(home, /Guide|development page|guide.md/);
  `], { cwd: web, env: { ...process.env, NODE_ENV: "production" }, stdio: "pipe" });
});

test("hidden pages disappear locally without deleting their source and return when unhidden", () => {
  const source = readFileSync(join(web, "docs/agents/guide.md"), "utf8");
  put("README.md", "# Home\n\nWelcome.\n\n- [Agents](agents/README.md)\n- [Guide](agents/guide.md): Hidden offer.\n");
  assert.ok(docs.routes()["/agents/guide"]);
  // Even a page otherwise allowed in development is hidden immediately.
  index(["web/apps/first"], ["agents/guide.md"], undefined, ["agents/guide.md"]);
  assert.equal(docs.page("en", ["agents", "guide"]), undefined);
  assert.ok(!docs.pages("en").some((p) => p.file === "agents/guide.md"));
  assert.equal(docs.routes()["/agents/guide"], undefined);
  assert.deepEqual(docs.alternates("agents/guide.md"), {});
  assert.deepEqual(docs.tree("en")[1].children, []);
  assert.equal(docs.neighbors("en", "/agents").next, undefined);
  const prose = JSON.stringify(docs.page("en", ["agents"]));
  assert.match(prose, /Guide/);
  assert.doesNotMatch(prose, /"href":"\/agents\/guide"/);
  assert.doesNotMatch(JSON.stringify(docs.page("en", [])), /Guide|Hidden offer|guide.md/);
  assert.equal(readFileSync(join(web, "docs/agents/guide.md"), "utf8"), source);
  index(["web/apps/first"]);
  assert.equal(docs.page("en", ["agents", "guide"])?.title, "Guide");
  assert.ok(docs.routes()["/agents/guide"]);
  assert.ok(docs.tree("en")[1].children.some((n) => n.href === "/agents/guide"));
});

test("new languages refresh localized routes without changing English links", () => {
  docs.routes();
  writeFileSync(join(web, "docs/navigation.es.md"), website.replace("[Guide]", "[Guía]") + "\n## URLs\n\n- agents: agentes\n- guide: guia\n");
  put("agents/guide.es.md", "# Guía\n\nUna página.\n");
  put("agents/README.es.md", "# Agents\n\nAgentes.\n");
  assert.deepEqual(docs.languages(), ["en", "es"]);
  assert.ok(docs.isLanguage("es"));
  assert.equal(docs.tree("es")[1].children.find((n) => n.label === "Guía")?.href, "/es/agentes/guia");
  assert.equal(docs.page("es", ["agentes", "guia"])?.title, "Guía");
  assert.equal(docs.page("es", ["agentes"])!.contents.find((entry) => entry.href === "/es/agentes/guia")?.description, "Una página");
  assert.deepEqual(docs.alternates("agents/guide.md"), { en: "/agents/guide", es: "/es/agentes/guia" });
  assert.match(readFileSync(join(web, "docs/agents/README.md"), "utf8"), /guide.md/);
  assert.match(JSON.stringify(docs.page("en", ["agents"])), /"href":"\/agents\/guide"/);
});

test("a link to the other website's page goes to its address there, in the tree too", () => {
  const other = join(root, "web/apps/other/docs");
  mkdirSync(other, { recursive: true });
  writeFileSync(join(other, "terms.md"), "# Terms\n\nThe rules.\n");
  put("agents/guide.md", "# Guide\n\n[Terms](../../../other/docs/terms.md#use)\n");
  assert.match(JSON.stringify(docs.page("en", ["agents", "guide"])), /"href":"http:\/\/localhost:3001\/terms#use"/);
  put("navigation.md", website.replace("- [Agents]", "  - [Terms](../../other/docs/terms.md)\n- [Agents]"));
  assert.deepEqual(docs.tree("en")[0].children, [{ label: "Terms", href: "http://localhost:3001/terms", children: [] }]);
  // An external section's children are still described from their owning docs.
  put("navigation.md", website + "  - [Terms](../../other/docs/terms.md)\n");
  assert.equal(docs.page("en", ["agents"])!.contents.at(-1)?.description, "The rules");
});


test("a held folder's pages are no page of the site: its links go to the repository, and the tree drops them", () => {
  const held = join(root, "other/docs/tunnels");
  mkdirSync(held, { recursive: true });
  writeFileSync(join(held, "README.md"), "# Tunnels\n\nHeld back.\n");
  index(["web/apps/first", { directory: "other", released: false }]);
  put("agents/guide.md", "# Guide\n\n[Tunnels](../../../../../other/docs/tunnels/README.md)\n");
  put("navigation.md", website + "  - [Tunnels](../../../../other/docs/tunnels/README.md)\n");
  assert.equal(docs.page("en", ["tunnels"]), undefined);
  assert.equal(docs.routes()["/tunnels"], undefined);
  assert.match(JSON.stringify(docs.page("en", ["agents", "guide"])), /"href":"https:\/\/github.com\/[^"]*\/other\/docs\/tunnels\/README.md"/);
  assert.deepEqual(docs.tree("en")[1].children.map((n) => n.label), ["Get Started", "Guide"]);
  // A page placed only beneath a held one is held with it.
  put("tunnels/wire.md", "# Wire\n\nIts protocol.\n");
  put("navigation.md", website + "  - [Tunnels](../../../../other/docs/tunnels/README.md)\n    - [Wire](tunnels/wire.md)\n");
  assert.equal(docs.page("en", ["tunnels", "wire"]), undefined);
  assert.deepEqual(docs.tree("en")[1].children.map((n) => n.label), ["Get Started", "Guide"]);
  index(["web/apps/first", { directory: "other", released: true } as unknown as { directory: string; released: false }]);
  assert.throws(() => docs.tree("en"), /released is false or absent/);
});

test("unreleased modules keep their spec references on the documentation site", () => {
  const reference = join(root, "other/docs/config/specs");
  mkdirSync(reference, { recursive: true });
  writeFileSync(join(reference, "env.md"), "# Environment Spec\n\nThe environment format.\n");
  mkdirSync(join(root, "other/docs/services/environments"), { recursive: true });
  writeFileSync(join(root, "other/docs/services/environments/README.md"), "# Environments\n\nNot released.\n");
  index(["web/apps/first", { directory: "other", released: false }]);
  put("navigation.md", website + "  - [Environment Spec](../../../../other/docs/config/specs/env.md)\n");
  put("agents/guide.md", "# Guide\n\n[Spec](../../../../../other/docs/config/specs/env.md)\n");
  assert.equal(docs.page("en", ["config", "specs", "env"])?.title, "Environment Spec");
  assert.ok(docs.routes()["/config/specs/env"]);
  assert.equal(docs.page("en", ["services", "environments"]), undefined);
  assert.match(JSON.stringify(docs.page("en", ["agents", "guide"])), /"href":"\/config\/specs\/env"/);
  assert.ok(docs.tree("en")[1].children.some((n) => n.href === "/config/specs/env"));
  // Documentation previews publish usage pages without changing release status.
  index(["web/apps/first", { directory: "other", released: false, preview: true }]);
  put("navigation.md", website + "  - [Environment Spec](../../../../other/docs/config/specs/env.md)\n  - [Environments](../../../../other/docs/services/environments/README.md)\n");
  assert.equal(docs.page("en", ["services", "environments"])?.title, "Environments");
  assert.ok(docs.tree("en")[1].children.some((n) => n.href === "/services/environments"));
});

test("documentation shows short titles and leaves repository indexes out of the article", () => {
  const source = "# Agents\n\n> A much longer promotional heading\n\nStart your agent.\n\n## First Message\n\nTalk to it.\n\n<!-- docs-tree:start -->\n\n## Docs\n\n- [Guide](guide.md): repository-only description\n\n<!-- docs-tree:end -->\n";
  put("agents/README.md", source);
  const page = docs.page("en", ["agents"])!;
  const rendered = JSON.stringify(page.cards);
  assert.doesNotMatch(rendered, /promotional heading|repository-only description/);
  assert.match(rendered, /"tagName":"h1"[^}]*[\s\S]*"value":"Agents"/);
  assert.deepEqual(page.headings, [{ label: "First Message", href: "#first-message" }]);
  assert.equal(readFileSync(join(web, "docs/agents/README.md"), "utf8"), source);
  assert.ok(docs.tree("en")[1].children.some((n) => n.href === "/agents/guide"));
});

test("Docs follows navigation order, describes its links and stays below all page content", () => {
  put("agents/README.md", "# Agents\n\nStart an agent.\n\n## Setup\n\nInstall and start it.\n\n---\n\nFinal instructions.\n");
  const page = docs.page("en", ["agents"])!;
  const children = docs.tree("en")[1].children.filter((node) => node.href !== "/agents");
  assert.deepEqual(page.contents, [
    { label: "Setup", href: "#setup", description: "Install and start it" },
    ...children.map(({ label, href }) => ({ label, href, description: "First description" })),
  ]);
  assert.match(JSON.stringify(page.cards), /"ariaLabel":"Docs"/);
  assert.match(JSON.stringify(page.cards), /: First description/);
  assert.doesNotMatch(JSON.stringify(page.cards[0]), /"ariaLabel":"Docs"/);
  const last = page.cards.at(-1)!;
  assert.ok(last.kind === "card");
  assert.match(JSON.stringify(last.content.children.at(-1)), /"tagName":"nav","properties":\{"ariaLabel":"Docs"\}/);
  assert.ok(!page.contents.some((entry) => entry.href === "#contents"));
  assert.deepEqual(docs.page("en", ["agents", "guide"])!.contents, [
    { label: "Guide", href: "#guide-1", description: "One" },
    { label: "Guide", href: "#guide-2", description: "Two" },
  ]);
  assert.deepEqual(docs.page("en", [])!.contents, [{ label: "Agents", href: "/agents", description: "Start an agent" }]);
  put("agents/guide.md", "# Guide\n\nChanged **description**. More detail stays on the page.\n");
  assert.equal(docs.page("en", ["agents"])!.contents.at(-1)?.description, "Changed description");
});

test("reference sections open directly and list their child pages in Docs", () => {
  put("config/README.md", "# Config\n\nConfiguration reference.\n");
  put("config/specs.md", "# Specs\n\nSpec formats.\n");
  put("navigation.md", website + "- [Config](config/README.md)\n  - [Specs](config/specs.md)\n");
  const section = docs.tree("en").find((node) => node.href === "/config")!;
  assert.deepEqual(section.children.map((node) => node.label), ["Specs"]);
  assert.deepEqual(docs.page("en", ["config"])!.contents, [{ label: "Specs", href: "/config/specs", description: "Spec formats" }]);
  put("agents/README.md", "# Agents\n\nAgent references.\n");
  assert.deepEqual(docs.tree("en")[1].children.map((node) => node.label), ["Guide"]);
});

test("metadata comes from the homepage and shared labels fall back and refresh", () => {
  assert.equal(docs.site("en").name, "Example");
  assert.equal(docs.site("en").description, "Welcome.");
  assert.equal(docs.shareTitle("en"), "Example", "a home page without a headline is shared by the site's name");
  assert.equal(docs.shareImage("en"), "/opengraph-image/share");
  assert.equal(docs.shareImage("es"), "/es/opengraph-image/share");
  assert.equal(docs.page("en", [])?.description, "Welcome.");
  put("README.md", "# New home\n\nA changed opening.\n");
  assert.equal(docs.site("en").description, "A changed opening.");
  writeFileSync(join(web, "docs/navigation.es.md"), website);
  writeFileSync(join(catalogs, "es.json"), JSON.stringify({ Copy: "Copiar", Language: "Español" }));
  assert.equal(docs.word(docs.site("es"), "Copy"), "Copiar");
  assert.equal(docs.word(docs.site("es"), "Next"), "Next");
  writeFileSync(join(catalogs, "es.json"), JSON.stringify({ Copy: "Copia", Language: "Español" }));
  assert.equal(docs.word(docs.site("es"), "Copy"), "Copia");
  rmSync(join(catalogs, "es.json"));
  assert.equal(docs.word(docs.site("es"), "Copy"), "Copy");
  assert.throws(() => docs.word(docs.site("en"), "Missing"), /missing UI label/);
});

test("authored order is retained and duplicate, unlabeled or dangling navigation fails", () => {
  put("agents/second.md", "# Second\n\nAnother page.\n");
  put("navigation.md", website.replace("  - [Guide]", "  - [Second](agents/second.md)\n  - [Guide]"));
  assert.deepEqual(docs.tree("en")[1].children.map((n) => n.label), ["Get Started", "Second", "Guide"]);
  put("navigation.md", website + "  - [Guide](agents/guide.md)\n");
  assert.throws(() => docs.tree("en"), /duplicate page/);
  put("navigation.md", website.replace("[Guide]", "[Wrong]"));
  assert.throws(() => docs.tree("en"), /not its title/);
  put("navigation.md", website.replace("  - [Guide](agents/guide.md)", "  - Missing link"));
  assert.throws(() => docs.tree("en"), /one labeled link/);
});

test("a contributor navigation guide remains a page while only app navigation is metadata", () => {
  put("more/navigation.md", "# Navigation\n\nA contributor guide.\n");
  put("navigation.md", website + "- [Navigation](more/navigation.md)\n");
  assert.ok(docs.page("en", ["more", "navigation"]));
  assert.ok(docs.tree("en").some((n) => n.href === "/more/navigation"));
  assert.ok(!docs.pages("en").some((p) => p.file === "navigation.md"));
});


test("UI catalog errors are visible instead of silently showing configuration keys", () => {
  writeFileSync(join(web, "docs/navigation.es.md"), website);
  writeFileSync(join(catalogs, "es.json"), JSON.stringify({ Copy: "" }));
  assert.throws(() => docs.site("es"), /nonempty strings/);
  writeFileSync(join(catalogs, "es.json"), JSON.stringify({ Unknown: "Desconocido" }));
  assert.throws(() => docs.site("es"), /unknown UI label/);
});
