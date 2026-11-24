# Troubleshooting

What to do when your agent can't write or read one of its [files](README.md). Start by inspecting the current state, then use the symptom below to choose the next check.

```sh
lnk agent show
```

## "you may write only in ..."

Its sandbox file's `[files]` doesn't let it write that folder. Add the
folder to `[files] write`.

## "you may reach only ... of team ...'s folder"

Its sandbox file doesn't grant it that folder of the team's. Run `lnk
agent team add <team> --member <agent>` with `--write` or `--read` that folder.

## "is a link, which file doesn't follow"

File never follows a link, so a link in the files folder is refused,
never read. Put the file itself in the files folder.

## "changed since it was read"

You, or another conversation of your agent, changed the file after this
conversation read it. The agent reads it again, then writes, so neither
change is lost unseen. This holds on a machine and through the files
bridge alike.

## "this conversation reaches only its own folder in chats/"

Each conversation reads only its own folder of `chats/`. To use what
was said in another conversation, ask there, or search it with
[memory](../../../../../memory-tool/docs/agents/tools/memory/guide.md),
which indexes your conversations for the owner.

## "a stranger's conversation reaches no conversation in chats/"

A stranger's conversation, such as a webhook's, reads no conversation.
So does a call whose harness doesn't name its conversation and standing.
Use Link Harness, which names both on every call.

## "Home is full"

Your agent's Home, kept off this machine and reached through the files
bridge, is at its cap, which Home and memory's index share: remove
files from Home to make room.

## "isn't text (UTF-8)"

File reads text, images and PDFs, and writes text only; `run` works on
other files.

## Your agent has no `file`

Its sandbox is off (`--no-sandbox`), the File tool is disabled, or its
harness does not declare the compatible folder layout. Enable the tool
with [`lnk agent tool add file`](../../../../../../agent/docs/agents/README.md#add-tools-and-skills); use [`lnk agent harness set link`](../../../../../../agent/docs/agents/harnesses/use/README.md#switch-in-one-command) when
the selected harness lacks that layout.
