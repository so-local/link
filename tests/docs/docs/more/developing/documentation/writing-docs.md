# Writing Docs

How a page, README or guide is written and checked: the rules the docs
checks hold every page to, and the limits they read.

## Read the Rules

Read [Docs Rules](docs-rules.md) before writing or editing a page,
README or guide: each rule by its id, as the docs checks report it. A
section's hook follows [Writing Hooks](hooks.md).

## Limits

The docs checks read these from `tests/docs/limits.toml`, and fail when
this table differs from it. A file past its line limit is listed in
`tests/docs/large-files.toml`, and so is a page written before the
shape rules: each may shrink, never grow. An exception holds one page,
named by its address, to a higher number of its own, and its row follows
the limit it raises.

| Limit | At most | Why |
|---|---|---|
| Words besides its links in a parenthesis holding a link | 3 | Work the link into the sentence, never beside it. |
| Average hundredths of a link from the root README | 265 | Link a page where a reader meets what it explains. |
| Characters in a table cell | 60 | A cell is a few words; more is a section. |
| Links a reader follows from the root README to a page | 5 | Link a page where a reader meets what it explains. |
| Levels from the home page to a page's deepest heading | 7 | The address and the headings are one tree. |
| Times a package's docs are off its Rust lines to the 0.7 | 3.5 | Docs grow with the code they explain, more slowly. |
| Characters before two pages may not share a paragraph | 120 | Say it once, where it belongs, and link there. |
| Pages a page with pages under it holds at least | 3 | Fewer is a level opened for one or two pages. |
| Headings under a heading, and at a page's top | 10 | Group a longer list a level deeper. |
| Characters in a hook | 45 | It's a heading, and half the tab's title. |
| Words a hook has at least | 3 | A hook says more than its page's name. |
| Words in a non-root README outside docs/ | 120 | A README is a map; how it works is a page. |
| Characters in a list item | 400 | An item is a line or two; more is a section. |
| Lines in a Markdown file | 500 | Split a longer page by subject. |
| Pages under a page in the site's tree | 10 | Group a longer list in a folder. |
| Pages under a page in the site's tree, at `/agents` | 12 | Agents, the product's center, holds more first-class pages. |
| Pages under a page in the site's tree, at `/agents/tools` | 12 | Each tool is a first-class page of the product's center. |
| Characters in a paragraph | 600 | More is two subjects: two sections. |
| Links under a README's ## Code | 5 | Name the entry points, three or so. |
| Lines in a Rust file | 1,000 | Split where a responsibility ends. |
| Characters a section's lone paragraph has at least | 80 | Less is too thin to stand alone: fold it in. |

## Terminal Source Discovery

The shared example reader keeps inline references separate from complete
shell examples. Its terminal inventory also retains headings, fence numbers
and source lines across split steps. Compact alternatives in text fences
expand into branches, while shell pipes remain operators. Discovery reads
source without executing commands, and reports unfinished syntax for review.

### Unresolved Shell Syntax

Quoted shell values may span lines. Compound control flow, command substitutions,
and shell command strings that the reader cannot model stay explicit review
work rather than disappearing from discovery.

### Displayed Responses

Mark a displayed response with
`text output` in its opening fence. Its contents remain documented output,
with a reported exclusion from input-command discovery, even when a Next
suggestion mentions a command.

### Wrapped Commands and Lookups

Use an explicit `--` before the inner command in a `strace` example, such as
`strace -f -- lnk agent list`; discovery retains that inner invocation.
Bounded shell lookups (`type -a lnk`, `which -a lnk`, `command -v lnk`)
are reported as introspection exclusions with their original arguments.
Unsupported wrapper or lookup syntax remains a discovery problem.

### Cargo Artifact Selection

Cargo's `--bin lnk` selects an artifact in bounded `build`, `check`, `test`,
and `clippy` commands; it is reported with its arguments as an artifact-selection
exclusion. A following shell command is still discovered. Unknown Cargo syntax,
extra operands, or an ambiguous argument suffix remain review problems.

## A Page for Each Verb

The suggested shape of a module's docs: each verb its commands start
with, such as [`lnk agent move`](../../../../../../modules/agents/agent/docs/agents/move/README.md#move-your-agent-to-the-cloud) or [`lnk box create`](../../../../../../modules/compute/box/docs/services/boxes/guide.md#create-a-box), has a page or folder of its own
in its module's `docs/`, named for the verb, saying what it does and how
it works with the module. The module's other pages say how its verbs
connect and what the module is, as the agent module's `move/`, `chat/`
and `start/` folders do.

### A Suggestion, Not a Rule

The `verb-pages` check warns for each module, naming the verbs with no
page of their own, and fails nothing. Follow it when a page reads
clearer for it. A good page shared by two commands stays, and a docs
tree is never reorganized for it alone.

## Click Depth

How many links a reader follows from a workspace's root README to each
page, the fewest there are. The report prints each workspace's average,
median and deepest clicks, naming the deepest pages, and the `clicks`
and `average-clicks` limits hold the workspace there. Run `cargo run -p link-docs-check --bin lnk-docs-clicks -- --root .`.

### Lowering It

Link a page where a reader meets what it explains: a tool named in a
task, a package named in a page's opening. A link added only to lower
the number is noise. Lower the limits to what the workspace reaches,
never raise them.

## Documentation Prominence

Rank the documented features in one workspace using authored links and
command mentions. The report reads that workspace's READMEs, English pages
and action declarations, without reading case catalogues or execution history. Run `cargo run -p link-docs-check --bin lnk-docs-priority -- --root .`, then `cargo run -p link-docs-check --bin lnk-docs-priority -- --root . --json`.

### Scoring

The [weights](../../../../priority.toml) give 20 points for a feature shown
on the root README, 3 per distinct outside page linking into it, 2 per page
mentioning its commands and 1 per outgoing documentation target. Repeated
links or mentions on one page count once. Internal links within a feature,
generated inventories and generated navigation add no points.

### Reviewing the Order

JSON keeps every contributing page and each feature's README examples,
including their source locations and alternative branches. Navigation-only
references and README link counts are separate signals. Use `--weights` with
another file to compare opinions without changing source. Prominence is a
reason to review a feature first, not measured usage, failure impact,
execution time or a claim that a test has passed.

### Argument Value

The default argument weights give positionals 5 points, ordinary flags 1,
and required inputs 2 extra. Reviewed rules promote execution choices and
permissions to 8 points, while resource selectors receive 5. The rules name
their reason and can be scoped to a command. Semantic choices can earn
separate value points, while incidental resource names share one input.

### Marginal Value

Each source form has a floor of 1 point. A newly selected command adds 10,
then its strongest new argument or unflagged default path adds its points.
Default paths receive 8. Extra flags do not accumulate points without bound,
and repeated command or argument coverage earns less. This is modeled value
for selecting work, without claiming that any behavior was verified.

### Positional Rotation

Each feature rotates through command paths and semantic positional groups
before returning to flag variants of a group. Distinct connection and cloud
kinds get separate turns. Free-form names of the same resource share a group.
The configurable rotation applies within features, while the importance
curve chooses among the eligible groups across features.

### Selection Curve

The default next priority is the marginal value multiplied by the logarithms
of one plus the feature points and one plus its command points, divided by
the square root of one plus the feature's earlier picks. Command points use
homepage presence and distinct mentioning pages. Thus agent depth gradually
gives way to initial checks elsewhere. The queue suggests authoring order,
with prerequisites, controls, complete workflows and timings still to resolve.
