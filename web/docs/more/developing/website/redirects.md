# Moved Documentation

Permanent redirects preserve existing documentation links when a page
moves to a new address, including its child pages and language-prefixed
addresses. The docs app keeps their data in
[`relocations.json`](../../../../apps/docs/docs/relocations.json), read by its
Next configuration. Each declaration points directly to its final address.

## Link Harness addresses

The docs app adds redirects in `apps/docs/next.config.ts` while retaining
the shared configuration. Addresses under `/agents/harnesses/link`
redirect to `/agents/link`, with the remaining path and query preserved.
The shared configuration, which other apps use, has none of these feature redirects.

### Languages

English addresses with `/en` redirect directly to the canonical address
without that prefix. Other language prefixes remain in the destination,
so an existing localized link continues to select the same language.

## Sandbox Addresses

Sandbox belongs to the Security section, so addresses under
`/tooling/sandbox` redirect to `/security/sandbox`, with the remaining path,
query and any language prefix preserved. The docs app holds these beside the
Link Harness redirects in `apps/docs/next.config.ts`.

## Reference Pages

Configuration, Credentials and Secrets open directly at their reference pages.
Their former `guide` addresses permanently redirect to those pages, including
English and other language prefixes, so saved links continue to work.

## Moved Sections

A URL fragment, the part after `#`, stays in the browser and never reaches
the server. When a section moves to another page or changes its heading,
the same data file declares its old and new page-and-fragment addresses.
The docs app resolves those declarations after navigation, retaining the
query and selected language. Unknown fragments remain unchanged.

## Verify a relocation

The docs app tests its actual Next configuration, including nested
paths, language prefixes, query strings, unrelated routes and shared
headers. Run its routing tests from the web workspace after changing a
redirect, then build the sites to validate the configuration. Run `npm run test --workspace @link-web/docs`, then `npm run build`.
