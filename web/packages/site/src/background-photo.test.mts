import assert from "node:assert/strict";
import { test } from "node:test";
import { imageSources, readPhotos } from "./background-photo.ts";
import settings from "./background-settings.json" with { type: "json" };

const base = { name: "dunes", position: "60% center", wide: "left center" };

test("cover sizing includes the photo width hidden by a tall screen", () => {
  const [photo] = readPhotos({ backgrounds: [{ ...base,
    responsive: { width: 1600, height: 900, widths: [640, 960, 1280, 1600] } }] });
  const sources = imageSources(photo, "/dunes");
  assert.equal(sources.src, "/dunes/1600.jpg");
  assert.equal(sources.srcSet, "/dunes/640.jpg 640w, /dunes/960.jpg 960w, /dunes/1280.jpg 1280w, /dunes/1600.jpg 1600w");
  assert.equal(sources.sizes, `max(100vw, ${settings.layerHeight * 1600 / 900}lvh)`);
  // A 390 x 844 phone displays a landscape source across a much wider area.
  const coveredWidth = Math.max(390, settings.layerHeight / 100 * 844 * 1600 / 900);
  assert.ok(coveredWidth > 1600);
});

test("malformed lists and responsive dimensions are refused", () => {
  for (const value of [null, {}, { backgrounds: {} }, { backgrounds: [base] }, { backgrounds: [{ ...base, name: "../other" }] }]) {
    assert.throws(() => readPhotos(value));
  }
  for (const responsive of [null, {}, { width: 800, height: 0, widths: [800] },
    { width: 800, height: 600, widths: [] }, { width: 800, height: 600, widths: [960] },
    { width: 800, height: 600, widths: [640, 640, 800] },
    { width: 800, height: 600, widths: [800, 640] },
    { width: 800, height: 600, widths: [640] }]) {
    assert.throws(() => readPhotos({ backgrounds: [{ ...base, responsive }] }));
  }
});

test("responsive settings describe ordered positive sizes and a taller layer", () => {
  assert.ok(settings.widths.length > 1);
  assert.ok(settings.widths.every((w, i) => Number.isSafeInteger(w) && w > 0 && (i === 0 || w > settings.widths[i - 1])));
  assert.ok(settings.layerHeight >= 100);
});
