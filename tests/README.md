# Tests

Tests holds the checks that read across Local Link's packages: the repository checks, the docs checks, the check runner and its timing report, each specification's cases, and the fixtures system tests run.

<!-- docs-tree:start -->

## Docs

### Pages

- [Docs Checks](docs/README.md): The docs checks read every page once and check links, headings, limits, and READMEs together
  - [Writing Docs](docs/docs/more/developing/documentation/writing-docs.md): How a page, README or guide is written and checked: the rules the docs checks hold every page to, and the limits they read
  - [Docs Rules](docs/docs/more/developing/documentation/docs-rules.md): Every rule the docs checks hold a page, README or source file to, by its id, so a writer sees each rule and its limit before a check fails
  - [Writing Hooks](docs/docs/more/developing/documentation/hooks.md): A hook is the one line under a section's plain title
  - [Documentation Navigation](docs/docs/more/developing/documentation/navigation.md): The documentation writer maintains README indexes from the pages beneath them, so readers can follow links from the repository's root without opening a file browser
  - [Docs Code Checks](docs/code/README.md): `link-docs-check-code` checks documentation examples and inventories against the command parsers, types, and declarations Local Link releases

### Packages

- [Checks and Timings](timing/README.md): Checks and Timings runs a workspace's checks as one command, `lnk-check`, and turns its test run into a timing report that sorts each failure by whose it is
- [Repository Checks](repository/README.md): Repository Checks read the whole repository rather than one package's code

### Folders

- [Specification Cases](specs/README.md): Specification Cases holds each specification's cases, one TOML file per case, giving a specification as a person writes it (`[spec]`) and what must come of it (`[expect]`)
- [Test Fixtures](fixtures/README.md): Test Fixtures holds executable examples and shared setups for Local Link's tests

<!-- docs-tree:end -->
