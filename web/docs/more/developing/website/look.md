# Look

One set of values draws every Local Link interface, from its frosted cards to
its pills, tones, type, and wordmark. A website spells them as Tailwind
classes, and an app that can't run Tailwind reads the same values, so the two
match to the pixel.

## Parts

A page sits in one column under the wordmark, the sunflower beside the name
`local.link`. Every part on it is one of a few kinds, below.

### Column

The column sets where cards sit and how wide they are, the same in every app.
On a phone, cards fill the screen less a 16-pixel margin each side. From 640
pixels wide the margin is 40 pixels, up to 768-pixel cards. `cardsWidth` gives
the cards' width for a screen's width and the page's, which is the screen's
less any scrollbar.

### Three Columns

From 1024 pixels wide the column holds three. A side column on each side is
18% of the screen's width, between 160 and 256 pixels, and the cards sit
between them, 24 pixels apart, at most 768 pixels wide. A website shows its
page tree in the left side column, and a page without one leaves both sides
empty, so the cards keep one width and one center either way.

### Cards

Content sits in frosted cards: white at 90%, a 24-pixel background blur,
rounded 16-pixel corners, a faint one-pixel ring, and a small shadow. Text
reads easily over the flowers, and their colors still show softly through.

### Pills

White pills are the tabs and the buttons. Each is white at 80% with a
12-pixel blur, a ring, and a deeper shadow, and it turns solid white with
darker text under the pointer. The dark pill marks the tab you're on and a
card's main action. Every button has one size, and every tab another, so two
apps never disagree about how tall a button is.

### Insets and Fields

An inset is a softer box inside a card, for one item of a list or one pick
among several, with a dark ring when picked. A field is a white pill to type
in, rounded like a card when it takes several lines.

### Tones

Tones are the only colors besides the zinc greys. A notice, a full-width line,
or a badge, a short label, takes one of four. Note, in zinc, is news that's
neither good nor bad. Good, in emerald, says something went well. Warn, in
amber, is something to see to, and bad, in red, says something failed. An
interface never picks another color for a label, a price, or a state, so a new
kind of message takes one of these tones, or the look gains a tone first.

### Messages and Actions

A message and the button that acts on it each take their own line. The
message is a paragraph, and its button sits below it, never at the end of the
sentence. "This didn't load." is one line, and Try Again is
the next. A button on its own line reads as a choice, while one trailing a
sentence reads as part of it.

## Words

Every interface writes in Geist, the websites' font, at three weights:
regular for text, medium for buttons and badges, and semibold for headings.
Text sizes follow Tailwind's scale. A card's heading is 24 pixels, set tight.
Its copy is 16 pixels in a dark grey, and a quieter line is 14 pixels in a
lighter one.

### Casing

Headings, buttons, and tabs are in title case: "Your Agents", "Create an
Agent", "Back to Agents". Short joining words such as "a", "of", and "with"
stay lowercase inside a title. Everything else, a badge, a notice, a label,
or a sentence, is in sentence case. No interface writes a label in capitals or
spaces its letters apart.

### Names

The wordmark is `local.link`, lowercase, only in the header. On every
website, the wordmark links to the home page,
`https://www.local.link`, the look's `brand.home`. Prose names the
company and product Local Link, in title case. A product is Link and its name,
such as Link Harness or Link Models, never Local Link Harness.

## One Look, Two Spellings

The look's values live in `@link-web/site/look` (`web/packages/site/src/look.ts`),
which imports nothing, so any app can read it. It names each part in
Tailwind's own words: a card's fill is `white` at 0.9, its radius `2xl`, its
shadow `sm`. Beside it, `@link-web/site/card` (`card.ts`) spells each part as
Tailwind classes, such as `rounded-2xl bg-white/90 shadow-sm`.

### Tailwind

Tailwind CSS builds a stylesheet from the class names a page uses, each class
one style, such as `rounded-2xl` for 16-pixel corners. Tailwind defines its
colors in OKLCH, a color space browsers convert to the screen's sRGB as they
paint. The look keeps each color it uses in sRGB hex, the value a browser
paints.

### React Native

React Native draws native views from JavaScript and has no stylesheet, so it
can't read Tailwind's classes. An app built on it styles each piece from the
look's values instead. `css` turns a part into plain style values: a hex or
`rgba()` color, a radius, and a box shadow with the ring first, as Tailwind
stacks them. A background blur is CSS, so it shows on the web, and a phone
draws the rest.

### Held Together by a Test

`look.test.mts` reads Tailwind's own theme from the installed `tailwindcss`
package. It checks that every color converts to the look's hex and that every
size, radius, shadow, and blur matches. Then it checks that each class string
in `card.ts` holds the classes the look's recipe for that part names. It also
checks that the wordmark's CSS in `globals.css` uses the look's sizes, and that
`cardsWidth` gives the widths a browser measures for the column's classes. A
change to one spelling without the other fails `npm test`.

## Change the Look

Change a value or add a part in `look.ts` first, then spell it in `card.ts`,
and run the package's tests. A website picks up the class. An app styled from
the look picks up the value, with nothing of its own to change. Run `cd web/packages/site`, then `npm test`.

<!-- docs-tree:start -->

## Docs

- [The Parts](#parts): A page sits in one column under the wordmark, the sunflower beside the name `local.link`
- [Words](#words): Every interface writes in Geist, the websites' font, at three weights: regular for text, medium for buttons and badges, and semibold for headings
- [One Look, Two Spellings](#one-look-two-spellings): The look's values live in `@link-web/site/look` (`web/packages/site/src/look.ts`), which imports nothing, so any app can read it
- [Change the Look](#change-the-look): Change a value or add a part in `look.ts` first, then spell it in `card.ts`, and run the package's tests

<!-- docs-tree:end -->
