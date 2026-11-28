//! `lnk agent move` end to end, with two homes on this machine standing
//! for this computer and a box, a fake `lnk` (`lnk box run b1 -- ...`
//! runs the harness module in the box's home) and a fake adapter. The
//! agent isn't started (`--no-start`): what's checked is what moves.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use link_test_kit::TempDir;

mod backups;
mod boxes;
mod finding;
mod harnesses;
mod holding;
mod hosted;
mod members;
mod recovering;
mod switching;
mod to_a_box;
mod trust;

/// The agent's ID in these tests.
const ID: &str = "a1b2c3d4e5f6";

const ADAPTER: &str = r#"#!/bin/sh
here=$(dirname "$0")
case "$1" in
  needs) cat > /dev/null; [ -f "$here/fake-needs.json" ] && cat "$here/fake-needs.json" ;;
  state) echo '{"include": ["~/.fake"], "exclude": ["~/.fake/bin", "~/.fake/.env"]}' ;;
  traces) echo '[]' ;;
  *) exit 0 ;;
esac
"#;

struct World {
    root: TempDir,
    here: PathBuf,
    there: PathBuf,
    bin: PathBuf,
    /// `lnk-agent` alone in a folder, no other module beside it: one
    /// `lnk module add` (the fake's) links the module it names in.
    alone: PathBuf,
    /// Each machine's own runtime folder, short as a machine's is:
    /// `here`, `there` and `old` in it, so one's sockets aren't another's.
    runtimes: TempDir,
}

/// Link's connections' modules, each answering `describe` as its module
/// does: what a move's other end keeps of the connection.
const CONNECTIONS: [(&str, &str); 3] = [
    (
        "lnk-telegram",
        r#"{"protocol":1,"title":"Telegram","secret_fields":["bot_token"],"bridged":true}"#,
    ),
    (
        "lnk-slack",
        r#"{"protocol":1,"title":"Slack","secret_fields":["bot_token","app_token"],"bridged":true}"#,
    ),
    (
        "lnk-discord",
        r#"{"protocol":1,"title":"Discord","secret_fields":["bot_token"],"bridged":true}"#,
    ),
];

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn executable(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    link_test_kit::write_program(path, text).unwrap();
}

/// `lnk-storage`, next to `lnk-agent`, where a move and a backup find
/// it.
fn bucket_module() {
    link_test_kit::programs();
}

impl World {
    fn new(test: &str) -> Self {
        Self::at(TempDir::new(&format!("moving-{test}")))
    }

    fn at(root: TempDir) -> Self {
        bucket_module();
        let (here, there, old, bin) = (
            root.join("here"),
            root.join("there"),
            root.join("old"),
            root.join("bin"),
        );
        std::fs::create_dir_all(there.join(".lnk")).unwrap();
        std::fs::create_dir_all(old.join(".lnk")).unwrap();
        let runtimes = TempDir::short("mv");
        for machine in ["here", "there", "old"] {
            std::fs::create_dir_all(runtimes.join(machine)).unwrap();
        }
        crate::common::write_adapter(&bin.join("lnk-fake"), ADAPTER).unwrap();
        // The login module, beside lnk: signed in where the home holds an
        // account (its whoami); else out.
        executable(
            &bin.join("lnk-login"),
            r#"#!/bin/sh
[ "$*" = "--link-answer lnk.account" ] || exit 1
cat > /dev/null
[ -e "$HOME/account.json" ] || exit 1
printf '{"protocol": 1, "answer": '; cat "$HOME/account.json"; echo '}'
"#,
        );
        for (program, describe) in CONNECTIONS {
            let script = format!(
                "#!/bin/sh\n[ \"$1\" = describe ] && echo '{describe}' && exit 0\nexit 1\n"
            );
            executable(&bin.join(program), &script);
        }
        // A cloud account that hosts agents: the host fixture, in a folder.
        std::fs::create_dir_all(root.join("host")).unwrap();
        std::os::unix::fs::symlink(
            link_test_kit::programs().join("lnk-agent-place-fixture"),
            bin.join("lnk-agent-place-fixture"),
        )
        .unwrap();
        let harness = link_test_kit::programs().join("lnk-agent");
        let alone = root.join("alone");
        std::fs::create_dir_all(&alone).unwrap();
        // A copy keeps module discovery here and gives this process its
        // own executable inode while concurrent builds replace programs.
        link_test_kit::copy_program(&harness, &alone.join("lnk-agent")).unwrap();
        let built = link_test_kit::programs();
        let harness = harness.display();
        std::fs::create_dir_all(&bin).unwrap();
        link_test_kit::write_core(
            &bin.join("lnk"),
            format!(
                r#"#!/bin/sh
case "$1 $2 $3" in
  "box list --json"|"box list --here")
    # The box `box create --cloud azure` makes, once it has; else one found in
    # azure by its tags alone (no created time), once one is planted.
    new=''
    [ -e '{root}/planted' ] && new=', {{"name": "c1", "account": "azure", "state": "running", "created": 0, "found": "cloud"}}'
    [ -e '{root}/started' ] && new=', {{"name": "azure-1", "account": "azure", "state": "running", "created": 5}}'
    # The box `box create --cloud aws --image` makes, beside b1.
    [ -e '{root}/started-aws' ] && new="$new"', {{"name": "aws-2", "account": "aws", "state": "running", "created": 9}}'
    # A lost box is gone from the list.
    b1='{{"name": "b1", "account": "aws", "state": "running", "created": 3}}, '
    [ -e '{root}/lost-b1' ] && b1=''
    echo '['"$b1"'{{"name": "b2", "account": "gcp", "state": "stopped", "resource": "box:p8r2c4n6w0zt", "created": 2}}, {{"name": "old", "account": "gcp", "state": "running", "created": 1}}'"$new"']' ;;
  # No box here is a managed VM guest: each keeps its own networking.
  "box machine "*|"box machine") cat >/dev/null; echo '{{"version": 1, "result": {{"Ok": {{"kind": "not-managed"}}}}}}' ;;
  "cloud list --json") echo '[{{"name": "aws"}}, {{"name": "gcp"}}, {{"name": "azure"}}, {{"name": "host"}}]' ;;
  "cloud env host") echo '{{"name": "host", "kind": "agent-place-fixture", "place": "", "env": {{"LNK_AGENT_PLACE_FIXTURE": "{root}/host"}}}}' ;;
  "box create --cloud")
    case "$4" in
      azure) echo "$*" >> '{root}/started' ;;
      aws) echo "$*" >> '{root}/started-aws' ;;
    esac ;;
  "box run b1"|"box run azure-1"|"box run c1"|"box run aws-2")
    echo "$*" >> '{root}/ran-there'
    # b1 lost, or there but not answering (dark).
    [ "$3" = b1 ] && {{ [ -e '{root}/lost-b1' ] || [ -e '{root}/dark-b1' ]; }} && exit 255
    shift 4
    case "$1 $2" in
      "module add"|"sandbox check") echo "$*" >> '{root}/readied' ;;
      *) [ "$1" = agent ] && shift
         case "$*" in *"show --json"*)
           [ -e '{root}/never-answers-b1' ] && exit 1
           # The move killed as it asks whether the agent answers there.
           [ -e '{root}/kill-at-answer' ] && {{ rm '{root}/kill-at-answer'; kill -KILL $PPID; exit 1; }} ;; esac
         # A box whose lnk agent left fails there.
         case "$*" in *" left --"*) [ -e '{root}/never-leaves-b1' ] && exit 1 ;; esac
         # The move killed in its copy, or once the agent is paused.
         case "$*" in *"receive --no-start --phase copy"*) [ -e '{root}/kill-at-copy' ] && {{ kill -KILL $PPID; exit 1; }} ;; esac
         case "$*" in *"receive --no-start --phase switch"*)
           # What changes between the copy and the switch.
           [ -e '{root}/before-switch' ] && sh '{root}/before-switch'
           [ -e '{root}/kill-at-switch' ] && {{ kill -KILL $PPID; exit 1; }}
           # The box lost as the switch starts.
           [ -e '{root}/lose-at-switch' ] && {{ : > '{root}/lost-b1'; exit 255; }}
           # The network dropped once the switch landed there.
           [ -e '{root}/drop-after-switch' ] && {{ rm '{root}/drop-after-switch'; HOME='{there}' XDG_RUNTIME_DIR='{runtimes}/there' HOME_NAME=there LNK_TEST_MACHINE_KIND='{there}/kind.toml' '{harness}' "$@"; : > '{root}/dark-b1'; exit 255; }} ;; esac
         # The box stops answering once the agent started there.
         case "$*" in "-a main start"*) [ -e '{root}/dark-after-start' ] && {{ HOME='{there}' XDG_RUNTIME_DIR='{runtimes}/there' HOME_NAME=there LNK_TEST_MACHINE_KIND='{there}/kind.toml' '{harness}' "$@"; : > '{root}/dark-b1'; exit 0; }} ;; esac
         # A switch the box refuses, before anything of it is read.
         case "$*" in *"receive --no-start --phase switch"*) [ -e '{root}/refuse-switch' ] && {{ echo "refused" >&2; exit 1; }} ;; esac
         # Ctrl-C, as the move asks what its harness needs there.
         case "$*" in *needs*) [ -e '{root}/stop-at-needs' ] && kill -INT $PPID ;; esac
         HOME='{there}' XDG_RUNTIME_DIR='{runtimes}/there' HOME_NAME=there LNK_TEST_MACHINE_KIND='{there}/kind.toml' exec '{harness}' "$@" ;;
    esac ;;
  "box run old")
    shift 4
    [ "$1" = agent ] && shift
    HOME='{old}' XDG_RUNTIME_DIR='{runtimes}/old' exec '{harness}' "$@" ;;
  agent*)
    shift
    # Services don't run in tests: an agent whose systemd unit is written
    # here (its start ran) counts as running, as systemd would say.
    case "$*" in
      *"show --json"*)
        # Where it went, it never answers as running.
        [ -e '{root}/never-answers' ] && exit 1
        HOME='{here}' XDG_RUNTIME_DIR='{runtimes}/here' HOME_NAME=here '{harness}' "$@" | if [ -e '{here}/.config/systemd/user/lnk-agent-{ID}.service' ]; then sed 's/"running":false/"running":true/'; else cat; fi ;;
      *) HOME='{here}' XDG_RUNTIME_DIR='{runtimes}/here' HOME_NAME=here exec '{harness}' "$@" ;;
    esac ;;
  "storage push"*) echo "$*" >> '{root}/pushed' ;;
  "storage list --json") [ -e '{root}/no-clouds' ] && echo '[]' || exit 1 ;;
  "storage export --owner") echo '{{"password": "OBSCURED", "buckets": {{"aws": {{"name": "local-link-k3v9q2x7m1ab", "resource": "bucket:b1u2c3k4e5t6"}}}}}}' ;;
  "storage keys --owner") echo '{{"aws": {{"bucket": "local-link-k3v9q2x7m1ab", "credential": {{"type": "s3"}}}}}}' ;;
  "storage adopt --owner") cat > '{root}/adopted-'"$HOME_NAME" ;;
  "storage leave --owner"|"storage revoke --owner") echo "$*" >> '{root}/buckets' ;;
  "storage restore --owner")
    IFS= read -r password; printf '%s\n' "$password" > '{root}/restored-with'
    echo "$*" > '{root}/restored'
    mkdir -p "$6/Backups" && cp '{root}/backup.tar' "$6/Backups/heron.tar" && cp '{root}/backup.tar' "$6/Backups/box.tar" && echo pulled > "$6/notes.txt" && {{ [ ! -f '{root}/own.tar' ] || cp '{root}/own.tar' "$6/Backups/Agent.tar"; }}
    echo '{{"owner": "agent:{ID}", "buckets": {{"aws": "local-link-k3v9q2x7m1ab"}}}}' ;;
  "storage find --owner") echo '[{{"cloud": "aws", "bucket": "local-link-k3v9q2x7m1ab", "place": "eu-west-1", "owner": "agent:{ID}", "name": "main"}}]' ;;
  "storage pull --replace")
    # Storage refuses an owner's folder while its owner runs.
    if [ -e '{root}/running' ] && [ ! -e '{root}/stopped' ]; then echo 'its owner is running' >&2; exit 1; fi
    cp '{root}/backup.tar' "$4" && {{ [ -z "$5" ] || [ ! -f '{root}/own.tar' ] || cp '{root}/own.tar' "$5"; }} && echo "$*" >> '{root}/pulled' ;;
  "storage pull "*) mkdir -p "$3/Backups" && cp '{root}/backup.tar' "$3/Backups/fake.tar" && echo pulled > "$3/notes.txt" ;;
  "module add storage"|"module add box"|"module add cloud")
    echo "$*" >> '{root}/added'; ln -s '{built}'/lnk-$3 '{alone}'/lnk-$3 ;;
  "sandbox tool remove") echo "$*" >> '{root}/tools-removed' ;;
  # Every other sandbox command here, by the real sandbox module.
  "sandbox "*) shift; exec '{sandbox}' "$@" ;;
  # Link MCP asks the built core for its registry.
  "--link-registry "*) exec '{built}/lnk' "$@" ;;
  # Link's modules, as its registry has them, then those from outside.
  "module list --json") echo '[{{"name": "login", "installed": true, "answers": ["lnk.account"]}}, {{"name": "box", "installed": false}}, {{"name": "storage", "installed": false}}, {{"name": "cloud", "installed": false}}, {{"name": "exit", "installed": false}}, {{"name": "fake", "installed": true, "source": "https://github.com/someone/link-fake", "signer": "ssh-ed25519 FAKE"}}, {{"name": "heron", "installed": true, "source": "https://github.com/someone/link-heron", "signer": "ssh-ed25519 AAAA"}}]' ;;
  *) echo "fake lnk: $*" >&2; exit 1 ;;
esac"#,
                there = there.display(),
                old = old.display(),
                here = here.display(),
                root = root.display(),
                alone = alone.display(),
                built = built.display(),
                sandbox = crate::common::sandbox_module().display(),
                runtimes = runtimes.display(),
            ),
        )
        .unwrap();
        World {
            root,
            here,
            there,
            bin,
            alone,
            runtimes,
        }
    }

    fn lnk_agent(&self, args: &[&str]) -> Output {
        self.lnk_agent_by(&link_test_kit::programs().join("lnk-agent"), args)
    }

    /// [`World::lnk_agent`] by the harness with no module beside it.
    fn lnk_agent_alone(&self, args: &[&str]) -> Output {
        self.lnk_agent_by(&self.alone.join("lnk-agent"), args)
    }

    fn added(&self) -> String {
        std::fs::read_to_string(self.root.join("added")).unwrap_or_default()
    }

    fn lnk_agent_by(&self, harness: &Path, args: &[&str]) -> Output {
        self.command_by(harness, args).output().unwrap()
    }

    /// [`World::lnk_agent`] on the box, as another computer's move runs
    /// it there.
    fn lnk_agent_there(&self, args: &[&str]) -> Output {
        self.lnk_agent_command(args)
            .env("HOME", &self.there)
            .env("XDG_RUNTIME_DIR", self.runtimes.join("there"))
            .env("HOME_NAME", "there")
            .env("LNK_TEST_MACHINE_KIND", self.there.join("kind.toml"))
            .output()
            .unwrap()
    }

    /// [`World::lnk_agent`], to start: its output not yet read.
    fn lnk_agent_command(&self, args: &[&str]) -> Command {
        self.command_by(&link_test_kit::programs().join("lnk-agent"), args)
    }

    fn command_by(&self, harness: &Path, args: &[&str]) -> Command {
        let mut command = link_test_kit::env::command(harness, &self.here);
        command
            .args(args)
            .env_clear()
            .env("LNK_SERVICES", "off")
            .envs(link_test_kit::keychain::isolated(&self.here))
            .env("HOME", &self.here)
            .env(
                "XDG_RUNTIME_DIR",
                link_test_kit::env::made_runtime(&self.here),
            )
            .env("XDG_RUNTIME_DIR", self.runtimes.join("here"))
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("LNK_EXE", self.bin.join("lnk"))
            .stdin(Stdio::null());
        command
    }

    fn agent(&self, home: &Path) -> PathBuf {
        home.join(".lnk/data/agents/main/harnesses/fake/.fake")
    }

    fn files(&self, home: &Path) -> PathBuf {
        home.join("Link/Agents/main/home")
    }
}

/// What a command said, stdout then stderr.
fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn ok(output: &Output) -> String {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{text}");
    text
}

/// The fake's agent `main`, with a hosted model, on this computer.
fn an_agent_here(world: &World) {
    crate::common::write_agent(
        &world.here,
        "main",
        &format!(
            "id = \"agent:{ID}\"\nharness = \"fake\"\nfiles = \"{}\"\n\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-test\"\n",
            world.files(&world.here).display()
        ),
    );
}

/// An adapter whose harness speaks no connection: Link carries them to its
/// endpoint.
const BARE: &str = r#"#!/bin/sh
case "$1" in
  state) echo '{"include": ["~/.bare"], "exclude": []}' ;;
  connections) echo '[]' ;;
  endpoint) echo '"http://127.0.0.1:18800/v1/chat/completions"' ;;
  traces) echo '[]' ;;
  *) exit 0 ;;
esac
"#;

/// A Signal module answering `state` as `lnk-signal` does.
const SIGNAL: &str = r#"#!/bin/sh
case "$1" in
  state) echo '{"include": ["~/.signal-cli/data", "~/.signal-cli/account"], "exclude": []}' ;;
  describe) echo '{"protocol": 1, "title": "Signal", "linked": true, "bridged": true, "cli": true}' ;;
  *) exit 1 ;;
esac
"#;

fn only_entry(dir: &Path) -> PathBuf {
    let entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(entries.len(), 1, "{entries:?}");
    entries[0].clone()
}

/// The agent's files, every one, by its path in the folder `files`.
fn every_file(files: &Path) -> std::collections::BTreeMap<String, String> {
    let mut found = std::collections::BTreeMap::new();
    let mut folders = vec![files.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                let rel = path.strip_prefix(files).unwrap().display().to_string();
                found.insert(rel, text);
            }
        }
    }
    found
}

/// An agent here with files, a conversation kept in its Home as an
/// earlier release kept it (it hasn't started since), and memory its
/// harness keeps.
fn an_agent_with_files(world: &World) {
    an_agent_here(world);
    let files = world.files(&world.here);
    write(&files.join("notes.txt"), "from the Mac");
    write(&files.join("taxes/2025.txt"), "a tax report");
    write(
        &world
            .here
            .join("Link/Agents/main/agent/chats/telegram-42/events.jsonl"),
        "{\"seq\":1,\"event\":1}\n",
    );
    write(&world.agent(&world.here).join("memory/first"), "remembered");
}

/// What the host fixture was asked, in order.
fn host_log(world: &World) -> Vec<String> {
    std::fs::read_to_string(world.root.join("host/log"))
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| l != "agent-runs" && l != "agent-list")
        .collect()
}
