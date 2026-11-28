# Archives

> Your Things in One File

Archives carry agent state between machines and people in one file. They
only read and write files. On macOS or Linux, create an agent on Link
Harness and stop it so its files are whole. Then export its state and
import the archive. Creation asks for any configuration it needs.

```sh
lnk agent create main --harness link
lnk agent stop --agent main
lnk archive export --select agent.main
lnk archive import ~/Link/Archive/main.tar.gz
```

## Export

`lnk archive export` writes the things you select to one archive. The
answer lists each thing with its size, so you see what you're about to
share. Export refuses a running thing and names the command that stops it,
so the files are whole.

```sh
lnk archive export --select agent.main,agent.work
lnk archive export --all
```

### Export Refuses Agents on Other Harnesses

Export refuses an agent on another harness, because that harness owns state
outside the archive's selected locations.
[Switch the agent to Link Harness](../../../../../agents/agent/docs/agents/harnesses/use/README.md)
through its owner before exporting, and stop it so the files can be copied.

### Choose What to Export

`--select` takes a comma-separated list of selections, and `--all`
selects every thing here of every kind that exports. Export takes
exactly one of them. A name that isn't here, or whose kind doesn't
export, refuses the export and is named.

### Choose Where the Archive Goes

Export writes one thing to `~/Link/Archive/<name>.tar.gz`, as
`main.tar.gz`, and it writes several things to `~/Link/Archive/<date>.tar.gz`. `--to`
names another file. An existing file is refused rather than overwritten.

### Export an Archive Kept as a Folder

An archive can also be kept unpacked, as a folder with `archive.toml` at
its top and each thing's folders laid out as its `[map]` says. A community
keeps an agent this way in a Git repository, and Local Link keeps its own
agents this way. `--from` writes such a folder as one `.tar.gz`, with the
same writer, in place of `--select` or `--all`.

```sh
lnk archive export --from coach --to link-coach.tar.gz
```

The manifest and selected state folders are checked before packing, and
files outside those folders or the declared root files are refused. Use
a prepared folder you trust because its root metadata is read by path. No module is asked
anything, since nothing here is exported. Without `--to`, it's written
to `~/Link/Archive/<folder's name>.tar.gz`.

### Review Private Values Before Sharing

Export selects state locations and leaves Local Link's credentials,
approvals, and installed programs behind. It copies personal files and chats
whole, including private values you or the agent wrote there. Review the
content before you share it, and export an agent prepared for its recipient.

## Import

`lnk archive import <path>` takes a `.tar.gz` file on disk. It reads and
checks the whole archive, offers each thing's name for you to keep or
change, and moves the thing's state into place here. `--select` imports only
the things you list. Import installs, connects and starts nothing. It ends
with the setup command for each missing requirement, then the command that
starts the thing.

```text output
This agent is named main. Rename it? [y/N] y
Name: work
✓ Imported work.
  Next
    lnk agent start --agent work   start it
```

### Check Everything Before Anything Moves

Import structurally checks the whole archive, then validates each selected
thing's files with its owner's types before asking or placing anything.
If a selected thing fails its check, or its module cannot import it, the
import is refused. With `--select`, unselected things are not adopted.

### Rename a Thing Whose Name Is Taken

Import never overwrites or merges into a name already used here. It says the
name is taken and asks for another, so one question settles the clash.
Without a terminal, import asks nothing. A taken name refuses the import and
is named. `--select agent.main=agent.work` imports a thing under the name
after `=`.

### Get a New Identity on Each Import

Export leaves out Local Link's machine-managed identity records. Each
imported thing gets a new identity, so two people importing one community
agent have different agents and do not inherit grants to the exporter's
identity. References written in personal files remain file content. Once
the files are in place, the thing's owner writes the records it keeps in
`~/.lnk` from files alone. The thing then appears in its kind's list,
stopped, until you meet its requirements and start it.

### Leave This Machine's Records Out of a Thing's Files

A record Local Link wrote into a thing's own files that names this
machine's identity stays here too. A chat records which work each
model request and proposed call was part of, naming its owner's ID on
this machine, and export carries it with that work left out. The thing's owner
says which files go in place of its own (`archive.carry`), and the files
on this machine never change.

### Recover a Partial Import

If a later thing fails, earlier things stay imported and the error names them.
Retry only the remaining selections with `--select` rather than importing the
whole archive again. A failure to read missing prerequisites leaves the thing
imported and reports that uncertainty alongside its owner's suggested command.

## State Moves and Prerequisites Stay

Everything Local Link keeps for a thing is either its state or what the
thing needs to run. An archive carries state only. Each machine sets up
the prerequisites with the owner's own commands, so an archive moves
between machines with different harnesses, clouds and credentials
unchanged.

### State Is in `~/Link`

A thing's state is its configuration, in its folder under `~/Link/Safe`,
and its files, in its folder under `~/Link`: an agent's settings, sandbox
file, instructions, skills, chats, memory and Home. Losing the machine
loses this state, and someone else needs it to have the same agent.

### Prerequisites Are in `~/.lnk`

What a thing needs to run is in `~/.lnk`: an installed harness in `exec`,
a connected cloud account, credentials and approvals in `safe`, and
caches in `data`. None of it is in an archive. Each is made on a machine
by one command, such as [`lnk agent create work --harness hermes`](../../../../../agents/agent/docs/agents/start/create.md#create-on-a-hosting-account) or `lnk
cloud connect aws|gcp`, which you run yourself.

### Identity Stays Here

An agent's ID, service identity, and owner are [machine-managed records](../../../../../agents/agent/docs/agents/state/files.md) in
`~/.lnk/safe/agents/<name>/`, along with its ports and disabled services.
Its `agent.toml` carries its setup and specification. These installation identity records
are not part of the archive, and an import creates a new agent.
Which locations are state, and the test that holds it, are in
[Where Link Keeps Things](../../../../../../docs/more/reference/locations.md#state-and-prerequisites).

## Archives Use Files Only

`lnk archive export` and `lnk archive import` read and write files on
this machine, and nothing else. They make no network call, start or stop
nothing, and run no program but the answers Link's own modules give to
the archive's questions, which read and write files too. A thing that's
running, or that needs more than its files to be carried whole, is
refused by name.

### Dependencies Keep Archives Offline

The archive package depends on no network or harness crate at all, and
the module links only the module protocol every module links and
packages that reach no network themselves. A repository check holds
that list, so export and import work on a machine with no network.

### Export Only What Is on This Machine

Export checks names against what's on this machine, from its own files.
It doesn't export an agent on a box or a cloud, and import doesn't ask
about one.

## Select Things by Kind and Name

A thing's name belongs to its kind's module: an agent is `main` to
[`lnk agent`](../../../../../agents/agent/docs/agents/README.md#agents). An archive holds things of several kinds, so it selects
them by kind and name together, `agent.main`. A kind and a name are
joined by `.`, which neither ever holds, so it splits one way. It never
names the thing: the agent is still `main`, its folders are `main`,
and every command but `lnk archive` takes the bare name.

### Names and IDs

An ID is `agent:a1b2c3d4e5f6`, and it's what grants and references
keep, since it never changes. IAM's dotted actions, such as `box.ssh`,
name things only by ID, so a selection never appears in one, and
neither is read as the other.

## Open Archives Safely

Import reads an archive as untrusted data and executes nothing. It
admits only `archive.toml`, a `README.md` beside it and the folders
`archive.toml` names, as plain files and folders, within the sizes it
allows and the space free here. It refuses absolute paths, `..`, links,
devices and duplicate entries, naming the entry, and stages the archive
in `~/.lnk/data/archives`, never in `~/Link`. How it's read and written
is in the [archive package](../../../../../../packages/archive/docs/tooling/archive/package.md).

## Ask Each Kind's Module Five Questions

The archive package does everything above for every kind. The kind's
module answers what only it knows, through five questions, each from files
alone: whether the thing's files are its own and whole
(`archive.check`), whether it's running (`archive.running`), which of its
files export carries in place of its own, so nothing of this machine's
goes (`archive.carry`), what an imported thing still needs and each one's
setup command (`archive.missing`), and the records it keeps in `~/.lnk` for a thing
arrived under a new name (`archive.adopt`). The wire is in
[Archive Questions](../../../contract/docs/tooling/archive/more/questions.md).
