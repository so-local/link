import assert from "node:assert/strict";
import test from "node:test";
import { renderToString } from "react-dom/server";
import { FlowerBackground, FlowerSvg } from "./index";

test("both components prerender as vector artwork with no editor or raster assets", () => {
  const html = renderToString(<><FlowerSvg title="Sunflower" /><FlowerBackground fixed={false} /></>);
  assert.equal((html.match(/<svg /g) ?? []).length, 2);
  assert.equal((html.match(/data-plant=""/g) ?? []).length, 37);
  assert.ok(html.includes("<defs>"));
  assert.ok(html.includes("<use "));
  assert.ok(html.includes("<title"));
  assert.ok(!/<(image|img|canvas|iframe|input|button)\b/.test(html));
});

test("multiple instances have self-contained definitions and honor count and color props", () => {
  const html = renderToString(<><FlowerBackground settings={{ flowerCount: 8 }} /><FlowerSvg framing="head" flower={{ petals: { innerColor: "#112233" } }} /></>);
  const ids = [...html.matchAll(/\bid="([^"]+)"/g)].map(m => m[1]);
  assert.equal(new Set(ids).size, ids.length);
  for (const [, target] of html.matchAll(/(?:href="#|url\(#)([^"\)]+)/g)) assert.ok(ids.includes(target), target);
  assert.equal((html.match(/data-plant=""/g) ?? []).length, 9);
  assert.ok(html.includes('fill="#112233"'));
});
