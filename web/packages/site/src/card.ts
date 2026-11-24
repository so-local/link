// The look every website shares, as Tailwind classes: look.ts says the
// same parts as values, for an app that can't run Tailwind, and
// look.test.mts holds the two together. The column every page sits in, set
// once in the root layout, so the tabs never move when you switch them:
// centred on any screen, with the site's tree beside the page once it's
// wide.
export const column =
  "mx-auto flex w-full max-w-3xl flex-1 flex-col gap-4 px-4 pt-4 pb-6 sm:px-10 sm:pt-8 sm:pb-10 lg:max-w-[94rem] lg:px-6 lg:pt-16";

// On a wide screen the column is three: the tree, the cards and a space
// as wide as the tree, so the cards keep one width and one center on every
// page and in every app, with a tree beside them or not. A page's cards
// go in the middle one.
export const spread = "flex flex-1 flex-col gap-4 lg:grid lg:grid-cols-[clamp(10rem,18vw,16rem)_minmax(0,48rem)_clamp(10rem,18vw,16rem)] lg:justify-center lg:gap-6";
export const middle = "flex min-w-0 flex-col gap-4 lg:col-start-2 lg:row-start-1";

// The home page's cards, as wide as any page's.
export const alone = "flex flex-col gap-4";

// Every other page's cards, wide enough for code and tables, beside the
// tree of its tab on a wide screen (nav.tsx).
export const body = "flex min-w-0 flex-col gap-4";

// Frosted white cards: nearly solid and strongly blurred, so text reads
// easily over the flowers while their colors still show through softly.
// The tree and the tabs' menus are frosted the same, with less room.
export const frost =
  "rounded-2xl bg-white/90 text-zinc-900 shadow-sm ring-1 ring-black/5 backdrop-blur-xl contrast-more:bg-white";
export const card = `${frost} p-6 sm:p-8`;

// The home page's headline card: its tagline is the one heading set
// narrow enough that each of its sentences starts a line, and
// larger than every other page's.
export const headline = `${card} prose prose-zinc max-w-none sm:p-8 prose-h1:mb-4 prose-h1:max-w-[6.8em] prose-h1:text-4xl prose-h1:font-semibold prose-h1:tracking-tight sm:prose-h1:text-5xl prose-p:text-lg prose-p:leading-8 prose-p:text-zinc-700`;

// A card of Markdown, the same on every page. Its first line starts at
// the card's padding, whatever it is, a heading sits close above what it
// heads, and a table reads at the size of the text around it.
export const section = `${card} prose prose-zinc max-w-none sm:p-8 [&>:first-child]:mt-0 prose-headings:tracking-tight prose-h1:mb-4 prose-h1:font-semibold prose-h2:mt-8 prose-h2:mb-2 prose-h3:mb-2 [&_:is(h2,h3)+*]:mt-0 prose-a:underline-offset-2 prose-code:before:content-none prose-code:after:content-none prose-table:text-base`;

// White pills: the tabs and the waitlist button. They fade to solid white
// on hover.
export const pill =
  "rounded-full bg-white/80 text-zinc-700 shadow-md shadow-black/15 ring-1 ring-black/5 backdrop-blur-md transition-colors duration-200 ease-out hover:bg-white hover:text-zinc-900 contrast-more:bg-white";

// What you press (look.ts's `shape`): a button or a tab, a raised pill
// at the copy's size, its words in its middle.
export const shape = "inline-flex w-fit max-w-full items-center gap-2 px-5 py-2.5 text-base font-medium";
const centred = "justify-center text-center";

// Every action uses the same pill, including standalone links.
export const action = `not-prose ${shape} ${centred} ${pill}`;

// A page of a section, in its row or its list: the current one dark, as
// its tab is.
export const current = "rounded-full bg-zinc-900 text-white shadow-md shadow-black/15 ring-1 ring-zinc-900";

// A dark pill that does something: a form's main button.
export const dark = `${current} transition-colors hover:bg-zinc-700`;

// A button: a white pill, or the dark one for a form's main action, the
// same size as every action.
export const button = `${shape} ${centred} ${pill}`;
export const primary = `${shape} ${centred} ${dark}`;

// A tab: the same shape, a white pill or the dark one for the page
// you're on.
export const tab = `${shape} ${centred}`;

// The tones (look.ts's): the only colors besides the greys. One yellow,
// the sunflower's, see-through, for what's on offer and what to see to
// alike.
const sunflower = "bg-sunflower/30 text-zinc-900 ring-sunflower-dark/50";
export const tones = {
  offer: sunflower,
  note: "bg-zinc-100 text-zinc-800 ring-zinc-200",
  good: "bg-emerald-50 text-emerald-900 ring-emerald-200",
  warn: sunflower,
  bad: "bg-red-50 text-red-800 ring-red-200",
} as const;

// What you read (look.ts's `tag`): a flat, see-through pill in a tone
// with no shadow, so it never looks pressable; what's on sale and what
// a form answered alike.
export const tag = "inline-flex w-fit max-w-full items-center gap-2 rounded-full px-3 py-1 text-base font-medium ring-1";

// A card's words (look.ts's `words`): its heading, a heading inside it,
// its copy, a quieter line, and a term over its value.
export const heading = "text-2xl font-semibold tracking-tight text-zinc-900";
export const subheading = "text-base font-semibold text-zinc-900";
export const copy = "text-base text-zinc-700";
export const quiet = "text-base text-zinc-600";
export const term = "text-base text-zinc-500";
export const label = "text-base text-zinc-900";

// A box inside a card, an item in its list or a pick among several, its
// ring dark when picked; and a control while it can't be used.
export const inset = "rounded-xl bg-white/70 p-4 text-zinc-900 ring-1 ring-zinc-900/10";
export const insetPicked = "rounded-xl bg-white/70 p-4 text-zinc-900 ring-1 ring-zinc-900";
export const disabled = "disabled:opacity-60";

// A field to type in: a white pill, a rounded box when it takes lines.
export const field = "w-full rounded-full bg-white px-4 py-2.5 text-base text-zinc-900 ring-1 ring-zinc-900/15 focus:outline-none focus-visible:ring-2 focus-visible:ring-zinc-900";
