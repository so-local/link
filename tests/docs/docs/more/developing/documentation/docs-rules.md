# Docs Rules

Every rule the docs checks hold a page, README or source file to, by its
id, so a writer sees each rule and its limit before a check fails.

## How the Rules Are Checked

Each rule names its check in `tests/docs` (the text) or
`tests/docs/code` (what needs Link's types), and its numbers come from
`tests/docs/limits.toml`. The [hook guide](hooks.md) applies when
writing or changing a hook. Generated `artifacts/` directories are excluded
from source discovery, so reports never become README navigation. YAML
frontmatter is metadata rather than prose, and `:::cards` delimiters wrap a
normal list whose links remain checked.

### Declared Data

Folders declared in `large-files.toml`'s `data` are inputs or records
consumed as a whole. Their Markdown remains raw data for inventories and
line checks, without joining current navigation or contributing outgoing
documentation links. Links from current pages to those files and headings
still resolve, and adjacent paths outside each declared folder remain
current documents.

## Every page of the website

These rules hold every page the websites publish, by what a reader sees
first: openings, sections, lists, tables, code and links.

- Every authored heading opens with a paragraph (`heading-paragraph`),
  before any list, table, code block or the next heading. A generated
  Docs index begins directly with its links; it needs no repeated introduction.
- A specification kind's reference page keeps its generator's layout, a heading
  per field opening with its quoted line, and a command reference has a
  heading per resource. Each is one page, however long:
  `limits.toml`'s `[spec-pages]` and `[reference-pages]` name the rules,
  such as `markdown-lines` and `headings`, that don't hold them.
- A specification page or command reference generated from declarations says
  each entry as its declaration's help does, as deep as they make it.
  `[spec-pages]` and `[reference-pages]` name the rules that don't hold
  it for that: `duplicates`, `depth`, `item-length`, `thin-section` and
  the two click rules.
  The `item-length`, `thin-section`, `duplicates` and `depth` rules
  don't hold it.
- A generated page's kind has a doc comment on every field and an
  `about`, its Every Option reads back through its reader, and its
  module's section page links it in its opening (`references`).
- One paragraph a section (`one-paragraph`): two in a row get a
  heading between them.
- At most 600 characters in a paragraph (`paragraph-length`), link
  targets left out: split a longer one into subsections.
- A section's lone paragraph has at least 80 characters (`thin-section`):
  fold a thinner one into the section beside it. A page's opening and a
  License section are exempt.
- At most 60 characters in a table cell (`cell-length`): a row that
  needs explaining is a subsection.
- At most 400 characters in a list item (`item-length`): one that needs
  a paragraph is a subsection. No limit on how many items or rows.
- A list is three or more like things, or links (`list-items`): fewer
  fit in a sentence. A short item may lead with its name in bold.
- A table compares two rows or more (`table-rows`): one row is a
  paragraph.
- Keys, flags and commands are subsections, never a table's rows
  (`table-code`): a table whose rows each open with inline code fails.
- A row is scanned by its first word (`table-articles`): a table's
  first cells don't open with "a", "an" or "the", but one catch-all row.
- A section's page, one with pages under it in `## Pages`, has a hook
  (`hooks`): a one-line quote right under its plain title. Documentation
  shows the short title; the marketing site may show the quote as its
  heading. No other page has one. A section in
  `large-files.toml`'s `unhooked` has none yet: none joins.
- At most 45 characters in a hook (`hook-length`), opening with its
  keyword, never "it", "its", "a" or "the", and holding no colon. Before writing one, read
  [the hook guide](hooks.md).
- A hook is a title, so it's in Title Case (`hook-case`), and it says
  more than its page's name (`hook-words`, 3 words at least).
- Code is in backticks: every command and what's typed in a terminal
  (`bare-command` fails a bare `lnk` command), environment variables
  (`bare-variable`), paths and file names (`bare-path`), types,
  functions, crates and programs, settings' keys and values, hosts and
  ports that aren't links, what a user types to the agent, and output
  to match. Names of products, parts and ideas are not.
- A module a user acts on opens its first page with a quickstart
  (`quickstart`): an `sh` block before the first `##`, or inside the first
  task section introduced by `<!-- quickstart -->`, of the commands that
  get it working. It ends at the first thing that works.
- A quickstart assumes only `lnk` installed and [`lnk up`](../../../../../../docs/more/modules/install.md#pick-your-modules) run, so
  everything else it needs comes first, in order, such as [`lnk agent start`](../../../../../../modules/agents/agent/docs/agents/README.md#create-an-agent) before [`lnk agent connect slack`](../../../../../../modules/agents/agent/docs/agents/README.md#connect-your-channels), and a command installs its own
  module. One only Link runs says `quickstart = false` in its
  `module.toml`'s `[documentation]`.
- A quickstart sets no environment variable (`quickstart-env`): each
  command runs alone and asks for what it needs, as `lnk agent start`
  asks for a model's key.
- A terminal block holds `lnk` and `curl` commands alone, one a line
  (`terminal-blocks`): no other program, comment, pipe, redirect,
  heredoc or line continued with a backslash. The one pipe is the
  install line, `curl -fsSL https://local.link/install.sh | sh`. Another
  program's command goes in prose, as inline code. It holds every page
  of both repositories.
- A file is its own fenced block, never written by a shell command
  (`file-blocks`), and the prose says where it lives. Its one comment is
  its path, as its first line, `# ~/Link/Safe/Agents/main/agent.toml`:
  what a key means goes in the prose around it.
- A quickstart's file is marked `<!-- quickstart-file: env.toml -->`,
  inside `<!-- quickstart:steps -->` and `<!-- quickstart:end -->`, so a
  fixture running the steps writes the same file.
- Link writes only under `~/Link` and `~/.lnk` (`output-paths`): an
  `lnk` command's `--out`, `--to` or `--dir`, and `LNK_INSTALL_DIR`, in a
  page's `sh` block name a folder there, or are left out for the layout's
  default.
- Troubleshooting is a page of its own (`troubleshooting`):
  `troubleshooting.md` beside the feature's page, one heading per
  symptom with its fix. `decisions.md`, `security.md` and
  `troubleshooting.md` have no line limit: they're grouped by topic in
  sections and subsections, and never name a run, a date, a person or
  anything private.
- A page in `large-files.toml`'s `unshaped` (`unshaped`) was written
  before these: none joins, and a page leaves once it keeps them.
- Paragraphs and subsections come before lists and tables; a list or
  table is for short parallel items or for links. A page opens with a focused
  quickstart block; every other command is under a heading of
  its own. Each specification key, flag or setting is a subsection: its name the
  heading, a paragraph on what it takes and its default.
- Present tense (`history`): how it works now. No dates of
  development, no "we tried", "it used to", "decided by".
- Links resolve (`links`): every relative link and anchor, in every
  page and every document the README indexes hold, in any workspace; a
  link to a page is a relative path to its file, never its address on a
  website.
- At most 10 headings under a heading (`headings`), and 10 at a page's
  top: group more a level deeper, keeping their words. Generated index
  headings are outside the authored heading budget.
- An abbreviation is learned along the way (`abbreviations`): a page
  spells it out where it first uses it, as "Agent State Protocol (ASP)",
  and uses only the abbreviation after. Headings may spell it out. The
  checked abbreviations are `tests/docs/abbreviations.toml`'s.
- One word per meaning (`words`): a phrase `tests/docs/words.toml`
  lists, from `AGENTS.md`'s Words, such as `signing key` for a signer,
  is never in a page's prose. Among them, "ships" never says what Link
  offers (`Link ships`, `ships with`, `the shipped`): Link comes with
  harnesses, includes its model facts and provides ready-made agents,
  while a release still ships.
- IAM's pages never say a retired word (`retired-words`): in the folders
  `tests/docs/iam-pages.toml` lists, no phrase of `words.toml`'s
  `[[retired]]`, such as `sign in` for log in, each in its retired sense
  alone, so "public key" and "assign" still read.
- A page's title names its subject, never a pronoun (`title-pronouns`):
  `words.toml` lists "it", "they" and their forms, since a title stands
  alone in a tab, a search result or another page's index.
- Headings start with their subject or action, never
  "a", "an", "the", "it", "its", "it's", "this", "that", "these" or "they".
- At most 10 pages under a page (`pages`) in `navigation.md`'s `## Pages`,
  and one page at each URL. `[limit.pages]`'s `except` list gives a page,
  by its address, a higher number and a `why`. Agents, `/agents`, and its
  Tools, `/agents/tools`, hold 12 as the product's center with more
  first-class pages, and no other section has an exception.
- A page with pages under it holds at least 3 (`group-pages`): fold one
  or two into it, or move them beside it, so no level opens for a page or two.
- A page's children are read by their kind, `tests/docs/kinds.toml`'s
  order: the guide, the section's own pages, developer pages (More,
  contracts, interfaces, developing, command line and adapters), generated
  references, then decisions, security and troubleshooting. The tree
  writer orders them so, keeping the curated order within a kind, and the
  `navigation` rule holds the outline to it.
- A guide, decisions, security or troubleshooting page holds no pages
  (`kind-leaves`): what it holds is one page's subject.
- A module with commands of its own, so a generated command reference,
  has a `guide.md` and a `troubleshooting.md` in its `docs/`
  (`module-pages`): where a reader starts, and where they go when it breaks.
- Website prose stays in docs (`website-words`): no Markdown in its
  `src/` or `web/packages/site/src/`. Each app's `docs/navigation.md`
  holds structure and order; shared locale catalogs hold interface text.
- A translation (`translations`) is `<page>.<language>.md` beside its
  English page, its language one of `packages/component/languages.toml`'s,
  and its first line names that English and the hash it
  was made from. While the English is unchanged, it keeps the English's
  headings and code blocks: edit the English, never a translation.
- A page is introduced in a paragraph of the page at its address's
  parent (`introduced`), the nearest that's a page: a sentence there
  says why it exists and links to it. A list, a table or a generated
  index is no introduction, so the links in prose alone reach every page.
- A paragraph is on one page only (`duplicates`, from 120 characters):
  keep it where its subject is owned and link there.
- A heading is one level below the heading it's under
  (`heading-levels`): no `###` right under a `#`.
- A page's deepest heading is at most 7 levels down (`depth`) from
  the home page: its address's segments, then a level for each heading
  below its title, since the address and the headings are one tree.
- A package's pages are under the address its enclosing package's
  pages share (`follows-code`), so the docs' tree follows the code's.
  A typed configuration reference also establishes a logical home
  for that owner's configuration pages through the checked specification
  catalogue; unrelated pages still follow their enclosing package.
- A page or package these structure rules held when they came in is
  in its workspace's `large-files.toml` under `[held]` (`held`), by the
  rule's id: none joins, one leaves once it keeps the rule, and an
  entry whose rule or file is gone fails.
- Design rationale goes in a `decisions.md` beside the page (`decisions`),
  never a `## Decisions` section; why someone would use the feature
  belongs in its main explanation.
- A website app's own pages (its `docs/`, outline aside) put every
  heading in Title Case (`title-case`): each word capitalized but the
  short words inside it.
- No page is titled `# Get Started` (`get-started`): a section's first
  page has a short pitch of its own.
- A page opens with a paragraph under its title (`opening`): its first
  sentence is what every README linking the page says of it.
- Terminal explanations are prose, never padded inline comments (`comments`). The `zsh` rule still applies to shell syntax.
- A comment pastes into zsh (`zsh`): plain words, none of
  `' " ? * [ ] ( ) { } ; | & < >` or a backtick, and none after `--`.
- Repository layout inventories list only existing source folders
  (`repository-layout`). List and table entries in Layout, Source Tree,
  Folder Structure, or Repository Organization sections resolve inside
  this workspace or the owning component. Placeholders match existing
  folders; omit absent entries rather than explaining them.
- Every `lnk` command a page gives exists (`commands`), in a shell block
  or in inline code.
- A flag on an explicit command example exists on that command or its
  parents (`flags`), including continued shell lines. Flags after `--`
  belong to the invoked program. Standalone flag mentions need enough
  context for a reader to identify their command.
- Provided literal values use the command's actual value parsers (`arguments`).
  Complete shell examples also satisfy required arguments and conflicts.
  Inline fragments may omit arguments; placeholders are not literal values.
  A deliberately rejected shell example says `sh expect=error`.
- Production Rust reads of static variables have declarations in the reading
  crate's `variables.toml` (`environment-reads`): a definition, or a use of a
  variable another crate defines. Each variable has one definition, and a
  flag's fallback names a defined one. Dynamic namespaces and caller-selected
  reader functions need explicit declarations, and a wildcard never permits
  an undeclared literal.
- Every JSON block names its owning released type or schema (`json`) with
  `<!-- json: owner-id -->` immediately before its fence. Show complete valid
  values, not ellipses or pseudocode, and reject unknown or mistyped fields.
- Published specification references bind to their actual types (`references`). Generate
  their field and checked-default summaries with the inventory writer. Shared
  sections name their owner; proposed formats visibly say they are unimplemented.
  Preserve the authored explanations around generated summaries.
- Every TOML block a page shows is a specification Link reads (`specs`), by the
  page's kind: box, sandbox, agent, environment, job or policy. A block
  whose first line names its file, as `# types.toml` or `# schema.toml`,
  is read as that file.
- The writing guide's table of limits is `limits.toml`'s (`limits`).
- A specification kind's reference page is written whole from its module's
  `types.toml` by `link-actions-surfaces`, with the module's command
  reference, and the references check refuses one absent, stale or
  edited by hand (`references`).
- Source-backed inventories match their declared sets (`inventories`):
  regenerate with `cargo run -p link-docs-check-code --bin lnk-docs-inventories -- --write`
  after changing commands, declared variables, credentials, specification references or shared state locations.
  Credential kinds come from the runtime catalogue and retain owning code and docs.
- A command a person runs, on the command line and not plumbing, names
  the section explaining it (`command-pages`), `docs =
  "<address>#<anchor>"` in its `actions.toml`, and that section shows it
  as `lnk <command>`, outside the generated indexes and inventories. A
  primary command's section is a heading of its own.
- Each verb a module's public commands start with has a page or folder
  of its own in its `docs/`, named for the verb (`verb-pages`): a
  suggestion, so it warns and never fails.
- A page's first mention of a command another package owns links to
  that section (`owner-links`), so a page explains its own function and
  sends the reader to the owner for the rest. `lnk-docs-inventories --write`
  inserts the links.
- A command group whose word is no approved verb, such as `lnk box
  image` (`command-folders`), has a folder named its word in its
  owner's docs, whose pages show it: a major command gets room of its own.
  The owner is the package containing its declared documentation section.
  An agent command driving a tool uses that tool's existing folder.
- An inventory lists what exists; a page says what it's for. Every
  `LNK_*` variable (`variable-pages`) and every folder of Link's state
  (`state-pages`) is named in backticks on a page beyond its
  inventory. What these three held when they came in is under
  `[held]`, by `<declaring file>#<name>`.
- Every tracked TOML file is of a kind in `tests/docs/code/files.toml`
  (`files`), and each kind with a notice, `module.toml` and `layer.toml`,
  opens with the header the catalogue writes, as does the Repository
  Files page's lists: regenerate them with `lnk-docs-inventories --write`.

## Every file in a workspace

These rules hold every file of a workspace, a page or not: READMEs,
Markdown and Rust lengths, layers, and what a `docs/` folder holds.

- Every Cargo package has a page in its own `docs/` (`crate-page`),
  saying what its code is and does, but a test's fixture. One listed in
  `large-files.toml`'s `undocumented` was written before this: none
  joins, and one leaves once it has a page.
- A non-root README outside `docs/` is a map (`hub-words`): at most 120 words,
  a sentence on what the folder holds, its pages under `## Docs`, then
  entry points under `## Code`. How it works is a page in `docs/`.
  The workspace root README introduces the product, its features and
  its mission without the map word limit. Its other checks still apply.
- A list names each page by its title (`list-titles`): a list item
  that opens with a link to a page says that page's title as its text,
  ignoring backticks, and says anything more before the link, as
  "Agents:". Prose keeps its own words, as do links to a heading, code,
  a navigation outline, and a generated inventory's entries.
- A link is part of its sentence, never an aside (`asides`, 3 words):
  no parenthesis in prose holds a link and at most 3 other words, code
  aside, as "(see Boxes)" linked does. A link goes in the sentence or a
  list. A parenthesis with no link, such as an abbreviation's
  expansion, stays, and generated blocks are their generators'.
- A page describes behavior, never the Rust (`rust-names`): no code span
  in a page's prose names a Rust file, a path through a source folder, or
  a Rust item path, two names joined by two colons. A page is Markdown
  below a `docs/` folder holding no package manifest, so a package's
  README keeps its `## Code` list. Code blocks, comments, and generated
  blocks aren't read.
- Rust names stay on the pages a maintainer reads (`rust-names`), each
  declared as data. These are decisions, security, and troubleshooting
  pages, which `kinds.toml` marks `names_rust`, pages below a `more/`
  folder of a member's `docs/`, as `limits.toml`'s `[rust-names]` says,
  and the folders a workspace's `rust-pages.toml` lists.
- A README's shape (`readme-shape`), at most 5 links under `## Code`: a
  paragraph under its title on its folder (its long description), then
  `## Docs`, its pages and child hubs once each, briefly described,
  in the website's order and nesting, in Pages, References and More. [A README of code](navigation.md#indexes-of-code)
  sorts them under subheadings by kind. A hub with nothing
  under it has no `## Docs` index.
- A README's or `docs/` page's title, the name every index lists it
  by, is in Title Case like a website heading (`titles`). Code in a
  title goes in backticks.
- A package's README then has `## Code`, at least one link, to code only:
  each a file's name in backticks, linked, then a colon and what it is,
  for its entry points, three or so, alphabetically by name. Any other
  section goes after them.
- At most 500 lines in a Markdown file (`markdown-lines`): split it by
  subject before it grows past them. Generated files and validated
  generated index blocks are exempt from the authored line budget;
  their links and prose still pass the other checks.
- A library's existing doc examples run (`doc-tests`): adding the first Rust
  example to a library's docs (a fence that is plain, `rust`, `no_run`,
  `compile_fail` or `should_panic`) takes `doctest = false` out of its
  `[lib]`; a library with none keeps it. A block that isn't Rust says
  its language: `text`, `sh`, `toml`.
- A `docs/` folder holds only what people read (`docs-files`): Markdown,
  the shell a person types, what a person pastes (`.env.example`,
  `domains.txt`), and the data or pictures a page shows (`.yaml`,
  `.json`, `.svg`, `.png`). A program, its inputs and its
  output go beside the code that runs them. A `docs` folder that is a
  package of its own is code, but its own `docs/` is docs again.
- A package's docs lines are within its Rust's prediction
  (`docs-ratio`, 3.5 times either way), its `src/` lines to the 0.7:
  docs grow with code, more slowly. Test-only packages use maintained
  test code. `limits.toml` holds the curve; `lnk-docs-survey` reports
  production, test, generated, and documentation counts separately.
- At most 1,000 lines in a Rust file (`rust-lines`): split it where a
  responsibility ends before it grows past them.
- A file already over its limit is in its workspace's
  `large-files.toml` (`large-files`): it may shrink, never grow, and no
  file joins it.
- Only `link-format` prints (`output`): code `lnk` runs writes blocks
  to its `Out`, never `println!`, `eprintln!`, `print!`, an ANSI escape
  or a backtick in a string. A file in `large-files.toml`'s `[output]`,
  with its print sites, may shrink, never grow, and none joins it.
- Each group folder is a layer (`layers`): its `layer.toml` lists every
  group folder it is built on, with no cycle, and a crate depends on,
  runs (`lnk <group>`, or through `ensure`) and answers the questions
  of only its own group's crates or a listed one's.
- Every Cargo package, every folder holding two or more, and every
  folder grouping two or more files or folders outside a package, a
  `docs/` tree and a hidden folder has a README hub (`readmes`), but
  under `fixtures/`, `assets/` or a package's `src/` and `tests/`, and
  in the folders a program reads or writes whole, which
  `large-files.toml`'s `data` names.
- A folder hub links one level down (`through-readmes`), to a folder's
  `README.md`. The workspace README introduces the products and may
  link directly to their documentation. Source links still follow
  folder hubs.
- README indexes are generated from documents and nearest child hubs
  (`navigation`): run `cargo run -p link-docs-check --bin lnk-docs-tree -- --root <workspace> --write`,
  then verify with `--check`. Preserve prose outside marked indexes.
  Site pages follow their nearest address ancestor and retain sibling order.
- Every selected document has a reading path from its root README
  (`doc-reachability`), including English public pages and private
  planning documents; the writer and checks use the same discovery.
- A page is at most 5 links from the workspace's root README (`clicks`):
  link it where a reader meets what it explains. `lnk-docs-clicks`
  reports each workspace's clicks, naming the deepest pages.
- The pages are on average at most 265 hundredths (`average-clicks`) of
  a link from the root README. Lower both limits as they're reached,
  never raise them. Both measure the authored pages: a generated page
  is looked up from its index, never read on the way to another.
- A package's `Cargo.toml` description is its README's first sentence,
  but for the package's name opening it (`descriptions`): every index
  lists the package by it, and [`lnk mod list`](../../../../../../docs/more/modules/guide.md#get-help) shows it.
- A non-package grouping with one child hub and no documentation of its own
  is reported for review (`single-child-groups`): advisory only; architectural
  layers and docs bridges are exempt, and no folder is flattened automatically.
