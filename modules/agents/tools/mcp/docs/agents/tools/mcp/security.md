# Security

Command tools retain the calling client's approval boundary and bind agent reads to the selected agent. The server executes only commands explicitly exposed by installed component declarations.

## Command Authority

A Link agent reaches this server through its sandbox tool proxy. Startup allows only the names declared read-only and leaves every other call subject to approval. The server does not treat MCP annotations as authorization. Other MCP clients must enforce their own approval policy before forwarding calls.

## Agent Identity

An agent's tool definition binds its stable agent ID outside model inputs. Requests cannot replace that selector, choose a request file or override output formatting. The server runs outside the harness sandbox and uses Link's existing command authority, so its definition belongs to the owner and is not writable by the agent.

## Exposed Commands

Tool exposure is explicit in each command's owning declaration. Hidden commands, login, credential management, permission grants and tool installation are not exposed. A guessed tool name cannot select another command. Arguments are type-checked, enum-checked and passed as individual process arguments, with positional values following the option terminator.

## Resource Limits

Requests are limited before JSON parsing. Each command declares an execution deadline and answer budget, and diagnostic output is bounded separately. Calls have no terminal input. Cancellation and deadlines stop the owned process group, including descendants that remain in it. Accepted calls drain when standard input closes. These limits bound one call and do not impose a billing budget on the external services a command uses.
