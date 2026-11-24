// A public, provider-independent snapshot of Markdown, prepared before Next builds.
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rename, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { translationLanguage } from "./languages.mjs";

export type ContentManifest = { schema: 2; files: Record<string, string> };
export type ContentFiles = Record<string, string>;
export type ReadText = (url: URL) => Promise<string>;
export const digest = (text: string) => createHash("sha256").update(text).digest("hex");

/**
 * Every name is a Markdown file, English at its path and a translation
 * beside it as `<page>.<language>.md`, with no URL or path escapes.
 */
export function manifest(value: unknown): ContentManifest {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid pages manifest");
  const record = value as Record<string, unknown>;
  if (record.schema !== 2 || Object.keys(record).some(key => !["schema", "files"].includes(key))) throw new Error("unsupported pages manifest");
  if (!record.files || typeof record.files !== "object" || Array.isArray(record.files)) throw new Error("pages manifest needs files");
  const files: Record<string, string> = {};
  for (const [path, hash] of Object.entries(record.files)) {
    if (!/^(?:[A-Za-z0-9_-]+\/)*[A-Za-z0-9_-]+(?:\.[a-z0-9]+(?:-[A-Z]{2})?)?\.md$/.test(path)) throw new Error(`invalid page path: ${path}`);
    if (typeof hash !== "string" || !/^[a-f0-9]{64}$/.test(hash)) throw new Error(`invalid page digest: ${path}`);
    files[path] = hash;
  }
  if (!files["README.md"] || !files["navigation.md"]) throw new Error("pages manifest needs the English homepage and navigation");
  for (const path of Object.keys(files)) {
    const language = translationLanguage(path);
    if (language && !files[`navigation.${language}.md`]) throw new Error(`pages manifest needs navigation for ${path}`);
  }
  return { schema: 2, files };
}

export function contentManifest(files: ContentFiles): ContentManifest {
  return manifest({ schema: 2, files: Object.fromEntries(Object.entries(files).sort(([a], [b]) => a.localeCompare(b)).map(([path, text]) => [path, digest(text)])) });
}

const fetchText: ReadText = async url => {
  const response = await fetch(url, { cache: "no-store", signal: AbortSignal.timeout(30_000) });
  if (!response.ok) throw new Error(`cannot read pages: ${url} (${response.status})`);
  return Buffer.from(await response.arrayBuffer()).toString("utf8");
};

/** Verify the entire publication before handing any of its pages to the renderer. */
export async function readContent(base: URL, index: ContentManifest, read: ReadText = fetchText): Promise<ContentFiles> {
  const entries = await Promise.all(Object.entries(index.files).map(async ([path, hash]) => {
    const text = await read(new URL(path, base));
    if (digest(text) !== hash) throw new Error(`page does not match its published digest: ${path}`);
    return [path, text];
  }));
  return Object.fromEntries(entries);
}

/** The cache is selected by manifest content, never by a mutable bucket URL alone. */
export async function prepareContent(url: string, directory: string): Promise<string> {
  const base = new URL(url.endsWith("/") ? url : url + "/");
  if (!["https:", "http:"].includes(base.protocol) || base.username || base.password || base.search || base.hash) throw new Error("pages URL must be a public HTTP directory");
  const index = manifest(JSON.parse(await fetchText(new URL("manifest.json", base))));
  const text = JSON.stringify(index);
  const destination = join(directory, digest(text));
  const ready = join(destination, "manifest.json");
  if (existsSync(ready) && await readFile(ready, "utf8") === text) return destination;
  const files = await readContent(base, index);
  await mkdir(directory, { recursive: true });
  const temporary = await mkdtemp(join(directory, ".stage-"));
  try {
    await Promise.all(Object.entries(files).map(async ([path, body]) => {
      const file = join(temporary, path);
      await mkdir(join(file, ".."), { recursive: true });
      await writeFile(file, body);
    }));
    await writeFile(join(temporary, "manifest.json"), text);
    try {
      await rename(temporary, destination);
    } catch (error) {
      if (!existsSync(ready) || await readFile(ready, "utf8") !== text) throw error;
    }
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
  return destination;
}
