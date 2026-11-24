# Archive Questions

The five questions each kind's module answers so `lnk archive` can export and import its things. The archive module asks them, and the module `layout.toml`'s `[owners]` names for the kind answers them.

## Who Answers

A module owning a kind lists the five in its `module.toml`, as
`answers = ["archive.check", "archive.running", "archive.carry",
"archive.missing", "archive.adopt"]`, and [answers each through its hidden](../../../../../../../../core/framework/docs/more/modules/protocol.md#questions) `lnk-<module>
--link-answer <question>` flag, one JSON line in and one out, each stating
its protocol version.
A kind whose module isn't installed, or doesn't answer all five, isn't
exported or imported. Every answer reads and writes files alone: no
network call, nothing started or stopped.

## The Thing Asked About

Every question names a thing (`Thing`): its kind, its name and its
folders, absolute, each by the key of the location it's a folder of:

<!-- json: link_archive_contract::Thing -->
```json
{"kind": "agent", "name": "main", "folders": {"link.agents": "/home/me/Link/Agents/main", "link.agent-settings": "/home/me/Link/Safe/Agents/main"}}
```

At export, they're the thing's folders here. At import, `archive.check`
is asked of the folders staged in `~/.lnk/data/archives`, and the
others of the folders in place, under the name chosen.

## `archive.check`

Reads the thing's files with their owning types, refusing what they
refuse, or what its files alone don't carry whole. It answers `{}`, or
fails, saying why on stderr, which refuses the export or the whole
import.

## `archive.running`

Whether the thing is running here, so export refuses it:
`{"stop": "lnk agent stop --agent main"}` while it runs, the command
that stops it, else `{}`.

## `archive.carry`

Which of the thing's files export writes in place of its own, or leaves
out, so the archive holds nothing of this machine's. It's asked
`{"thing": ..., "staging": "<an empty folder>"}`, writes each
replacement in that folder, and answers `{"replaced": [{"file":
".../events.jsonl", "with": "<staging>/events.jsonl"}]}`, or `{}`. A
file without `with` is left out. A path outside the thing's folders or
that folder refuses the export, and the thing's own files never change.
A module that keeps chats carries them this way, so their IDs from this machine stay out.

## `archive.missing`

What an imported thing still needs to run here, read from its files,
each with the one command that sets it up, which import names and never
runs: `{"needs": [{"what": "Telegram", "setup": "lnk agent connect
telegram"}]}`.

## `archive.adopt`

Writes the owner's records in `~/.lnk` for a thing that arrived in its
folders, as a new thing, under the name chosen. It's asked `{"thing":
..., "was": "main"}`, `was` the name in the archive, and answers what
to tell you and the command that starts it: `{"notes": [], "next": "lnk
agent start --agent work"}`. When it fails, import removes the
folders it placed.

<!-- docs-tree:start -->

## Docs

- [Who Answers](#who-answers)
- [The Thing Asked About](#the-thing-asked-about)
- [`archive.check`](#archivecheck)
- [`archive.running`](#archiverunning)
- [`archive.carry`](#archivecarry)
- [`archive.missing`](#archivemissing)
- [`archive.adopt`](#archiveadopt)

<!-- docs-tree:end -->
