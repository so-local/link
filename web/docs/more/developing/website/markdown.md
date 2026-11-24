# Markdown Pages

Write pages and card collections with the shared Markdown formatter. The
documentation site and other websites use the same syntax and page metadata.

## Write a Card Grid

Wrap an unordered list of page links in a `cards` container directive. Each
card reads its title, introduction and primary image from the linked page,
so changing that page updates its cards without editing every collection.

```markdown
:::cards
- [Websites Guide](guide.md)
- [Decisions](decisions.md)
:::
```

### Grid Layout

Cards fill the available width in one, two or three columns. Each card has
one keyboard-accessible link, a primary-image area, a title and an introduction
shown in up to three lines. Missing or unreadable images show shared artwork.

### Page Links

Each entry must contain exactly one direct Markdown link to a published page
of the same site. Relative links and language-specific website paths resolve
through the site's normal page index. Invalid entries fail the build rather
than producing incomplete cards, and examples inside code fences stay code.

## Set Page Metadata

The first level-one heading supplies the title, and the opening paragraph
supplies the introduction. Optional YAML frontmatter overrides either field
and supplies a primary image. Unknown fields, empty values and duplicate
fields are refused.

```markdown
---
title: Portable Agents
description: Keep your agent while changing its harness, model or machine.
image: https://images.example/portable-agents.jpg
---

# Portable Agents

Your full introduction can explain more than the card needs to show.
```

### Primary Images

Use a public HTTP image URL or a root-relative path to an app asset. A relative
image path must name a declared website asset, following the
[shared homepage's asset mapping](guide.md#shared-homepage). The same image
appears in collection cards and the page's Open Graph and Twitter metadata.
Images are cropped to a 1200 by 630 frame in cards.

## Format Documentation

The formatter preserves headings and their anchors, GitHub-flavored tables,
lists, images and fenced code. Shell blocks have a copy button, and links
titled `button` render as pills. Horizontal rules divide ordinary pages into
cards, while raw HTML and YAML frontmatter stay out of the rendered prose.

```markdown
[Get Started](guide.md "button")
```

## Supply Content

The [formatter](../../../../packages/site/src/format.ts) takes a Markdown
tree and a caller-supplied page lookup for card metadata. It never reads
files or contacts storage. The documentation reader supplies repository pages,
and a website can supply a verified bucket snapshot through the same reader.
Navigation, breadcrumbs and pagination remain separate from page formatting.

## Explore the Website

These pages use the same card-grid directive as any other collection. Their
metadata comes from their own Markdown, without a separate catalogue of copy.

:::cards
- [Websites Guide](guide.md)
- [Decisions](decisions.md)
:::
