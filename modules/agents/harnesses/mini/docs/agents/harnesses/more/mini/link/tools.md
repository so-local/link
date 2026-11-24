# Tools

A tool is an operation the program can execute for the model. Mini connects the tool's definition, the model's request and its recorded result without adding a planner or choosing tools for the model.

## Discover Available Tools

[`Tools::open`](../../../../../../src/link/tools.rs) opens the Model Context Protocol (MCP) servers supplied by Link and reads their tool definitions. Each model-facing name includes its server name, such as `files__read`. Mini skips unreachable servers and sorts the discovered names so differences in discovery order do not change the prompt.


## Offer a Definition

A definition describes the tool's name, purpose and argument schema. The model sees that description and can produce a structured call. The definition alone does not execute anything or grant permission to an otherwise unavailable tool.

## Execute a Call

[`Tools::call_tool`](../../../../../../src/link/tools.rs) looks up the offered name, then sends the arguments to the server using its original tool name. The MCP operation carries the current thread metadata with the call. Link enforces the agent's permissions, and the core records the returned answer before asking the model again.


## Match the Result

After reading the MCP result, the operation returns the model's call ID with the tool's text and failure flag. This is the value that `run_turn` places in the conversation's `answers` field. Mini's tool bridge returns text results and leaves media parts empty.


## Report a Failure

An unavailable tool, a transport error or an MCP error becomes a failed tool answer. The model sees that failure and can explain it or make a different request. A model or storage failure ends the turn instead. Mini never retries a tool call automatically.

## Close the Turn

Tool sessions close after the loop finishes or fails. The conversation keeps the calls and results, while endpoint addresses and session credentials stay outside the model-facing transcript.

<!-- docs-tree:start -->

## Docs

<!-- docs-tree:end -->
