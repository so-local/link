import assert from "node:assert/strict";
import { test } from "node:test";
import relocations from "./docs/relocations.json";
import { relocatedFragment } from "./relocations";

test("moved sections preserve the fragment after page canonicalization", () => {
  assert.equal(relocatedFragment("/agents/tools/run/guide", "#run__start", "?view=full"), "/agents/tools/run/more/developing?view=full#direct-background-start");
  assert.equal(relocatedFragment("/agents/connections/more/protocol", "#lnk-connection-hook"), "/agents/connections/more/protocol/pushed#lnk-connection-hook");
  assert.equal(relocatedFragment("/agents/link/commands/decisions", "#why-does---agent-last-only-until-the-agent-starts-again"), "/agents/link/commands/decisions#why-does---every-chat-last-only-until-the-agent-starts-again");
});

test("translated and old English addresses retain their canonical language", () => {
  assert.equal(relocatedFragment("/es/agents/tools/run/guide", "#run__start", "?view=full", "es"), "/es/agents/tools/run/more/developing?view=full#direct-background-start");
  assert.equal(relocatedFragment("/pt-BR/agents/tools/run/guide", "#run__start", "", "pt-BR"), "/pt-BR/agents/tools/run/more/developing#direct-background-start");
  assert.equal(relocatedFragment("/en/agents/tools/run/guide", "#run__start"), "/agents/tools/run/more/developing#direct-background-start");
});

test("unknown fragments and current destinations keep their addresses", () => {
  assert.equal(relocatedFragment("/agents/tools/run/guide", "#unknown"), null);
  assert.equal(relocatedFragment("/agents/tools/run/guide", ""), null);
  assert.equal(relocatedFragment("/agents/tools/run/more/developing", "#run__start"), null);
});

test("declared relocations stay on this site and reach final destinations", () => {
  for (const [from, to] of Object.entries(relocations.pages)) {
    assert.match(from, /^\/(?!\/)/);
    assert.match(to, /^\/(?!\/)/);
    assert.equal(Object.hasOwn(relocations.pages, to), false, `${from} forms a page redirect chain`);
  }
  for (const [from, to] of Object.entries(relocations.fragments)) {
    assert.match(to, /^\/(?!\/)/);
    assert.equal(Object.hasOwn(relocations.fragments, to), false, `${from} forms a fragment redirect chain`);
  }
});
