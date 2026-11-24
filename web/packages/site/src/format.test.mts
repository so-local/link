import assert from "node:assert/strict";
import test from "node:test";
import { format, markdown, metadata, type PageReference } from "./format";

test("page metadata uses Markdown defaults and explicit frontmatter overrides", () => {
  assert.deepEqual(metadata(markdown.parse("# Example\n\nKeep **your** files.\\\nChoose a model.")), {
    title: "Example", description: "Keep your files. Choose a model.",
  });
  const tree = markdown.parse('---\ntitle: Short title\ndescription: A card introduction.\nimage: https://images.example/cover.jpg\n---\n\n# Longer Heading\n\nThe full introduction.');
  assert.deepEqual(metadata(tree), { title: "Short title", description: "A card introduction.", image: "https://images.example/cover.jpg" });
  assert.doesNotMatch(JSON.stringify(format(tree)), /Short title|images.example/);
});

test("metadata rejects invalid shapes, repeated fields and unsafe images", () => {
  for (const fields of ["[title, image]", "image: javascript:alert(1)", "image: data:image/png;base64,abc", "image: //example.com/a.jpg", "title: false", "title: ''", "title: One\ntitle: Two", "unknown: Value"]) {
    assert.throws(() => metadata(markdown.parse(`---\n${fields}\n---\n# Example`)), /metadata|image/);
  }
});

test("card grids use linked metadata in authored order and leave the input tree unchanged", () => {
  const source = "# Learn\n\n:::cards\n- [First label](first.md)\n- [Second label](second.md)\n:::\n";
  const tree = markdown.parse(source);
  const before = structuredClone(tree);
  const pages: Record<string, PageReference> = {
    "first.md": { href: "/first", title: "First page", description: "First intro", image: "/cover.jpg" },
    "second.md": { href: "/second", title: "Second page", description: "Second intro" },
  };
  const rendered = format(tree, href => pages[href]);
  const grid = rendered.children.find(node => node.type === "element" && node.tagName === "ul");
  assert.ok(grid?.type === "element");
  assert.deepEqual(grid.properties.className, ["page-grid", "not-prose"]);
  const text = JSON.stringify(grid);
  assert.ok(text.indexOf("First page") < text.indexOf("Second page"));
  assert.match(text, /"href":"\/first"/);
  assert.match(text, /"src":"\/cover.jpg"/);
  assert.match(text, /"dataPageCover":"Second page"/);
  assert.doesNotMatch(text, /First label|Second label/);
  assert.deepEqual(tree, before);
});

test("invalid and unavailable card entries fail before rendering", () => {
  for (const content of ["A paragraph.", "- words", "- [Page](one.md) with extra text", "1. [Page](one.md)", "- [ ] [Page](one.md)", "- [Page](missing.md)"]) {
    assert.throws(() => format(markdown.parse(`:::cards\n${content}\n:::`)), /cards/);
  }
});

test("documentation syntax and directive examples retain their ordinary meaning", () => {
  const tree = markdown.parse('# Guide\n\n## Choose a model\n\n[Go](#choose-a-model "button")\n\n| Name | Value |\n| --- | --- |\n| Model | **Example** |\n\n```markdown\n:::cards\n- [Page](page.md)\n:::\n```\n\n<script>bad()</script>');
  const text = JSON.stringify(format(tree));
  assert.match(text, /"id":"choose-a-model"/);
  assert.match(text, /"title":"button"/);
  assert.match(text, /"tagName":"table"/);
  assert.match(text, /:::cards/);
  assert.doesNotMatch(text, /bad\(\)|page-grid/);
});
