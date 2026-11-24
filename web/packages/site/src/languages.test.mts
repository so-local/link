import assert from "node:assert/strict";
import test from "node:test";
import { contentManifest } from "./content-source";
import { languages, translationLanguage } from "./languages.mjs";

test("a listed language, alone or with a region in capitals, names a translation", () => {
  assert.equal(translationLanguage("guide.es.md"), "es");
  assert.equal(translationLanguage("guide.pt-BR.md"), "pt-BR");
  assert.equal(translationLanguage("guide.pt-br.md"), undefined);
});

test("English and unlisted suffixes are pages, foo.js.md among them", () => {
  assert.equal(translationLanguage("guide.en.md"), undefined);
  assert.equal(translationLanguage("foo.js.md"), undefined);
  assert.ok(!languages().includes("en"));
  const files = { "README.md": "# Home", "navigation.md": "# Docs", "foo.js.md": "# Foo.js" };
  assert.ok(contentManifest(files).files["foo.js.md"], "foo.js.md is an English page, needing no navigation.js.md");
});
