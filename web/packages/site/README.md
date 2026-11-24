# Websites Package

`@link-web/site` holds what the Local Link websites share: the docs reader, the layout, the look, and the scripts. Each app is the website whose folder runs it.

## Docs

- [Websites](../../docs/more/developing/website/README.md): Change the documentation website, add a page to it, or translate it

## Code

- [`scripts/translate.mjs`](scripts/translate.mjs): translates every page
- [`src/docs.ts`](src/docs.ts): reads every folder's pages, for one website
- [`src/nav.tsx`](src/nav.tsx): the tabs and the tree
- [`src/page.tsx`](src/page.tsx): a page, as cards
