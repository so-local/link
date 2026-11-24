import assert from "node:assert/strict";
import { test } from "node:test";
import { actionTitle } from "./action.ts";

test("action titles retain existing actions and allow authored compact labels", () => {
  assert.deepEqual(actionTitle("button"), { kind: "button" });
  assert.deepEqual(actionTitle("placeholder"), { kind: "placeholder" });
  assert.deepEqual(actionTitle("button|Presale"), { kind: "button", short: "Presale" });
  assert.deepEqual(actionTitle("placeholder|Video"), { kind: "placeholder", short: "Video" });
  assert.equal(actionTitle("price"), undefined);
  assert.equal(actionTitle(undefined), undefined);
});

test("malformed compact action labels fail visibly", () => {
  for (const title of ["button|", "placeholder| ", "button|One|Two"]) {
    assert.throws(() => actionTitle(title), /action titles use/);
  }
});
