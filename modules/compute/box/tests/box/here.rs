//! Boxes here with no VM: Box's own placement on this machine. A box is
//! a home folder, `~/Link/Boxes/<name>/home`, and a record in Box's
//! state; its commands run from that home in its sandbox, or directly
//! where the machine's policy allows. `lnk sandbox` is a stand-in that
//! logs the spec it was given and runs the command, so what's proved is
//! Box's side: the confinement each command asks for, never the
//! sandbox's own, which its module's tests hold.

use std::path::{Path, PathBuf};
use std::process::Output;

use link_test_kit::TempDir;

fn script(path: &Path, body: &str) {
    link_test_kit::write_program(path, format!("#!/bin/sh\n{body}\n")).unwrap();
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

struct World {
    temp: TempDir,
    home: PathBuf,
    bin: PathBuf,
}

impl World {
    fn new(test: &str) -> Self {
        let temp = TempDir::new(&format!("here-{test}"));
        let home = temp.join("home");
        let bin = temp.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        // `lnk`: Box and Env are the workspace's own; `lnk sandbox`
        // logs its spec and runs what follows `--`; `ran` says what it
        // was given and in which box.
        script(
            &bin.join("lnk"),
            &format!(
                r#"case "$1" in
  box) shift; exec '{lnk_box}' "$@" ;;
  env) shift; exec '{lnk_env}' "$@" ;;
  sandbox)
    case "$2" in
      spec) cat "$5" ;;
      run) echo "$5" >> '{sandboxed}'; shift 6; exec "$@" ;;
    esac ;;
  ran) shift; echo "ran $* in $LNK_BOX_HERE at $(pwd)" ;;
  *) echo "lnk: no $*" >&2; exit 1 ;;
esac"#,
                lnk_box = env!("CARGO_BIN_EXE_lnk-box"),
                lnk_env = link_test_kit::programs().join("lnk-env").display(),
                sandboxed = temp.join("sandboxed.log").display(),
            ),
        );
        World { temp, home, bin }
    }

    fn run_with(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        link_test_kit::env::command(self.bin.join("lnk"), &self.home)
            .args(args)
            .env("LNK_EXE", self.bin.join("lnk"))
            .env("LNK_SERVICES", "off")
            .envs(env.iter().copied())
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> Output {
        let output = self.run_with(args, &[]);
        assert!(output.status.success(), "lnk {args:?}: {}", text(&output));
        output
    }

    fn fails(&self, args: &[&str]) -> Output {
        let output = self.run_with(args, &[]);
        assert!(
            !output.status.success(),
            "lnk {args:?} succeeded: {}",
            text(&output)
        );
        output
    }

    fn box_home(&self, name: &str) -> PathBuf {
        self.home.join("Link/Boxes").join(name).join("home")
    }

    fn sandboxed(&self) -> String {
        std::fs::read_to_string(self.temp.join("sandboxed.log")).unwrap_or_default()
    }

    fn boxes(&self) -> String {
        std::fs::read_to_string(self.home.join(".lnk/safe/boxes/boxes.toml")).unwrap_or_default()
    }

    fn policy(&self, text: &str) {
        let policy = link_files::layout::Hidden::AgentPolicy.in_home(&self.home);
        std::fs::create_dir_all(policy.parent().unwrap()).unwrap();
        std::fs::write(policy, text).unwrap();
    }
}

/// `lnk box create --here` with no VM makes a home and a record, and
/// each command acts in that home through its sandbox, its spec the one
/// captured with the home among what it writes; stopped, it isn't
/// reached; deleted, it's gone with its home.
#[test]
fn a_box_here_runs_its_commands_in_its_sandbox_from_its_home() {
    let world = World::new("sandboxed");
    let spec = world.temp.join("box-sandbox.toml");
    std::fs::write(&spec, "schema = 1\n[files]\nread = [\"/srv\"]\n").unwrap();
    let made = world.ok(&[
        "box",
        "create",
        "--here",
        "--name",
        "h1",
        "--isolation.sandbox",
        &spec.display().to_string(),
        "--yes",
    ]);
    let home = world.box_home("h1");
    assert!(home.is_dir(), "{}", text(&made));
    let boxes = world.boxes();
    assert!(boxes.contains("kind = \"sandbox\""), "{boxes}");
    assert!(
        boxes.contains("/srv") && boxes.contains(&home.display().to_string()),
        "{boxes}"
    );
    // A shell's command line and `lnk` both run in the box, from its home.
    let said = world.ok(&["box", "ssh", "h1", "--", "pwd"]);
    assert_eq!(
        String::from_utf8_lossy(&said.stdout).trim(),
        home.display().to_string()
    );
    let ran = world.ok(&["box", "run", "h1", "--", "ran", "hello"]);
    assert!(
        text(&ran).contains(&format!("ran hello in h1 at {}", home.display())),
        "{}",
        text(&ran)
    );
    let spec_file = world.home.join(".lnk/safe/boxes/here/h1.toml");
    assert_eq!(world.sandboxed().lines().count(), 2);
    assert!(
        world
            .sandboxed()
            .lines()
            .all(|l| l == spec_file.display().to_string())
    );
    // Files go into the home, never outside it.
    let file = world.temp.join("notes.txt");
    std::fs::write(&file, "hi").unwrap();
    world.ok(&["box", "copy", "h1", &file.display().to_string(), "~/in"]);
    assert_eq!(
        std::fs::read_to_string(home.join("in/notes.txt")).unwrap(),
        "hi"
    );
    world.fails(&["box", "copy", "h1", &file.display().to_string(), "../out"]);
    assert!(!world.box_home("h1").parent().unwrap().join("out").exists());
    // It's listed here, as a box of this machine.
    let listed = world.ok(&["box", "list", "--here", "--json"]);
    assert!(String::from_utf8_lossy(&listed.stdout).contains("\"h1\""));
    // A box user is a login of a machine of its own.
    let users = world.run_with(&["box", "user", "list"], &[("LNK_BOX_HERE", "h1")]);
    assert!(!users.status.success());
    assert!(
        text(&users).contains("which is a box here"),
        "{}",
        text(&users)
    );
    // Stopped, nothing runs in it; started, it does again.
    world.ok(&["box", "stop", "h1"]);
    assert!(text(&world.fails(&["box", "ssh", "h1", "--", "true"])).contains("stopped"));
    world.ok(&["box", "start", "h1"]);
    world.ok(&["box", "ssh", "h1", "--", "true"]);
    // Deleted with --yes, the record and the home both go.
    world.ok(&["box", "delete", "h1", "--yes"]);
    assert!(!world.boxes().contains("h1"));
    assert!(!home.exists() && !spec_file.exists());
}

/// `--no-sandbox` runs a box's commands directly, and only where this
/// machine's policy allows it: refused before anything is made, and
/// refused again at a run once the policy forbids it.
#[test]
fn a_box_here_with_no_sandbox_runs_directly_where_policy_allows() {
    let world = World::new("direct");
    world.policy("unsandboxed = false\n");
    let refused = world.fails(&[
        "box",
        "create",
        "--here",
        "--name",
        "d1",
        "--isolation.no_sandbox",
        "--yes",
    ]);
    assert!(
        text(&refused).contains("forbids running with no sandbox"),
        "{}",
        text(&refused)
    );
    assert!(!world.box_home("d1").exists());
    assert!(!world.boxes().contains("d1"));
    world.policy("unsandboxed = true\n");
    world.ok(&[
        "box",
        "create",
        "--here",
        "--name",
        "d1",
        "--isolation.no_sandbox",
        "--yes",
    ]);
    assert!(
        world.boxes().contains("kind = \"direct\""),
        "{}",
        world.boxes()
    );
    let ran = world.ok(&["box", "run", "d1", "--", "ran", "direct"]);
    assert!(text(&ran).contains("ran direct in d1"), "{}", text(&ran));
    assert_eq!(world.sandboxed(), "");
    world.policy("unsandboxed = false\n");
    let later = world.fails(&["box", "run", "d1", "--", "ran", "again"]);
    assert!(
        text(&later).contains("forbids running with no sandbox"),
        "{}",
        text(&later)
    );
    // Its sandbox flags are refused together, as Agent's are.
    world.fails(&[
        "box",
        "create",
        "--here",
        "--isolation.no_sandbox",
        "--isolation.sandbox",
        "s.toml",
        "--yes",
    ]);
}

/// A box here with an environment sets it up on this machine as `lnk
/// env setup --here` does, directly with `--no-sandbox`, and runs its
/// stages at every start again at `lnk box start`.
#[test]
fn a_box_here_sets_its_environment_up_on_this_machine() {
    let world = World::new("environment");
    let env = world.temp.join("env.toml");
    std::fs::write(
        &env,
        "schema = 1\nname = \"app\"\nversion = 1\n\n[[stage]]\nname = \"hello\"\n\
         run = \"echo started >> $HOME/started\"\nsandbox = \"run.toml\"\nwhen = \"start\"\n",
    )
    .unwrap();
    std::fs::write(world.temp.join("run.toml"), "schema = 1\n").unwrap();
    let made = world.ok(&[
        "box",
        "create",
        "--here",
        "--name",
        "e1",
        "--isolation.no_sandbox",
        "--env",
        &env.display().to_string(),
        "--yes",
    ]);
    let started = || std::fs::read_to_string(world.home.join("started")).unwrap_or_default();
    assert_eq!(started(), "started\n", "{}", text(&made));
    world.ok(&["box", "stop", "e1"]);
    world.ok(&["box", "start", "e1"]);
    assert_eq!(started(), "started\nstarted\n");
    // Nothing went through a sandbox: the box runs directly.
    assert_eq!(world.sandboxed(), "");
}
