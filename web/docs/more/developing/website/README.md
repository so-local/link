# Websites

> Websites Built from Each Component's Docs

Change the documentation website using the repository’s Markdown pages and
shared navigation outline. The website is an app of the web workspace, built on
a package that other Local Link websites share.
Page metadata and card collections work across the websites, as
[Markdown Pages](markdown.md) explains. Every page is drawn over an animated
field of sunflowers, which
[Flowers](../../../../packages/flowers/docs/more/developing/website/flowers/README.md)
renders and configures. Every Local Link interface shares one
[Look](look.md), its cards, pills, tones, and type, from one set of values
that websites and apps alike read.

<!-- quickstart -->

## Start the Development Server

Install the web dependencies and start the development server.
The docs site runs on port 3001, as [Develop](guide.md#develop) explains. Run `cd web`, `npm install`, then `npm run dev`.

## Check the Website

From the web workspace, check code, navigation, and shared rendering behavior.
The checks for website changes are in [Before Pushing](guide.md#before-pushing). Run `npm run lint`, then `npm test`.

## Build the Website

Build the site and validate its generated pages.
Page ownership, navigation, and indexes are covered in [Add a Page](guide.md#add-a-page). Run `npm run build`.

## Find Design Decisions and Moved Pages

Shared page ownership, addresses, and the reading tree are explained in
[Website Decisions](decisions.md). Redirect declarations keep links working
after a page changes address, as [Moved Documentation](redirects.md) describes.

<!-- docs-tree:start -->

## Docs

### Pages

- [Websites Guide](guide.md): Change the documentation website, add a page to it, or translate it
- [Markdown Pages](markdown.md): Write pages and card collections with the shared Markdown formatter
- [Look](look.md): One set of values draws every Local Link interface, from its frosted cards to its pills, tones, type, and wordmark
- [Moved Documentation](redirects.md): Permanent redirects preserve existing documentation links when a page moves to a new address, including its child pages and language-prefixed addresses

### More

- [Decisions](decisions.md): Why docs.local.link is built the way it is

<!-- docs-tree:end -->
