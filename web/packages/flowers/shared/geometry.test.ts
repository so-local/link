import assert from "node:assert/strict";
import test from "node:test";
import { arc, arcPath, reach, petalPaths } from "./geometry";
import { createField, createSingle, fieldCount, groundPath, pose, updateInteraction, type Pointer } from "./field";
import { flowerDefaults, resolveFlower, resolveScene, sceneDefaults } from "./settings";

test("circular stems reach the requested head center on both sides and at zero bend", () => {
  for (const bend of [-1.7, -.2, 0, .2, 1.7]) {
    const point = arc(220, bend), solved = reach(point.x, point.y);
    assert.ok(Math.abs(solved.tip.x - point.x) < 1e-8);
    assert.ok(Math.abs(solved.tip.y - point.y) < 1e-8);
    assert.ok(Math.abs(solved.bend - bend) < 1e-8);
    assert.deepEqual(arc(solved.length, solved.bend, 0), { x: 0, y: 0 });
    assert.ok(!/NaN|Infinity/.test(arcPath(solved.length, solved.bend)));
  }
});

test("wind and pointer use the same arc endpoint and tangent without moving roots", () => {
  const plants = createField(1280, 590, sceneDefaults), roots = plants.map(f => [f.x, f.y]);
  const f = plants[12], pointer: Pointer = { active: true, x: f.homeX - 20, y: f.homeY, skyX: 0, skyY: 0 };
  const before = pose(f, 6000, sceneDefaults);
  updateInteraction(plants, pointer, 1000, 1280, 590, sceneDefaults);
  assert.ok(f.pushX > 0);
  const after = pose(f, 6000, sceneDefaults);
  assert.ok(after.tip.x > before.tip.x);
  assert.deepEqual(after.tip, arc(after.length, after.bend));
  assert.equal(after.angle, after.bend * 180 / Math.PI);
  assert.deepEqual(plants.map(f => [f.x, f.y]), roots);
  pointer.active = false;
  for (let i = 0; i < 300; i++) updateInteraction(plants, pointer, 50, 1280, 590, sceneDefaults);
  assert.ok(Math.abs(f.pushX) < .0001);
});

test("field layout is seeded, depth sorted and retains offscreen roots and opaque ground", () => {
  const plants = createField(1280, 590, sceneDefaults);
  assert.deepEqual(plants, createField(1280, 590, sceneDefaults));
  assert.notDeepEqual(plants, createField(1280, 590, resolveScene({ seed: 17 })));
  assert.equal(plants.length, 36);
  assert.ok(plants.every((p, i) => p.y > 590 && (!i || p.y >= plants[i - 1].y)));
  assert.ok(new Set(plants.map(p => p.phase)).size === plants.length);
  const ground = groundPath(plants, 1280, 590, sceneDefaults);
  assert.ok(ground.d.endsWith("L -16 590 Z"));
  assert.ok(ground.horizon > 0 && ground.horizon < 590);
});

test("valid extreme scene settings remain finite, including straight and still flowers", () => {
  for (const settings of [resolveScene({ windStrength: 0 }), resolveScene({ flowerCount: 96, windStrength: 300, windSpeed: 300, leafScale: 250, bloomScale: 180, sizeVariation: 40 })]) {
    for (const f of createField(390, 540, settings)) for (const t of [0, 7000, 24000]) {
      const p = pose(f, t, settings);
      assert.ok([p.length, p.bend, p.tip.x, p.tip.y].every(Number.isFinite));
    }
  }
  const still = resolveScene({ windSpeed: 0 }), [f] = createSingle(flowerDefaults, still);
  assert.deepEqual(pose(f, 0, still), pose(f, 20000, still));
});

test("flower props change shared petal geometry and invalid inputs fail before SVG rendering", () => {
  assert.equal(petalPaths(flowerDefaults).length, 24);
  assert.notDeepEqual(petalPaths(flowerDefaults), petalPaths(resolveFlower({ petals: { innerWidth: 44 } })));
  assert.throws(() => resolveScene({ responseTime: NaN }), RangeError);
  assert.throws(() => resolveScene({ flowerCount: 1.5 }), RangeError);
  assert.throws(() => resolveFlower({ petals: { countPerRing: 10000 } }), RangeError);
  assert.throws(() => resolveFlower({ stem: { color: "url(https://example.com)" } }), RangeError);
});

test("a field holds its full count at the initial width and wider, and fewer on a phone", () => {
  const settings = resolveScene();
  assert.equal(fieldCount(1280, settings), 36);
  assert.equal(fieldCount(2560, settings), 36);
  assert.equal(fieldCount(390, settings), 20);
  assert.equal(createField(390, 844, settings).length, 20);
});
