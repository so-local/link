# Documentation Navigation

The documentation writer maintains README indexes from the pages beneath them,
so readers can follow links from the repository's root without opening a file browser.

## Generate the reading tree

Run the writer from the Cargo workspace, selecting the directory whose README
starts the reading tree. It updates indexes in existing READMEs and creates no
new pages or website routes. Run `cargo run -p link-docs-check --bin lnk-docs-tree -- --root . --write`.

### Choose a root

`--root` selects a directory and defaults to the current directory. Use a
workspace root to update its documentation, or a checkout root to update several
workspaces together. Each index links only to its own descendants, so one
workspace's index never acquires links into a sibling workspace.

### Read the root's own data folders

The writer reads the selected root's own `tests/docs/large-files.toml`, the
one nearest the root, so the folders that workspace declares as data stay out
of its indexes, as its documentation tests leave them out. Two lists equally
near the root are refused, and a root with none uses this package's list.

### Check without writing

`--check` is the default mode. It fails for a stale index or a document with no
reading path from the selected root's `README.md`. The regular documentation
tests run these same checks for their workspace. Run `cargo run -p link-docs-check --bin lnk-docs-tree -- --root . --check`.

## Discover documents

The reading tree includes Markdown pages and README hubs under the selected
root, including English documentation, references and planning notes. Each page
belongs to its nearest existing README ancestor; a README belongs to the next
README above it. Bridge folders without a README are traversed directly.

### Excluded inputs

Source folders, a package's test and example folders, fixture data, assets, hidden
infrastructure, internal agent instructions and website configuration are not
reading-tree pages. Within a Cargo package, only README hubs and its own
`docs/` tree are documentation; Markdown elsewhere, such as command templates
or prompts, is implementation data. Translations (`<page>.<language>.md` beside their English) are excluded;
English pages provide the source tree. Build output and dependency folders are
already excluded by the documentation checks' file reader.

### Executable fixture examples

README hubs under `tests/fixtures/` are reading-tree pages, so developers can
find the executable examples through the tests' index. Other fixture Markdown
remains input data and does not become documentation merely by its extension.

## Render indexes

Each entry takes the child's title and opening sentence; a package hub uses its
Cargo description. Every index follows the shared outline, its order
and its nesting, with entries the outline doesn't hold after,
alphabetically.
Descriptions are shortened for scanning, and inline code keeps its formatting.
Changing a child's title, description, placement or location makes the
corresponding generated navigation stale.

### Indexes of code

A README of code (a layer's, a group's, a module's or a package's) lists
two kinds of children. Its folder's own documentation is the pages under
its `docs/`, and its other children are the folders beside them, each
with a README of its own. The index puts each kind under its own
subheading. The folders' kinds are the
[architecture guide's](../../../../../../docs/more/architecture/guide.md#kinds).
A README whose children are all one kind keeps one list without
subheadings.

### Heading order

The headings read Pages, References, Core, Modules, Packages, Folders,
and More. What the folder documents comes first, then
what's in it, from what a reader runs to the folders holding more, and
its catch-alls last.

### Pages, References and More

Every index, of code or not, lists its pages under the heading their
kind names in `tests/docs/kinds.toml`. Pages holds what's written for the
reader, References what's generated, and More developer pages, decisions,
security and troubleshooting, each page keeping the pages under it.

### Pages of code

Under Pages, the folder's documentation hub, the README in its
`docs/<topic>/`, is followed by its own pages one level deeper, so the
index shows everything the folder documents. For example, a group folder
holding `contract/`, `docs/` and `webhook/` lists its documentation hub
and that hub's pages under Pages, the webhook module under Modules and
the contract under Packages. The index never lists a child twice, so
lists of the same pages outside the markers are redundant.

### Order and nesting

The website's pages are listed in its outline's order, each nested under
the nearest listed page above it there, so an index nests only where the
website does. Anything else follows, alphabetically.

## Preserve authored content

The writer owns the section between `<!-- docs-tree:start -->` and
`<!-- docs-tree:end -->`, adding a `## Docs` heading unless one is authored
above the block. Keep any introduction outside the markers, between that
heading and the generated links. On later runs, only the marked section
changes. The opening, quickstart, other sections and code entry points
remain authored content. Malformed or repeated markers stop the writer
before it writes any index.

### Migrate an existing index

On the first run, an existing `## Docs` section becomes a marked index. Its
ordinary links to discovered children are replaced by generated entries;
editorial prose and links elsewhere remain. Without that section, the writer
inserts one before `## Code`, or at the end. Review the migration diff before
committing, particularly pages that use tables or custom index formatting.

## Keep the checks meaningful

Generated navigation has its own freshness check. Its lines and heading do not
consume the authored line and heading budgets, while its links and prose remain
checked. A generated block cannot hide arbitrary text: any content differing
from the writer's expected output fails the freshness check.

## Review grouping folders

The documentation checks warn about a code grouping folder that adds a
README level around one child and has no documentation of its own.
This is a review prompt rather than a failure: architectural layers,
package roots and documentation bridge folders can have a purpose even
when they contain one item. The writer never flattens folders.

## One shared outline

Each website's `docs/navigation.md` contains its named outline under
`## Pages`. The website and feature README indexes consume this same order;
page titles and descriptions remain in their owning documents. Locale
URL mappings are structural metadata, while interface labels live in
shared localization catalogs.

### Split a Large Outline

Keep a large top-level section in `docs/navigation/<section>.md` and include it from the site's root outline. The section file has its own title and `## Pages` list, with one top-level entry matching the include's label. Its page links are relative to that file. The website and README writer expand the same structure without creating a page for the section file.

```text
- [Agents](../navigation/agents.md "include")
```

### Check Section Files

The writer preserves section includes, refreshes their labels and inserts discovered pages in the owning section file. Section files retain the same Markdown size limit. Missing, duplicate, unused, nested or escaping includes fail validation. Translations keep their own section files, with structure and targets checked like the root outline. Every real page still needs one position in the expanded reading tree.

### Curate placement

Move an entry in the outline to change its reading position among pages
of its kind. Regeneration preserves existing nesting, and orders each
page's children by kind, `tests/docs/kinds.toml`'s order: the guide, the
section's own pages, developer pages, generated references, then
decisions, security and troubleshooting. A README's child hub takes the
position of its first ranked descendant; repository-only entries without
a position in the outline remain alphabetical.

### Add documentation

The writer discovers new genuine pages and inserts them beneath their
nearest existing parent in the outline. New root entries are appended
in deterministic order. Run it after generating documentation so every
new page has a persisted reading position, then review the placement.
Missing or duplicate entries and stale indexes fail the checks.

### Short tab labels

A top-level navigation link titled "tab" keeps its authored label while
its page keeps a longer title. A Presale tab can therefore point to a
page titled Local Link Presale. The writer still resolves the target
and checks its reading path.
