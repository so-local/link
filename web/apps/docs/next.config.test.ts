import assert from "node:assert/strict";
import { test } from "node:test";
import { getRedirectUrl, unstable_getResponseFromNextConfig } from "next/experimental/testing/server";
import docsConfig from "./next.config";
import sharedConfig from "@link-web/site/config";

const origin = "https://docs.local.link";

for (const [from, to] of [
  ["/config/environment", "/more/reference/environment"],
  ["/more/modules/commands", "/more/reference/commands"],
  ["/more/developing/repository-files", "/more/reference/files"],
  ["/config/locations", "/more/reference/locations"],
  ["/more/developing/component-build", "/more/libraries/types/component-metadata/build"],
  ["/more/developing/secretive-derive", "/more/libraries/files/secretive/derive"],
  ["/security/sandbox/binding", "/security/sandbox/more/bridges/binding"],
  ["/security/sandbox/protocol", "/security/sandbox/more/contract/client"],
  ["/security/sandbox/routes", "/security/sandbox/more/contract/routes"],
  ["/security/sandbox/models-and-secrets", "/security/sandbox/more/contract/models-and-secrets"],
  ["/agents/tools/memory/documents-contract", "/agents/tools/memory/documents/more/contract"],
  ["/agents/move-stream", "/agents/more/interface/contract/move-stream"],
  ["/agents/hosting", "/agents/more/interface/contract/hosting"],
  ["/agents/harnesses/link", "/agents/link"],
  ["/agents/harnesses/link/acp", "/agents/link/more/acp"],
  ["/agents/harnesses/link/commands/decisions?view=full", "/agents/link/commands/decisions?view=full"],
  ["/en/agents/harnesses/link", "/agents/link"],
  ["/en/agents/harnesses/link/media/decisions", "/agents/link/media/decisions"],
  ["/es/agents/harnesses/link", "/es/agents/link"],
  ["/pt-BR/agents/harnesses/link/commands?view=full", "/pt-BR/agents/link/commands?view=full"],
  ["/config/guide", "/config"],
  ["/config/credentials/guide", "/config/credentials"],
  ["/config/secrets/guide", "/config/secrets"],
  ["/en/config/credentials/guide", "/config/credentials"],
  ["/es/config/secrets/guide", "/es/config/secrets"],
  ["/config/guide?view=full", "/config?view=full"],
  ["/agents/machines/guide", "/agents/move"],
  ["/en/agents/machines/guide", "/agents/move"],
  ["/es/agents/machines/guide?view=full", "/es/agents/move?view=full"],
  ["/agents/machines", "/agents/move"],
  ["/en/agents/machines", "/agents/move"],
  ["/agents/machines/troubleshooting", "/agents/move/troubleshooting"],
  ["/agents/harnesses/switch", "/agents/harnesses/use"],
  ["/en/agents/harnesses/switch", "/agents/harnesses/use"],
  ["/pt-BR/agents/harnesses/switch?view=full", "/pt-BR/agents/harnesses/use?view=full"],
  ["/agents/harnesses/troubleshooting", "/agents/harnesses/use/troubleshooting"],
  ["/agents/harnesses/switch/decisions", "/agents/harnesses/use/migrate/decisions"],
  ["/agents/models/switch", "/agents/models/agent"],
  ["/agents/guide", "/agents/start"],
  ["/agents/files", "/agents/more/settings/files"],
  ["/agents/configuration", "/agents/more/settings"],
  ["/agents/protocol", "/agents/more/interface"],
  ["/agents/bridge", "/agents/more/interface/bridge"],
  ["/agents/namespaces", "/agents/more/interface/namespaces"],
  ["/agents/skills", "/agents/tools/use/skill"],
  ["/agents/security", "/agents/more/security"],
  ["/agents/decisions", "/agents/more/decisions"],
  ["/agents/troubleshooting", "/agents/more/troubleshooting"],
  ["/en/agents/files", "/agents/more/settings/files"],
  ["/es/agents/protocol?view=full", "/es/agents/more/interface?view=full"],
  ["/en/agents/models/switch/troubleshooting", "/agents/models/agent/troubleshooting"],
  ["/tooling/sandbox", "/security/sandbox"],
  ["/tooling/sandbox/proxy/connect", "/security/sandbox/more/proxy/connect"],
  ["/en/tooling/sandbox/guide", "/security/sandbox/guide"],
  ["/es/tooling/sandbox/wall?view=full", "/es/security/sandbox/wall?view=full"],
]) {
  test(`old documentation address ${from} permanently redirects directly to ${to}`, async () => {
    const response = await unstable_getResponseFromNextConfig({ url: origin + from, nextConfig: docsConfig });
    assert.equal(response.status, 308);
    assert.equal(getRedirectUrl(response), origin + to);
  });
}

test("new addresses and other harnesses keep their routes", async () => {
  for (const path of ["/agents/link", "/agents/link/more/acp", "/es/agents/link", "/agents/harnesses/hermes", "/agents/harnesses/linkage", "/agents/harnesses/use", "/agents/move", "/agents/models/agent", "/agents/harnesses/use/migrate", "/agents/start", "/agents/chat", "/agents/team", "/security", "/security/sandbox", "/security/sandbox/guide", "/tooling/measure"]) {
    const response = await unstable_getResponseFromNextConfig({ url: origin + path, nextConfig: docsConfig });
    assert.equal(getRedirectUrl(response), null, path);
  }
});

test("docs retains the shared English canonicalization and security headers", async () => {
  const response = await unstable_getResponseFromNextConfig({ url: origin + "/en/agents/harnesses/hermes", nextConfig: docsConfig });
  assert.equal(response.status, 308);
  assert.equal(getRedirectUrl(response), origin + "/agents/harnesses/hermes");
  assert.equal(response.headers.get("x-frame-options"), "DENY");
});

test("the shared configuration, which other websites use, has no docs-only relocation", async () => {
  const response = await unstable_getResponseFromNextConfig({ url: "https://www.local.link/agents/harnesses/link/acp", nextConfig: sharedConfig });
  assert.equal(getRedirectUrl(response), null);
});
