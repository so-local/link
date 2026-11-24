import assert from "node:assert/strict";
import test from "node:test";
import { contentManifest, manifest, readContent, type ContentFiles } from "./content-source";

const files: ContentFiles = {
  "README.md": "# Home\n\nWelcome.",
  "navigation.md": "# Example\n\n## Pages\n\n- [Home](README.md)",
  "learn/README.md": "# Learn\n\n:::cards\n- [Guide](guide.md)\n:::",
  "learn/guide.md": "# Guide\n\nLearn more.",
};

test("a bucket snapshot verifies and returns all Markdown files without a provider SDK", async () => {
  const base = new URL("https://bucket.example/pages/v1/");
  const requested: string[] = [];
  const result = await readContent(base, contentManifest(files), async url => {
    requested.push(url.href);
    return files[url.pathname.slice(base.pathname.length)];
  });
  assert.deepEqual(result, files);
  assert.ok(requested.includes("https://bucket.example/pages/v1/learn/guide.md"));
});

test("mixed or incomplete publications are rejected rather than using bundled copy", async () => {
  await assert.rejects(readContent(new URL("https://bucket.example/"), contentManifest(files), async () => "changed"), /published digest/);
  await assert.rejects(readContent(new URL("https://bucket.example/"), contentManifest(files), async () => { throw new Error("404"); }), /404/);
});

test("manifests refuse escaping paths, unsupported schemas and missing navigation", () => {
  const good = contentManifest(files);
  for (const path of ["../outside.md", "learn/../../outside.md", "learn/%2e%2e/outside.md", "/page.md", "page.md?x=1", "learn/page\\other.md", "learn/.hidden/page.md", "page.EN.md"]) {
    assert.throws(() => manifest({ schema: 2, files: { ...good.files, [path]: good.files["README.md"] } }), /page path/);
  }
  assert.throws(() => manifest({ ...good, schema: 1 }), /unsupported/);
  assert.throws(() => manifest({ schema: 2, files: {} }), /homepage and navigation/);
  assert.throws(() => manifest({ schema: 2, files: { ...good.files, "page.es.md": good.files["README.md"] } }), /navigation/);
});
