# Service Contract

> Manage Services Through One Process Contract

`link-service` runs background services as `lnk`'s modules
run them, through one job and lifecycle API. Native machines use launchd
or systemd, while explicitly configured Linux guests use a durable
process supervisor.

## A service

A `Job` is what a service runs: its `argv`, `env`, working folder and
log. A service is named by a launchd label and a systemd unit name, and
its file is a plist (`plist_path_of`) or a unit (`unit_path_of`).

### Native Units

Linux registers unit files by absolute path before reloading systemd.
A unit in a custom `HOME` or `XDG_CONFIG_HOME` can therefore start even when
its folder is outside the user manager's search path. Paths retain their
original filesystem bytes. Starts, stops and status queries use the current
user's bus in `/run/user/<uid>` when `XDG_RUNTIME_DIR` is absent, while an
explicit runtime directory is preserved.

### Local Service Peers

`peer::uid` obtains a connected Unix socket's peer identity from the
kernel, using Linux credentials or macOS peer IDs. `peer::owner`
accepts root or the service's own user for administrative operations.
It never trusts a user ID supplied in a request. Workload protocols
still authorize their principal and resource scope independently.

### What a job may not say

A job whose unit or plist could say more than it means is refused: a
control character, such as a newline, in a systemd value would start a
new directive.

### `LNK_SERVICES=off`

With it, which tests set, neither launchd nor systemd is asked anything
(`managed`). Both `off` and `0` also
skip guest marker selection and supervision. Installation and removal
keep their inert service-file behavior without changing guest intent.
A service's name doesn't depend on `HOME`, so a test's agent would
otherwise stop or replace yours.

### What a unit is handed

`setenv_args` is `systemd-run`'s `--setenv` for each variable of this
process a unit of Link's is started with: `HOME`, `PATH`, `LANG`,
`XDG_CONFIG_HOME` and Link's own (`LNK_*`), but one naming a secret (a
token, key, password). By name only, so `systemd-run` takes each value
from its own environment and none is on its command line for `ps` to
show.

### Guest Process Entry

A guest supervisor and worker enter through the program's hidden `guest-service` command. The service package owns the typed request: the process role, unit, and launch epoch. Callers build its arguments from that type, and the program parses it before choosing a runtime. The driver verifies the epoch before it touches the service's state.

## Starting and stopping

`start` installs the service for its job and (re)starts it, and
`start_all` starts several, with one reload of systemd for all of them
on Linux. `stop` stops the service and removes it, so it doesn't start
at login.

### `stop_before_start` and `reload`

`stop_before_start` stops one, leaving systemd's reload to the `start`
or `start_all` that follows, since each reload reloads every unit;
`reload` has systemd read the units again after ones with no start to
follow. There's nothing to reload with launchd.

### `stop_from_within`

Stops a service from its own process, by its file: the file removed, so
neither a restart nor the next login runs it, and its stop not waited
for, since it ends this process. Best effort.

### `pause` and `resume`

`pause` stops a service for now, its file kept, so `resume` starts it
again as it was: an agent stopped for being idle.

### Another program's service

`unload_other` stops a service of another program's by its label or
unit, before its file is removed, and `load_other` starts it again
once its file is back. Both are best effort.

### On macOS

Unloading waits, up to 10 seconds, until launchd has let the job go,
since `bootout` returns before the job is gone.

## System Services

`system::Service` adds a host-owned name and user to the same `Job`.
Its plan renders a systemd system unit or a launchd system daemon with
the shared argument and XML escaping. `system::Control` owns install,
stop and removal commands, with an injectable native runner for tests.
System services start without a login session and use private files.
Credentials belong in separate private files named by arguments, not
inside a service definition. Native manager authority remains required.

## After logout

systemd stops a user's services when their last session ends, unless
lingering is on for the user: on a server reached over SSH, a service
would stop at logout. `keep_after_logout` turns it on, which systemd
lets users do for themselves on most systems, or else says how.
Nothing on macOS.

## Linux guests

A prepared Linux guest without systemd selects process supervision
with `/etc/lnk/service.toml`. Missing native service tools never select
this mode, and the caller continues using the same `Job`, start, stop,
pause, resume and status functions.

### Explicit selection

The closed configuration contains exactly `schema = 1` and
`mode = "guest"`. The file and its ancestors must belong to root and
deny writes by other users, with trusted sticky directories allowed by
the shared file reader. The reader refuses links, nonregular files,
unknown fields, unsupported versions and configurations over 4 KiB.
Only an absent file in trusted folders selects native behavior.

### Process entry

An executable declares the hidden `guest-service` command using the shared
`cli::GuestEntry` type. After its invocation boundary parses that request,
it dispatches it to `guest_service_entry` before starting a runtime or worker
threads. The function returns `Result<()>`; ordinary invocation stays with
the caller's normal command dispatch. Non-Linux builds refuse guest entry.

### Lifecycle and readiness

Start waits for the worker to report the actual job process identity.
This confirms execution, while application readiness remains the
caller's check. The controller survives the initiating terminal,
restarts an exited job after ten seconds and stays active while pause
stops the worker. Resume explicitly restarts a paused job, including
after controller loss. A guest reboot requires a new explicit start
or resume, and makes old process identities stale.

### Ownership and recovery

The controller owns an unreaped worker process-group leader through
[`OwnedChild`](../../../../../secretive/docs/more/libraries/files/secretive.md#ownedchild).
Its main thread remains active during pause. Kernel parent-death
notification makes the worker stop its own group when the controller
dies. Persisted boot identity, process start ticks and generation fence
status and late completions. Recovery never signals a stored numeric
PID, and refuses replacement when an old group may still be live.

### Durable intent

Private state under `.lnk/safe/services/<unit>/` records the job, desired
state, revision, controller generation, process identities and start
count. Short metadata transactions precede external work. Stop records
its intent before waiting for confirmed cleanup, while stop from within
the service records that intent without waiting on itself. A unit file
remains at `unit_path_of` for the existing installed-service checks.

### Bounds and scope

Control waits and lock acquisition each have a ten-second deadline.
Jobs have at most 256 arguments and a serialized size below 128 KiB,
with absolute work and log paths. Each output pump uses an 8 KiB buffer
and truncates its private log before a write would exceed 8 MiB,
resetting the recorded start offset. Descendants must stay in the
owned process group. This guest lifecycle adds no host confinement or
resource guarantee, which belongs to the VM's host authority.

### Debug configuration path

Debug builds accept `LNK_TEST_SERVICE_SETTINGS` as a different marker
path, retaining all production root and ancestor checks. Release
builds always use `/etc/lnk/service.toml`. Unit fixtures inject owners
only in test code, and a path override never authorizes an untrusted
marker or substitutes for guest bootstrap verification.

## Its log

`open_log` opens the log for a new start: a regular file, only the
user's, in a folder only the user's, which launchd or systemd (outside
the sandbox) then appends the service's output to, or a start in a
terminal does itself. It records where this start's lines begin, kept
next to the log (`start_path`), for [`lnk agent log show`](../../../../../../modules/agents/agent/docs/agents/start/log/README.md#read-the-log).

## Its state

`running` says whether a service is installed and its process running,
and `status` gives its `Status`, from one `launchctl print` or
`systemctl show`. `running_each` and `status_each` ask for several at
once, in one `systemctl` or `launchctl`, rather than one per service.

### Without a systemd

`systemctl` runs as the user, reaching the user's bus in
`/run/user/<uid>` from a service's environment, which may lack it.
systemd's error when this user has no systemd of their own (over SSH,
in WSL or a container) is turned into what to do about it.

## Replace a Running Program

[Program Exchange](services/exchange.md) explains the handoff protocol
used to replace a running executable while preserving its service role.
