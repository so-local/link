# Interface

The environment module exposes process commands for running setup and
services on another machine. Their requests carry the environment,
sealed values and stages; the caller owns the purpose of the run, its
record and its output.

## `lnk env here`

This hidden command runs on the target machine. Its first output line
is `lnk-seal-to <key>`, from `link_secretive::Recipient::announce`:
a base64 X25519 public key made for this run. It then reads one JSON
request line and keeps the input open for the run's lifetime.

### Request fields

The request contains `name`, `file`, `action`, `given` and `mark`, and
optionally `direct`.
The environment's named files are already installed under
`~/.lnk/safe/envs/envs/<name>/`. Each sent stage is a
`link_env_contract::stage::Shipped`, with its sandbox specification
carried as text.

### Actions

The action's `do` chooses the operation. Each operation uses the same
environment values and machine boundary, with its own work to execute.

- **`image`:** run the stages saved in the image.
- **`setup`:** run those stages without saving an image.
- **`start`:** run start stages and services, which outlive startup.
- **`run`:** run the supplied `stages`, with optional `variables`, `secrets` and `out`.
- **`reset`:** run the named `service`'s reset command.
- **`stop`:** stop the services a start left running, removing their background services.

### Running directly

With `direct = true`, each run the machine's own user makes goes in no
sandbox: what `--no-sandbox` asks of a box here or an agent. It is
refused where this machine's agent policy says `unsandboxed = false`.
A sealed secret is held by a sandbox's proxy, so a direct run that would
be given one is refused rather than given its value. Root and other box
users' runs are unchanged.

### A run's output folder

The optional `out` value contains `folder` and `variable`: a folder
relative to home, created for stages to leave their files in, and the
variable that gives them its full path. That variable may be one of
Link's `LNK_` names because it is supplied by the caller.

### Sealed values

The `given` value carries `secrets`, `sealed` and `sealing`, encrypted
for the announced key with `link_secretive::Sealed::seal_json`.
Its envelope identifies `owner = "env:<name>"`, `kind = "given"` and
`version = 1`, so those values belong to this environment and run.

### Progress and output

Command output passes through unchanged. Error-output lines beginning
with `mark` describe progress: `stage <name> <seconds>`,
`service <name> <seconds>`, `running`, `failed <step>` or
`reset <service>`. The mark lets callers distinguish runner events
from the command's own output.

### Lifetime and status

Closing standard input stops everything the command started. The exit
status is the run's code, including 124 for a stage timeout; other
operations return 0 on success and 1 when a step fails.

### Machine boundary

Image builds and every root run require a box, as
`/etc/lnk/kind.toml` and `link_machine::machine::kind` report it.
They are refused on a computer before execution. A caller's reason for
the run does not widen this boundary.

### Callers

Image, start and reset commands use this interface through
[`lnk box run`](../../../../../box/docs/services/boxes/ssh.md#run-lnk-on-a-box). `lnk env image create here` on a box runs it on that
machine, and so do a box here and an agent's environment, through
`link_env_contract::remote::set_up_here`, `start_here` and `stop_here`. Other programs working in an environment use the same
interface and retain ownership of their own records and output.

## `lnk env serve`

The hidden `lnk env serve --spec <file> <service>` command runs a service in
its sandbox on the target machine until stopped, or in none with
`--no-sandbox` for an environment started directly. The background
service created when a box starts the environment invokes it for each service.

## Captured Work Directories

The environment module owns three internal commands for a program that
works in an environment's workdir.
`lnk env export --spec <file>` returns `link_env_contract::environment::Packed`,
the existing job environment contract's `file`, `hash` and `files` map.
The files are text, relative to the environment's folder, and contain
secret names rather than their values. At most 1024 files and 8 MiB
cross this boundary.

### Import and Prepare

`lnk env import` reads the packed JSON on standard input, validates its
paths, strict environment shape, hash, and declared-file set, then installs it.
A computer requires an exact match to its reviewed copy. `lnk env prepare
--spec <file>` prepares a reviewed local file directly. Both return the
[Environment Contract](../../../../contract/docs/services/environments/more/contract.md#packed)'s
`Prepared` as a final JSON line with target-local `env` and `workdir` paths,
after setup progress. Successful build stages are reused by image hash before
start stages and services run, and root setup is refused on a computer.
