# Formatted Answers

An agent writes its answers in Markdown, and each connection shows
them in its chat's own formatting: bold, code, links, lists, and
tables as the chat draws them. The contract's `rich` module reads the Markdown
once, cuts it into messages the chat takes, and leaves only the
drawing to each connection.

## Markdown Read Once

Markdown is the plain-text markup most agents answer in, and
CommonMark is its common specification. `rich::Document::parse` reads
an answer as CommonMark with tables, strikethrough, and task lists,
through `pulldown-cmark`, a widely used Rust parser of it. The result
is a `Document`: a list of blocks (a paragraph, a heading, a code block
with its language, a list, a quote, a table, or a thematic break), each
paragraph a list of spans.

### Spans and Marks

A span is text marked the same way throughout, with its marks
outermost first: bold, italic, strikethrough, inline code, or a link to
its URL. Raw HTML in the answer stays text, shown as written, and an
image is a link to it. Space at the edge of emphasis is left outside
it, since a chat that marks with characters, such as `*bold*`, draws
nothing for `* bold *`.

### An Answer Cut Short

An answer being written is read the same way, so each edit shown while
it's written is whole. An open code fence is code to the end, and
stars not yet closed are text. A connection never truncates its own
markup to fit.

## Cut to Fit a Message

`rich::pieces` cuts a document into messages, each within the chat's
limit as the chat counts it: in characters, or in UTF-16 code units,
where an emoji outside the Basic Multilingual Plane counts two. Whole
blocks go together while they fit, measured as the connection renders
them, markup included.

### Blocks Longer Than a Message

A block longer than one message is cut where its structure allows:

- A paragraph or heading between words, each part keeping its marks.
- A code block between lines, each part a code block of its own, so a
  fence is closed and opened again.
- A table as a code block of its aligned lines, cut the same way.
- A list between items, numbering on from where it stopped.
- A quote between its blocks, each part quoted.

Each piece keeps its blocks beside its rendered text, so a connection
can render the same piece again another way, such as plain text. A
piece doesn't change once its blocks are complete, so an answer shown
while it's written mostly edits its last message.

## Drawn by Each Connection

A connection implements `rich::Dialect`: how it renders a piece's
blocks, the text whose length its chat limits, and its `Channel`, that
limit and how it's counted. Each connection reads its `Channel` from a
`formatting.toml` beside its code.

### Characters and Tags

A chat that marks text inline, with characters or tags, implements
`rich::Markup`: how it escapes the agent's text, opens and closes each
mark, and writes code, a link, a heading, a code block, a quote, a
bullet, and a break. `rich::markup` lays out the rest the same for
every such chat. Blocks are a blank line apart, a list's items a line
each, and their later lines indented under the bullet. A quote is
wrapped in tags or written after a prefix on each line, as the chat
says.

### Text With Styles

A chat that takes plain text, with the ranges of it styled sent beside
it, uses `rich::styled`. A heading is bold, code and tables are
monospace, a link's URL follows its text, and a list's items follow
bullets. Each style's range is in bytes of the text, and
`Styled::utf16` counts it in UTF-16 code units instead. Any connection
uses the same text when its chat refuses its markup.

### Tables

No chat draws a Markdown table, so each shows one as aligned text in a
monospace font: each column as wide as its widest cell, the head ruled
off from the rows (`Table::monospace`).

## Links Only to the Web

A link is written as the chat's link only when `rich::web_url` allows
its URL: a scheme `rich.toml` names (`http`, `https`, or `mailto`) and
no control character. Each character that could end the link early in
a chat's markup, such as `|`, `<`, or `)`, is percent-encoded. Any other
link is shown as its text and then its URL, as text, so an answer can't
hide a link a chat would follow.
