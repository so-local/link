import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const script = fileURLToPath(new URL("../scripts/translate.mjs", import.meta.url));
const english = { Copy: "Copy", Language: "English", "Get Started": "Get Started" };
const navigation = "# Example\n\n## Pages\n\n- [Home](README.md)\n";

type Request = { custom_id: string; body: { messages: { role: string; content: string }[]; response_format?: { type: string } } };

async function fixture(run: (f: { root: string; web: string; ui: string; put: (path: string, value: string) => void; requests: () => Promise<Request[]>; collect: (responses: object[]) => Promise<string> }) => Promise<void>) {
  const root = mkdtempSync(join(tmpdir(), "link-translation-"));
  const web = join(root, "web/apps/first");
  const ui = join(root, "web/packages/site/locales");
  const put = (path: string, value: string) => { mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, value); };
  put(join(root, "scripts/documentation-index.json"), JSON.stringify({ schema: 1, sites: [{ name: "home", directory: "web/apps/first", roots: true }], roots: [] }));
  put(join(web, "docs/navigation.md"), navigation);
  put(join(web, "docs/README.md"), "# Welcome\n\nThe actual introduction.\n");
  put(join(ui, "en.json"), JSON.stringify(english));
  const invoke = async (...args: string[]) => (await promisify(execFile)(process.execPath, [script, ...args], {
    cwd: web, encoding: "utf8", env: { ...process.env, LNK_TRANSLATE_LANG: "es", OPENAI_API_KEY: "" },
  })).stdout;
  try {
    await run({ root, web, ui, put,
      requests: async () => {
        const output = join(root, "requests.jsonl");
        await invoke("submit", "--out", output);
        return readFileSync(output, "utf8").trim().split("\n").map((line) => JSON.parse(line));
      },
      collect: async (responses) => {
        const output = join(root, "results.jsonl");
        writeFileSync(output, responses.map((value) => JSON.stringify(value)).join("\n") + "\n");
        return invoke("collect", "--file", output);
      },
    });
  } finally { rmSync(root, { recursive: true, force: true }); }
}

function result(id: string, content: string) {
  return { custom_id: id, response: { status_code: 200, body: { choices: [{ message: { content } }] } } };
}

test("translation submits structural navigation and one shared JSON UI catalog", () => fixture(async ({ requests }) => {
  const entries = await requests();
  const nav = entries.find((r) => r.custom_id.endsWith(":navigation.md"))!;
  assert.equal(nav.body.messages[1].content, navigation);
  assert.match(nav.body.messages[0].content, /structural website navigation/);
  assert.ok(!nav.body.messages[1].content.includes("## Words"));
  const catalogs = entries.filter((r) => r.custom_id.startsWith("ui:"));
  assert.equal(catalogs.length, 1);
  assert.equal(catalogs[0].body.response_format?.type, "json_object");
  assert.deepEqual(JSON.parse(catalogs[0].body.messages[1].content), english);
}));

test("translation treats included outlines as structure and rejects prose in a section", () => fixture(async ({ web, put, requests, collect }) => {
  put(join(web, "docs/navigation.md"), '# Example\n\n## Pages\n\n- [Home](navigation/home.md "include")\n');
  const section = '# Home Navigation\n\n## Pages\n\n- [Home](../README.md)\n';
  put(join(web, "docs/navigation/home.md"), section);
  const entry = (await requests()).find((r) => r.custom_id.endsWith(":navigation/home.md"))!;
  assert.match(entry.body.messages[0].content, /structural website navigation/);
  await collect([result(entry.custom_id, section.replace("## Pages", "An introduction.\n\n## Pages"))]);
  assert.equal(existsSync(join(web, "docs/navigation/home.es.md")), false);
  await collect([result(entry.custom_id, section.replaceAll("Home", "Inicio"))]);
  const translated = readFileSync(join(web, "docs/navigation/home.es.md"), "utf8");
  assert.match(translated, /\[Inicio\]\(\.\.\/README.md\)/);
  assert.doesNotMatch(translated, /## URLs/);
}));

test("translation reads the shared homepage directly without creating an English copy", () => fixture(async ({ root, web, requests }) => {
  const registry = join(root, "scripts/documentation-index.json");
  const config = JSON.parse(readFileSync(registry, "utf8"));
  config.sites[0].homepage = "README.md";
  writeFileSync(registry, JSON.stringify(config));
  rmSync(join(web, "docs/README.md"));
  const source = "# Shared Home\n\nThe single authored introduction.\n";
  writeFileSync(join(root, "README.md"), source);
  const entries = await requests();
  const home = entries.find(entry => entry.custom_id.endsWith(":README.md"))!;
  assert.equal(home.body.messages[1].content, source);
  assert.equal(readFileSync(join(root, "README.md"), "utf8"), source);
  assert.equal(existsSync(join(web, "docs/README.md")), false);
}));

test("collect writes shared UI JSON separately and retains structural navigation URLs", () => fixture(async ({ web, ui, requests, collect }) => {
  const entries = await requests();
  const responses = entries.map((r) => {
    const content = r.custom_id.startsWith("ui:") ? JSON.stringify({ Copy: "Copiar", Language: "Español", "Get Started": "Comenzar" })
      : r.custom_id.startsWith("slugs:") ? "{}"
      : r.custom_id.endsWith(":navigation.md") ? navigation.replace("Example", "Ejemplo").replace("Home", "Inicio")
      : "# Bienvenidos\n\nLa introducción.\n";
    return result(r.custom_id, content);
  });
  await collect(responses);
  const translated = JSON.parse(readFileSync(join(ui, "es.json"), "utf8"));
  assert.equal(translated.Copy, "Copiar");
  assert.equal(translated.Language, "Español");
  assert.match(translated.$source, /^[0-9a-f]{16}$/);
  const nav = readFileSync(join(web, "docs/navigation.es.md"), "utf8");
  assert.match(nav, /## Pages\n/);
  assert.match(nav, /\[Inicio\]\(README.md\)/);
  assert.match(nav, /## URLs/);
  assert.ok(!nav.includes("Copy") && !nav.includes("## Words"));
  assert.ok(!existsSync(join(web, "locales/es.json")));
}));

test("stale or structurally invalid UI results cannot overwrite the shared catalog", () => fixture(async ({ ui, requests, collect }) => {
  const id = (await requests()).find((r) => r.custom_id.startsWith("ui:"))!.custom_id;
  const previous = { $source: "0000000000000000", Copy: "Antes", Language: "Español", "Get Started": "Comenzar" };
  writeFileSync(join(ui, "es.json"), JSON.stringify(previous));
  await collect([result(id, JSON.stringify({ Copy: "Copiar", Extra: "Extra" }))]);
  assert.deepEqual(JSON.parse(readFileSync(join(ui, "es.json"), "utf8")), previous);
  writeFileSync(join(ui, "en.json"), JSON.stringify({ ...english, Next: "Next" }));
  await collect([result(id, JSON.stringify({ Copy: "Copiar", Language: "Español", "Get Started": "Comenzar" }))]);
  assert.deepEqual(JSON.parse(readFileSync(join(ui, "es.json"), "utf8")), previous);
}));


test("navigation collection refuses added prose and changed link targets", () => fixture(async ({ web, requests, collect }) => {
  const id = (await requests()).find((r) => r.custom_id.endsWith(":navigation.md"))!.custom_id;
  await collect([result(id, navigation.replace("## Pages", "Unnecessary description.\n\n## Pages"))]);
  assert.ok(!existsSync(join(web, "docs/navigation.es.md")));
  await collect([result(id, navigation.replace("README.md", "missing.md"))]);
  assert.ok(!existsSync(join(web, "docs/navigation.es.md")));
}));
