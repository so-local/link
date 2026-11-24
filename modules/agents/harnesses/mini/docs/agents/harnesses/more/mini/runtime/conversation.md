# Conversation Storage

A Mini conversation is one JSONL file, with one complete model-interface message on each line. Keeping the shared message shape avoids translating saved history into a second, less complete representation.

## Locate a Conversation

[`conversation_path`](../../../../../../src/runtime/conversation.rs) maps a checked session name to its file. Link stores these files under the harness home's `threads/` directory. Names cannot contain path separators, control characters or percent signs that could select or alias another file. A colon in an accepted name is encoded in its filename.

## List Saved Sessions

`list_sessions` reads the conversation filenames and returns their names in sorted order. A saved session is separate from the current connection to the program, so it can be opened again after a restart.

## Load Previous Results

[`load_conversation`](../../../../../../src/runtime/conversation.rs) reads each line into the shared `Message` type. A missing file starts an empty conversation. Malformed JSON or a field with an invalid type stops loading with its line number. Deserialization keeps the complete message rather than projecting it into just user and assistant text.


## What Is Saved

Messages retain user text, assistant text, tool-call IDs, names and arguments, tool answers and failure flags, and provider thinking or continuation blocks. An answer points to its call by ID. Tool answers remain in the conversation even when the final assistant reply does not quote them. Provider data is retained as part of the shared type rather than interpreted by the runtime.

## Append a Message

[`append_message`](../../../../../../src/runtime/conversation.rs) creates private storage and opens the file for appending. The writer serializes the whole message with `link_files::saved::line`, which adds its schema, `"schema"` last, and the JSONL newline; the reader advances each line by its own schema with `parse_line`, and refuses a line a newer release wrote, naming [`lnk upgrade`](../../../../../../../../../../docs/more/modules/guide.md#upgrade). The schema is declared in `conversation/line.schema.toml`, beside the code. Earlier records remain unchanged, so a restarted runtime sees the same history and later model requests retain the same earlier prompt prefix.


## Handle Interrupted Work

`check_complete` tracks pending call IDs as it walks the history, rejects unmatched answers and checks that all calls are answered. If execution stopped after a call was recorded, the tool may already have changed something. The final check requires a new conversation rather than replaying that work. Mini has no automatic retry or recovery policy.


## Move and Export

Moving the agent carries its native files, including the full tool history. Portable session export projects user and assistant text into the common cross-harness format. It does not promise that a different harness can resume provider-specific tool or thinking state.

<!-- docs-tree:start -->

## Docs

<!-- docs-tree:end -->
