# Instructions

Mini reads one optional `instructions.md` file in the agent's Home. It provides a small way to guide a model without a persona system, skill loader or instruction hierarchy.

## Write an Instruction

For agent `main`, the default file is `~/Link/Agents/main/home/instructions.md`. Put ordinary text in it, such as a request to answer briefly or cite filenames. The adapter supplies that explicit path to the runtime.

## Missing Means Empty

[`read_instructions`](../../../../../../src/runtime/instructions.rs) returns the file's text unchanged. If it is missing, it returns an empty string. Mini adds no identity or instruction of its own. Other errors, including unreadable files or invalid text encoding, stop the turn.


## Read Once per Turn

The runtime reads the file before asking the model and uses that same text for every model call in the turn. An edit applies to the next turn. This avoids changing the prompt halfway through a tool interaction.

## Preserve the Boundary

An instruction is model input. It cannot grant tools, change permissions or supply credentials. Mini does not search parent directories or load other instruction files. Link's shared model client omits a system instruction when this string is empty.

<!-- docs-tree:start -->

## Docs

<!-- docs-tree:end -->
