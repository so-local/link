# Decisions

How the Signal module connects an agent, through `signal-cli`.

## How is Signal connected?

Linked as a device through `signal-cli`, which compatible harness
adapters or Link's Signal bridge run.
Link installs it as a pinned, checked release into the harness's home:
the native build on Linux x86-64, and the Java build with a pinned Java
runtime on the other supported platforms.

### Installed by Link

Link installing it is one command instead of asking you to install it
and Java, and nothing outside the home for the sandbox to open. `signal-cli` is
GPL-3.0, so Link downloads it from its releases and runs it as its own
program, never built into Link. It reads Java's proxy settings, not
`HTTPS_PROXY`, so Link's wrapper passes the proxy on.

### Where it runs

The [native setup boundary](more/native-program.md#where-it-runs) explains how the adapter installs and links `signal-cli` inside the harness sandbox. Its writable program and account data stay subject to that sandbox.

### Over its stdin

Link's own Signal bridge speaks to `signal-cli` over JSON-RPC on its
stdin and stdout, not its HTTP daemon, which would listen on the
loopback, where any user of the machine could send as the agent.
