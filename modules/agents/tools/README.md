# Tools

Tools let your agent do more than talk. Each tool is a module that runs outside the agent's harness and offers its abilities to the agent's model.

<!-- docs-tree:start -->

## Docs

### Pages

- [Tools](docs/agents/tools/README.md): Tools let your agent work with files, programs, web pages, and screens, keep searchable memory, and delegate or schedule work

### Modules

- [Link Browser](browser-tool/README.md): Link Browser (`lnk-browser-tool`) lets an agent open pages in a Chromium of its own, in a sandbox, then read, click, type into, and screenshot them
- [Link Code](code-tool/README.md): Link Code (`lnk-code-tool`) lets an agent ask a project's language servers for diagnostics, definitions, and references, each server in a sandbox
- [Link Desktop](desktop-tool/README.md): Link Desktop (`lnk-desktop-tool`) gives an agent a virtual desktop of its own, which the agent sees as screenshots and works with the mouse and keys
- [Link File](file-tool/README.md): Link File (`lnk-file-tool`) lets an agent `read`, `write`, `edit`, `glob`, and `grep` its files, each call in a sandbox of its own
- [Link MCP](mcp/README.md): Link MCP offers Local Link commands as tools to a client that speaks the Model Context Protocol (MCP)
- [Link Memory](memory-tool/README.md): Link Memory (`lnk-memory-tool`) runs locally and lets an agent search, read, and list its folder with no credential
- [Link Run](run-tool/README.md): Link Run (`lnk-run-tool`) lets an agent run programs in its files folder, each call in a sandbox of its own, and keeps background commands running past the turn
- [Link Scheduler](scheduler-tool/README.md): Link Scheduler (`lnk-scheduler-tool`) sends an agent messages from someone other than you, now or later
- [Link Web](web-tool/README.md): Link Web (`lnk-web-tool`) lets an agent fetch a page as Markdown and search the web through a search API with a secret of yours

### More

- [Tool Details](docs/agents/tools/more/README.md): Compare harness requirements and managed action namespaces in the provider guide

<!-- docs-tree:end -->
