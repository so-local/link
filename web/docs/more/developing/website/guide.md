# Websites Guide

Change the documentation website, add a page to it, or translate it. It is built from repository pages and a shared navigation outline, so each page says the same on GitHub and on the website. Run `cd web`, `npm install`, then `npm run dev`.

## The website
It is a Next.js app, `web/apps/docs`, over one package, `web/packages/site`: the reader, layout and look that any website can use, each choosing its own pages and landing layout.

### docs.local.link

`web/apps/docs`: the documentation, from the `docs/` of every folder `scripts/documentation-index.json` lists.

### Shared Homepage

A site's `homepage` in `scripts/documentation-index.json` names one Markdown
file relative to its repository. The docs site uses the workspace `README.md`
directly, so GitHub, local Markdown viewers and the website share one authored
overview. Relative links resolve from that source file: documentation links
become website routes, and repository-only links go to GitHub. The site's
`assets` map connects source image paths to its public image URLs. The
README's introduction is the same authored content on both, and its centered
header is not.

#### Shared Pages

A site's `sharedPages` maps a page of the site to a Markdown file in its
repository, or in a repository its index includes, such as
`{ "values.md": "../other/docs/values.md" }`. The page is the site's own,
at `/values`, with the site's tabs and look, and is read from that one
source. Links in it resolve from the source file, and a link anywhere on the
site to the source file goes to the site's page. A page the site also
authors itself is refused, as is a source outside those repositories.

#### Homepage layout

The header and introduction share one card using the site's
standard Markdown styles, including horizontal rules inside the card.
Other docs use rules to divide cards.
The homepage's section headings appear in the sidebar.
Repository contents indexes are omitted from the website, and its final Docs
section follows the website's navigation outline.
The web workspace's build and test inputs include the shared README so its
edits invalidate cached output.

#### Header on the Website

The website keeps only the header's brand: its logo, title and tagline,
in one compact row with the small logo beside the title. The header's
links and license lines are for readers on GitHub, and the website's tabs
already carry them, so the introduction starts near the top of the card.

### What it runs on

It uses Next.js 16 with the App Router, React 19, TypeScript, Tailwind CSS 4 and ESLint, and needs Node 20.9 or newer.

### Styles

Each app's root layout imports its own `globals.css`, which loads Tailwind,
`@link-web/site/styles` and the typography plugin. The shared stylesheet owns
the look and registers the shared components as Tailwind sources. Keeping
the Tailwind import in the app resolves build dependencies from that app's
workspace, including when the shared package is linked from another workspace.

### Linked packages

The shared configuration resolves each linked package dependency and all its
subpaths from the app's installation. Navigation hooks and the app's router
therefore share one Next.js routing context even when another workspace has
its own Next.js installation. Font entry points resolve through their package
exports before aliasing, so their public import paths need not match their files.

## One set of docs

These rules keep the docs one set. From the Rust workspace root,
`cargo test -p link-docs-check -p link-docs-check-code` checks each
folder's links and anchors, relative page links, and README hubs.

### Every fact is in a folder's `docs/`

Every file there is a page of one of the websites. Nothing is said only in the repository or only on the website.

### A folder's pages are in that folder

They change in the pull request that changes its code. `interop/agents/harnesses/openclaw/docs/agents/harnesses/openclaw/` is [OpenClaw's page](../../../../../interop/agents/harnesses/openclaw/docs/agents/harnesses/openclaw/README.md), at `/agents/harnesses/openclaw`, and the core's `modules/agents/agent/docs/agents/` is [Agents](../../../../../modules/agents/agent/docs/agents/README.md), at `/agents`.

### A page's path is its URL

A page's path in its folder's `docs/` is its URL on its website, whichever folder holds it, and the build fails if two folders of one
website hold one path. The shared navigation outline determines where the page appears in the reading tree, independently of its URL.

### A `README.md` outside `docs/` is a map of its folder

A paragraph says what the folder holds: its long description. Its `## Docs` lists its pages and child folders' README hubs alphabetically
by title, with their short descriptions. A description is said once where it lives (a package's `Cargo.toml` `description`, else the
first sentence of the README or page), so no parent restates it to drift. A hub with nothing under it lists nothing.

### A package's `## Code`

It links its entry points, three or so, only code, each in a few words, alphabetically by name.
Any other section comes after. How anything works is said in `docs/`, and what a file does at its top.

### A map where the website has none

GitHub shows a folder's `README.md` where the website has no page. So
do `.github/`'s `SECURITY.md`, `SUPPORT.md` and `CODE_OF_CONDUCT.md`, which point at their pages.

### A folder's `README.md` links one level down, no further

It links its own docs, and the `README.md` of each folder directly in it. The workspace's links its members (`harnesses/`, `connections/`,
...), and `harnesses/`'s each harness. Regeneration updates the hubs and shared outline after a folder is moved or added.

### Every link to a page is a relative path to its file

As in `../agent/README.md`, never `https://docs.local.link/...`, so a
reader following links stays in the repository. The website turns each
path into the page's URL, in the reader's language, and a path to
another website's page in this repository into its address there. Only a
website itself, `https://docs.local.link/`, and the legal pages that
www.local.link serves, such as `https://www.local.link/license`, are
linked by their address.

## Develop

`npm run dev` serves the site on port 3001, and `npm run lint && npm test && npm run build` checks it.

### Another website

In development a link to another website of the index goes to its address on this
machine, its `local`, so they link to each other as they do when deployed.

### Show it to someone

Run `npm run dev` in the web workspace and open the address printed by the documentation app. The development server reloads pages as you edit their owning Markdown files.

### Before pushing

Run `npm run lint`, `npm test` and `npm run build`, in that order.
`.github/workflows/website.yml` runs them in CI for matching public
changes. A bare `tsc` before the first build fails on `LayoutProps`, a
global type Next.js generates.

### Its cache

Development checks the docs' file versions once per server render and
reuses unchanged Markdown, page data and navigation indexes. Edits,
additions and deletions refresh the cache, including changes to
`navigation.md` and the folders the index lists.

### What a page renders

Each page supplies its own headings to the sidebar; opening one never
renders the other pages to collect theirs. The Geist fonts are bundled
locally, so development and production builds download no fonts.

## Add a page

Add a Markdown file to the `docs/` of the folder it's about, and its
path there is its URL, such as `../core/docs/pricing.md` for a page, or
`../core/docs/pricing/README.md` for one with pages under it. Run `npm
run dev` and refresh to see a change to the docs.

### Where it goes

A page of the documentation goes in the `docs/` of the folder it is about, and a new
folder of docs is a line in the documentation index.

### Its title and description

Its first `#` heading is its title ("Local Link — Pricing"), and its
first paragraph is its description in search results and link previews.

### Its place in the tree

Each website's `docs/navigation.md` holds its shared reading order
under `## Pages`. The website renders that outline, and the README
writer follows it. Run the [documentation writer](../../../../../tests/docs/docs/more/developing/documentation/navigation.md)
after adding or moving a page; it inserts missing entries while keeping
curated order. Move links in the outline to change their placement.

```markdown
- [Agents](../../../../modules/agents/agent/docs/agents/README.md)
  - [Link Harness](../../../../modules/agents/harnesses/link/docs/agents/link/README.md)
  - [Harnesses](../../../../modules/agents/harnesses/docs/agents/harnesses/README.md)
    - [OpenClaw](../../../../interop/agents/harnesses/openclaw/docs/agents/harnesses/openclaw/README.md)
```

### A section

A page with pages under it is a section. Its line names the section (OpenClaw), and its own page, its folder’s `README.md`, is the first
page under it, called Get Started when it opens with a shell example or a
first task marked `<!-- quickstart -->`. With `sectionPages` set to "heading", the site uses the page’s hook or title instead.

#### Reference sections

Reference sections, including Configuration and Tooling indexes, open directly. Introduce major commands in short task sections, with detailed usage in the guide beneath them.
The writer inserts a folder's other pages under its README unless the outline already places them elsewhere.

#### Docs

Every documentation article ends with Docs generated from its own headings
and the same outline used by the navigation. A section lists its child
pages in navigation order; a reference lists its headings and related pages.
The homepage lists the other tabs. Each link is followed by a colon and the
first sentence of its description, matching the Markdown README indexes.
Quickstarts and all other article content come before Docs. Continue and Next Step lists are unnecessary.

### The order and the words of its lines

The outline owns sibling order. Labels follow page titles; a top-level
link titled "tab" keeps its authored short label. Generated README
indexes don't follow it: they are alphabetical by title, so a reader
finds a child in a folder's map without knowing the website's order.

### Another website's pages

A line linking another website's page is a tab or a page there,
opened in a new tab with an arrow: the outline links Home and the legal
pages by their addresses on www.local.link, where they live once.

### Pages held back

A folder `scripts/documentation-index.json` marks `"released": false`
is a module's a release holds back, and its pages are no page of the
website. A link to one goes to its file in the repository, and the tree
drops its line with every line under it, so a page placed only there is
held with it. Specification references under `config/specs/` remain readable
on the documentation site even when their owning module is held. An owner marked `preview: true` also publishes its usage pages without changing the program’s release status.

#### Pages reserved for development

A website's `developmentPages` in the documentation index lists paths relative to `docs/`. These pages are available only through `npm run dev`. Every other environment excludes them from routes, language alternatives, navigation and lists linking them, so a direct request returns 404. Links in prose retain their words without a link.

#### Hidden pages

A website's `hiddenPages` lists paths in the same format and excludes them in every environment, including local development. Their source and reading-tree entries remain intact so they can return later.

### What a page may link

A link in a page is to a page, a file, or an `https://`, `http://` or
`mailto:` address; any other scheme fails the build.

### Interface text

Shared interface labels live in `packages/site/locales/<language>.json`,
with English fallback. Navigation files contain structure, and page
content supplies descriptions and share metadata.

### Headings

No heading has more than ten headings under it, so group a longer list under headings one level deeper.

## How a page is laid out

The tabs are pills across the top, and on a wide screen each drops down its pages. The content column is centred in the screen, with equal space on either side and the navigation tree in the left space. Folding the tree keeps the content's position and width. Its hamburger opens the tree without moving the content. There is no footer because every page is in the tabs.

### The tree

Beside documentation pages is the whole site's tree, the same from tab to tab, with the page you're on dark, the sections above it in bold, its `##` headings under it and the other sections folded. On narrow screens, the hamburger floats the tree over the page just beneath the tabs, so nothing under it moves, wherever the page is scrolled. Picking a section goes to its page underneath and keeps the tree open with that section's pages unfolded, ahead of the page's headings. Picking a page with nothing under it, a heading, or a link to another site, or tapping outside the tree, closes it.

### Folding it away

On a wide screen a pill before the tabs folds the tree away, leaving
the page alone in the middle, and back: a site starts folded or open
as the index's `tree` says (docs.local.link open), and a
browser keeps how its reader last left it.

### Around a page

Documentation pages show their sections above the title and previous/next
pills below the cards.

### What the Markdown means there

The docs are plain Markdown, so they read the same on GitHub. A few
things mean something more on the website:

| In the Markdown | On the website |
|---|---|
| First `#` heading | Its short title, or its tab’s name if a tab |
| Each `##` heading | A line under the page in the tree |
| Generated README index | Repository navigation, omitted on the website |
| First paragraph | Its description, with line breaks as spaces |
| `---` | Divider inside a homepage, otherwise between cards |
| Link titled "button", optional "\|Short Label" | Standard pill with a compact label |
| Link titled "button" in a card | A pill, grouped in action lists |
| Link titled "placeholder", optional "\|Short Label" | A disabled outbound action |
| List of links alone between two `---` | A row of standard action pills |
| `sh` code block | Separate copy buttons for complete commands |
| Any other code block | Code |
| `<!-- ... -->` | Hidden in both |
| Relative link to a `.md` file | A link to its page, anchor kept |
| Link to its own website's address | That page, in the reader's language |
| Relative link to another website's page | A link to its address there |
| Relative link outside the docs | The file on GitHub, marked as leaving |
| Link to another site | A link with an arrow after it |

### Copy Commands

Each `sh` block gives every complete shell statement its own copy button.
Commands joined with `&&`, `||`, or a pipe remain together, as do continued
lines, quoted strings, loops, and conditionals. A heredoc passes text between
delimiter lines to a command, so its entire input stays with that command when copied.
Copy button labels are excluded from selection, so selecting several commands copies only their text.

### Languages and builds

English is served without a prefix. `/agents` is
`app/[lang]/[[...path]]` with `lang` = `en`, and `/en/agents` redirects
to `/agents` (`packages/site/src/config.ts`). Every page is built once,
at `npm run build`. [Moved documentation](redirects.md) explains redirects for relocated pages.

## Translate the docs

You write the docs in English, in each folder's `docs/`. A model
writes the other languages beside them, each page's translation a
`<page>.<language>.md` next to it (`guide.es.md` beside `guide.md`), and
never by hand. The language is one of
[`languages.toml`](../../../../../packages/component/languages.toml)'s,
alone or with a region in capitals (`pt-BR`), which every check, build
and script reads, so `foo.js.md` is an English page. Each page names the
English it was made from, and the website marks a page whose English
has changed since.

### Run the Translation Steps

In `web/apps/docs`, `node ../../packages/site/scripts/translate.mjs`
takes a step as its word. `plan` lists what needs translating. With
`OPENAI_API_KEY` set, `submit` sends the work, `status` follows it, and
`collect` writes the translations beside their English pages.

### A language

A language is one this app's `docs/` has a `navigation.md` for, whose
`## URLs` lists the folder and file names in that language, such as
`/es/agentes`. The language pill in the tabs goes to the same page in
another language.

### What `collect` writes

`collect` validates page headings and code blocks, navigation links and
nesting, and UI catalog keys and English-source hashes before writing.
Invalid or stale results leave existing files intact. UI catalogs are
shared across every app. `LNK_TRANSLATE_MODEL` picks the model.

### Who runs it

No workflow runs this: whoever changes a folder's `docs/` runs it,
and commits what `collect` wrote with that change.

## Host the Website

Build from the public npm workspace and serve the documentation app with a Next.js-compatible host.
For a Vercel project, use `web/apps/docs` as the Root Directory. Its `vercel.json` pins the Next.js framework preset, so Vercel builds the app with Next.js rather than looking for a static `public` folder.
The build reads repository documentation outside the app directory, so include files outside the Root Directory.

### Security Headers

The shared configuration sets framing, content-type, referrer, HTTPS and device-permission headers.
Keep these headers when adapting the hosting configuration.

### Domain

The documentation app serves **`docs.local.link`**. Its
`scripts/documentation-index.json` entry supplies its public address
and the address used during local development.

### Its DNS and redirect

Point the documentation domain at the chosen hosting project and retain
the [documentation redirects](redirects.md) supplied by the app's Next
configuration. The installer source is `core/cli/install.sh`; the
[release procedure](../../../../../scripts/docs/more/developing/releases.md#how-users-get-it)
explains its version floor and signatures.

### What they keep

The websites store nothing of their own. Photos and lists are read from
[a public bucket](#the-bucket).

## The bucket

What changes without a release is in one public Google Cloud Storage
bucket, `local-link-web` (`packages/site/src/bucket.ts`), read in the
browser for its `backgrounds/` assets.
Anyone may read it, from any site; only its owners write.

### Backgrounds

Every page shows the animated sunflower field from the
[Flowers package](../../../../packages/flowers/docs/more/developing/website/flowers/README.md),
which the shared layout mounts unless a site passes another background.

### Share Card

A link to a site, posted on X, in Messages or anywhere else that reads
Open Graph, shows the site's share card. The card is the header's
wordmark, large, on white: "local.link" in the header's type (`brand` in
`look.ts`), the tagline "Own Your Intelligence" under it at exactly its width, and
the sunflower beside both lines. The tagline is the catalog's `Tagline`,
translated with the other labels. Nothing else is on the card, so it still
reads as a thumbnail. Next draws it at build time in Geist.

#### Share Titles

The home page is shared as the site's name and its headline, and every
other page as its own title. A page with a [primary image](markdown.md#primary-images)
is shared with that image instead. `@link-web/site/share` makes each
website's card, and `@link-web/site/share-card` draws it for any app.

### Photos

A site may show the day's photo as its background (`@link-web/site/background`),
the same on every website that does: the list's day-th in turn,
counting days in UTC. Each photo is a folder,
`backgrounds/<name>/`, with responsive JPEGs named `<width>.jpg`, and
`blur.jpg`, a tiny copy shown blurred until the photo arrives. The browser
chooses a size for the screen and pixel density, accounting for the
photo's proportions and the height of the scrolling background.

### Image sizes

Uploads produce widths of 640, 960, 1280, 1920 and 2560 pixels, capped
at the original width without enlargement. A smaller original adds its
own width as the largest size. The list records the actual dimensions
and available widths. The shared `packages/site/src/background-settings.json`
holds the sizes and background height. Every listed photo must include
responsive dimensions and its generated size files.


## Layout

Page prose stays in each member's `docs/`; shared catalogs hold interface
labels. `packages/site/src/docs.ts` reads those pages through the
index's `sites` and renders each app's `navigation.md` outline.
[Decisions](decisions.md) explains the ownership and rendering choices.

### No Rust here

`web/` holds no Rust, and the core no TypeScript. `npm test` checks the
docs cache against edits, additions, deletions, source-folder changes
and translations.

### The Turbo cache

The Turbo cache includes the documentation index, component pages,
shared website pages and the repository's documentation guides. A page
edited in any of those roots invalidates the websites' builds, even
when an app's source is unchanged.

## Rendering Extensions

The page renderer accepts optional header, breadcrumb, pagination and card-rendering slots through `rendering`.
Markdown component overrides let another website render its own content while sharing the documentation layout and standard links.

### Markdown Collections

The shared formatter supports page metadata and `:::cards` collections on every
website. [Markdown Pages](markdown.md) explains their syntax, primary images,
responsive layout and validation, including an example rendered by the docs site.

### Background Slot

The shared root layout accepts an optional `background` React node, with the flower field (`@link-flowers/browser`) as its default. A host supplies its own background at that layout boundary, beside the changing page content, so client navigation preserves the mounted artwork and its animation. The shared package owns the slot and has no dependency on the host's renderer.

### Site Header Slot

The shared root layout accepts an optional `header` React node before the main content and its page navigation. A host supplies semantic branding or other site-wide content once at this boundary. It persists during navigation independently of each page's heading, while sites that omit the slot keep their existing layout.

### Visit Counts

Plausible Analytics counts a site's visits without cookies or personal data, in production alone. Each site has a Plausible script of its own and the hostnames it's served at in production, and the shared root layout accepts them as an optional `analytics` React node. The docs site passes its script and `docs.local.link` from `apps/docs/src/constants.ts`, and a site that omits the slot counts nothing.

#### Production Hostnames

The package's `Analytics` component (`@link-web/site/analytics`) writes one inline script. On a production hostname that script loads Plausible without blocking the page and starts its queue. On any other hostname, such as a preview or a machine running the site, it does nothing.
