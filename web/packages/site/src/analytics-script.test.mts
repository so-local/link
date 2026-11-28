import assert from "node:assert/strict";
import test from "node:test";
import { runInNewContext } from "node:vm";
import { countingScript } from "./analytics-script";

// The inline script, run as a page on `hostname` runs it: what it adds to
// the head, and whether Plausible's queue started.
function runOn(hostname: string) {
  const added: { async: boolean; src: string }[] = [];
  const page: Record<string, unknown> = {
    location: { hostname },
    document: { createElement: () => ({ async: false, src: "" }), head: { appendChild: (s: { async: boolean; src: string }) => added.push(s) } },
  };
  page.window = page;
  runInNewContext(countingScript({ script: "https://plausible.io/js/pa-test.js", hosts: ["local.link", "www.local.link"] }), page);
  return { added, started: typeof page.plausible === "function" };
}

test("a production hostname loads its Plausible script without blocking and starts its queue", () => {
  for (const host of ["local.link", "www.local.link"]) {
    assert.deepEqual(runOn(host), { added: [{ async: true, src: "https://plausible.io/js/pa-test.js" }], started: true });
  }
});

test("any other hostname loads nothing and starts nothing", () => {
  for (const host of ["local-link-scratch-cloud.vercel.app", "localhost", "docs.local.link", "evil.local.link.example"]) {
    assert.deepEqual(runOn(host), { added: [], started: false });
  }
});
