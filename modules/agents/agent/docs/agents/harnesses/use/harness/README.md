# Manage a Harness

`lnk agent harness` inspects a harness's installation, permissions, and sandbox for the selected agent. Use `--harness hermes` to name another harness. [Switch Harnesses](../README.md) covers switching which harness runs the agent.

## Inspect the Installation

`lnk agent harness list` lists available harnesses and the current selection. `lnk agent harness show` identifies the selected agent's harness home and traces outside it. [Remove a Harness](../README.md#remove-a-harness) explains how to review those files before removal.

## Inspect and Change Grants

`lnk agent harness-permission list` shows shared harness requirements and your decisions. `lnk agent harness-permission add` grants a declared requirement or named risk, and `lnk agent harness-permission remove` removes one. Both restart every agent here running that harness. `lnk agent harness reset` forgets previous answers so the next start asks again. [Permissions](../harness-permissions/README.md#allow-what-a-harness-needs) owns the grant rules and flags.

## Inspect the Sandbox

`lnk agent harness check` tests access from inside the harness's sandbox and reports what gets through. `lnk agent harness run` runs a command with the harness's home, grants, and proxy. [Run a Command in the Sandbox](../../../../security/sandbox/permissions/README.md#run-a-command-in-its-sandbox) explains that execution context.

## Remove the Harness

`lnk agent harness delete` removes the harness's home, including its native conversations and memory, and the traces `show` identifies. Shared grants stay while another agent here uses them. Review [removal behavior](../README.md#remove-a-harness) before you delete the selected harness.
