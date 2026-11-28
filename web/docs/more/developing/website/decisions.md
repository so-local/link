# Decisions

Why docs.local.link is built the way it is.
What it does: [the website](README.md).


## The docs

Why the docs are one set, kept beside their code, and checked as they
are written.

### Why is there one set of docs?

Because two drift. The repository and the website each had their own
copy of most pages, and they disagreed on wording, names and which
pages existed. With one file, a developer reads the same thing on
GitHub and on the website, and a change is one edit.

### Why are a folder's pages in that folder?

So a pull request that changes a folder's code changes its pages in the
same place, and a folder that leaves for a repository of its own takes
its pages with it. OpenClaw's pages are in `interop/agents/harnesses/openclaw/`,
Link Harness's in `modules/agents/harnesses/link/`. The Agent owner
keeps the shared agent workflows, while the repository's `docs/`
holds explanations that span subjects.

### Why is a page plain Markdown?

So it reads the same on GitHub and on the website, and a translation
never has to step around code. There is no MDX or JSX: the
[conventions](guide.md#add-a-page) are the whole of what the renderer
adds.

### Why is a page's description its first paragraph?

Because a page then needs no front matter, which GitHub shows as a table.
The paragraph that opens a page is what search results and link previews
say of it.

### Why is a link between folders a path to the page's file?

So a reader on GitHub follows it to the page's file and stays in the
repository, and the check can tell a page that's gone. The website
turns the path into the page's URL in the reader's language, or into
its address on another website.

### Why are the docs checked on their own?

So a change to a page is checked in seconds. The checks that read only
files are one small crate, `tests/docs`, with no Link dependency: it
reads the workspace once and checks each file on its own thread. The
checks that need Link's types, a page's specification blocks and `lnk` commands,
are `tests/docs/code`, which reads the libraries that own those types
and the core's registry, never a built program.

### How can a page change be checked independently?

Selecting `link-docs-check` and `link-docs-check-code` runs the
documentation checks without selecting every workspace test. Code
changes still need these checks because a renamed command or specification
field can break a page. The public CI workflow runs its configured
workspace checks on every pull request into `main`.

### Why is each limit stated in one file?

So a number can't say one thing in the checks and another in the
guide. `tests/docs/limits.toml` holds each limit with its reason; the
checks read it, and the writing guide's table must match it. Changing
a limit changes both in one pull request, or fails.

### Why can a file over its limit shrink, but never grow?

Because splitting every large file at once is a project of its own,
and a limit that fails on the day it lands gets turned off. Each
workspace's `large-files.toml` lists the files over their limit when it
came in, with their length then. A new file over its limit fails, and
so does a listed file that grows; the others are warnings, which a
split clears one at a time.

### Why does every package have a README?

So a reader who lands in a folder learns what it holds without opening
its `Cargo.toml`. Each package, and each folder holding two or more,
has a README hub: a sentence, and links one level down, to its docs and
its children. A hub never links past a folder with a hub of its own, so
each says only what's one level below it.


## Addresses and names

Why a page's address is what it is, and what Link and Local Link name.

### Why does a page's URL follow the tree?

Because a URL should say where a page is, the way the site does:
`/harnesses/openclaw/security` reads as Harnesses, then OpenClaw, then
Security. A file's path in its folder's `docs/` is still its URL, so
a folder holds its pages at their place in the tree
(`interop/agents/harnesses/openclaw/docs/agents/harnesses/openclaw/`).

### Why is English unprefixed?

So each page has one address: `/agents` is the page, and `/en/agents`
redirects to it. Other languages keep their prefix.

### Why is Link the command line and Local Link the rest?

So a name says what it is. The command line and its modules are Link
("Link's storage module"), and the company, the product, the website, the
repository and what it is for are Local Link. The command is `lnk`.


## The site's tree

Why the pages are a tree, and how it's ordered beside every page.

### Why is the site a tree?

Because a reader learns a few ideas at a time, and looks for a page by
what it's about, not by which folder holds it. The tabs introduce
the site's main subjects, and each opens a few more pages. So
`navigation.md` holds the tree, and a page outside it would leave the
reader lost, so the build refuses one.

### Why a tree beside the page, not rows of pills?

A tree shows each page within the whole site's hierarchy. The active page
is highlighted, its ancestors stay visible and other sections fold with
their page counts. Page headings appear beneath the active page. The map
stays in place across tabs, and a hamburger opens it on narrower screens. Tabs keep their menus for direct access to the next level.

### Why does a folded tree never flash?

A reader can fold the tree away on a wide screen, and the choice is kept
in the browser. A script at the top of the page applies it before the first
paint, so a page never draws the tree and then hides it. On a narrow
screen the tabs' row keeps its place from page to page and glides to the
tab just picked.

### Why at most ten under a heading?

A reader takes in about ten things at a glance; past that, a list is
something to search. So no page has more than ten pages under it in the
tree, `navigation.md`'s `## Pages`, and no heading more than ten headings
under it: a longer list is grouped a level deeper.

### Why do a part's specifications, Decisions and Security come last?

They describe the part beside them, so they sit with it, after a line:
what it does first, then exactly what it reads, why it is so, and what
it guarantees. A reader learning the part never has to step over them.


## The build

Why the websites are built and kept as they are.

### Why separate navigation, page content and interface labels?

They have different owners. Each page owns its explanation, title and
description; `navigation.md` owns placement and reading order; shared
locale catalogs own interface labels. The website renders those
sources. README indexes are alphabetical instead, since a folder's map
is read to find a child by name. A content edit or translation has one
authoritative home.

### Why is the site built once?

Next.js renders every page at build time, and nothing the build makes is
committed. The docs are the only source, so a generated copy could
disagree with them.

### Why are the websites their own folder?

The websites and the command line share nothing but the docs. In its
own folder, each builds without the other's tools, and a hosting provider
can build only the site it serves.

### Why is the shared package apart from the app?

Websites differ in who reads them, so the docs' tabs are the documentation's
sections, and another website has tabs of its own. They share one
package, so every component and the look are the same. The
documentation index names a website's folders, so the apps, the scripts and the docs
checks read which folders are whose from one file, and an index can include
another repository's to link to its websites.
