# Scripts

Scripts holds what a release reads from the workspace, the shell completion
for `lnk`, and the index of the website's pages.

<!-- docs-tree:start -->

## Docs

- [Branches and Releases](docs/more/developing/releases.md): Public contributions and reviewed source snapshots land on `main` by pull request
- [Developer Boxes](docs/more/developing/setup/dev-boxes.md): Build your checkout and try it on a cloud box
- [Completion](completion/README.md): Completion holds `lnk`'s shell completion for bash and zsh, generated from every module's declarations
- [Release Inputs](release/README.md): A release reads its inputs from the workspace

<!-- docs-tree:end -->

## Code

- [`completion/`](completion): [bash and zsh completion](../core/cli/docs/more/reference/completion.md) for `lnk`, generated from the declarations
- [`documentation-index.json`](documentation-index.json): the documentation website, the documentation owners; page placement lives in navigation.md
- [`release/`](release): the release and staging signers, the advisories a release seals and `set-version.sh`
