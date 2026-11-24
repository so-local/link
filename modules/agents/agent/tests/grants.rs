//! Two layers of grants: what you grant an agent (`lnk agent permission add`,
//! `--sandbox`), which follows it to any harness, and what a harness
//! needs beyond that, asked before it runs, remembered, and held only
//! while it runs (`lnk agent harness-permission add`, `deny`, `reset`).

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use link_test_kit::TempDir;

use crate::common;

mod agents_grants;
mod folders;
mod harness_needs;
mod sandbox_file;
mod tools;

/// A harness that needs what `needs.json` beside it says, keeping the
/// settings `needs` was handed in `needs.in`; it runs and exits. Without
/// `needs.json` it doesn't know `needs`, and says so as adapters do. It
/// keeps every conversation in Link's store, as each fake here does, so
/// a switch among them leaves nothing behind.
const NEEDY: &str = r#"#!/bin/sh
here=$(dirname "$0")
case "$1" in
  contract) printf '{"version":1,"proxy":false,"state":{"export":["sessions"],"import":["sessions"]},"notes":true}' ;;
  needs)
    [ -f "$here/needs.json" ] || { echo "lnk-needy: unknown command needs" >&2; exit 2; }
    env > "$here/needs.env"; cat > "$here/needs.in"; cat "$here/needs.json" ;;
  configure) cat > "$HOME/configure.in" ;;
  command) printf '{"argv":["/bin/true"],"ports":[]}' ;;
  *) exit 0 ;;
esac
"#;

/// A harness that needs nothing more.
const PLAIN: &str = r#"#!/bin/sh
case "$1" in
  contract) printf '{"version":1,"proxy":false,"state":{"export":["sessions"],"import":["sessions"]},"notes":true}' ;;
  command) printf '{"argv":["/bin/true"],"ports":[]}' ;;
  *) exit 0 ;;
esac
"#;

/// A harness that keeps to the agent's folder's layout (`folders`), takes
/// messages from others (`from`) and prints its prompt (`prompt`), as
/// Link Harness does: Link gives it `run`, with background commands, and
/// `file`. Its prompt is its agent's standing instructions and a skill,
/// from the agent's own folder.
const TIDY: &str = r#"#!/bin/sh
# Its input copied out by the shell alone: it asks to run no program.
put() { while IFS= read -r l || [ -n "$l" ]; do printf '%s\n' "$l"; done; }
case "$1" in
  contract) printf '{"version":1,"proxy":false,"folders":true,"from":true,"prompt":true,"conversations":true,"state":{"export":["sessions"],"import":["sessions"]},"notes":true}' ;;
  conversations) echo moved > "$LNK_AGENT_FOLDER/chats/moved" || exit 1
    if (echo x > "$LNK_FILES/planted") 2>/dev/null; then echo HOME-WRITTEN; else echo HOME-READ-ONLY; fi ;;
  configure) put > "$HOME/configure.in" ;;
  instructions) put < "$LNK_AGENT_FOLDER/instructions/instruction.md"
    put < "$LNK_AGENT_FOLDER/skills/tides/SKILL.md" ;;
  command) printf '{"argv":["/bin/true"],"ports":[]}' ;;
  *) exit 0 ;;
esac
"#;

/// Link run and Link file, standing in: only defined, never called.
const TOOL: &str = "#!/bin/sh\nexit 0\n";

/// A fake systemd: each call kept in `systemctl.log` beside it, a unit
/// active while `active` is there too.
const SYSTEMCTL: &str = r#"#!/bin/sh
here=$(dirname "$0")
echo "$*" >> "$here/systemctl.log"
case "$*" in
  *is-active*) [ -f "$here/active" ] ;;
  *) exit 0 ;;
esac
"#;

struct World {
    root: TempDir,
    home: PathBuf,
    bin: PathBuf,
    core: PathBuf,
    sandboxed: bool,
}

impl World {
    fn new(test: &str) -> Self {
        Self::at(TempDir::new(&format!("grants-{test}")))
    }

    fn at(root: TempDir) -> Self {
        let (home, bin) = (root.join("home"), root.join("bin"));
        std::fs::create_dir_all(home.join("Link/Agents/main")).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        for (name, script) in [
            ("lnk-needy", NEEDY),
            ("lnk-plain", PLAIN),
            ("lnk-tidy", TIDY),
            ("lnk-run-tool", TOOL),
            ("lnk-file-tool", TOOL),
            ("lnk-scheduler-tool", TOOL),
            ("lnk-browser-tool", TOOL),
            ("lnk-desktop-tool", TOOL),
        ] {
            link_test_kit::write_program(&bin.join(name), script).unwrap();
        }
        let core = common::core_with_sandbox(&bin);
        World {
            root,
            home,
            bin,
            core,
            sandboxed: link_test_kit::gate::sandbox_with_landlock(),
        }
    }

    fn lnk(&self, args: &[&str]) -> Output {
        self.lnk_with(args, &[])
    }

    /// [`World::lnk`], with variables `vars` too.
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn lnk_with(&self, args: &[&str], vars: &[(&str, &Path)]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_lnk-agent"))
            .args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env(
                "XDG_RUNTIME_DIR",
                link_test_kit::env::made_runtime(&self.home),
            )
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("LNK_EXE", &self.core)
            .env("ANTHROPIC_API_KEY", "sk-ant-test-key")
            .envs(vars.iter().copied())
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// `lnk agent <args>` starting agent `name` in the foreground.
    fn start(&self, name: &str, args: &[&str]) -> Output {
        let files = self
            .home
            .join("Link/Agents")
            .join(name)
            .join("home")
            .display()
            .to_string();
        let mut args = args.to_vec();
        args.extend(["--foreground", "--run.files", &files]);
        if !self.sandboxed {
            args.push("--no-sandbox");
        }
        self.lnk(&args)
    }

    /// An explicit spec enabling the tools these grant checks exercise.
    fn everything_on(&self) -> String {
        let path = self.root.join("everything-on.toml");
        std::fs::write(&path, "schema = 1\nextends = \"link\"\n[tool.run]\nenabled = true\n[tool.file]\nenabled = true\n[tool.code]\nenabled = true\n").unwrap();
        let sandbox = self.home.join("Link/Safe/Agents/main/sandbox.toml");
        std::fs::create_dir_all(sandbox.parent().unwrap()).unwrap();
        std::fs::write(
            sandbox,
            link_agent_contract::agent_sandbox::AgentSandbox::link()
                .text("main")
                .unwrap(),
        )
        .unwrap();
        path.display().to_string()
    }

    /// Writes agent `agent`'s sandbox file before it's created: Link's
    /// default with no `[commands] allow`, so it runs only its
    /// toolchains' programs and a harness asking for more is asked.
    fn toolchains_only(&self, agent: &str) {
        let text = link_agent_contract::agent_sandbox::AgentSandbox::link()
            .text(agent)
            .unwrap();
        let mut doc: toml_edit::DocumentMut = text.parse().unwrap();
        let commands = doc["commands"].as_table_mut().unwrap();
        assert!(commands.remove("allow").is_some(), "{text}");
        let sandbox = self
            .home
            .join(format!("Link/Safe/Agents/{agent}/sandbox.toml"));
        std::fs::create_dir_all(sandbox.parent().unwrap()).unwrap();
        std::fs::write(sandbox, doc.to_string()).unwrap();
    }

    /// `lnk agent <args>` in a pseudo-terminal (`script`'s), typing
    /// `answer`.
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn in_terminal(&self, args: &[&str], answer: &str) -> Output {
        let harness = env!("CARGO_BIN_EXE_lnk-agent");
        let line = std::iter::once(format!("'{harness}'"))
            .chain(args.iter().map(|a| format!("'{a}'")))
            .collect::<Vec<_>>()
            .join(" ");
        let mut child = Command::new("script")
            .args(["-qec", &line, "/dev/null"])
            .env_clear()
            .env("HOME", &self.home)
            .env(
                "XDG_RUNTIME_DIR",
                link_test_kit::env::made_runtime(&self.home),
            )
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("LNK_EXE", &self.core)
            .env("ANTHROPIC_API_KEY", "sk-ant-test-key")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut child.stdin.take().unwrap(), answer.as_bytes()).unwrap();
        child.wait_with_output().unwrap()
    }

    /// What you grant agent `agent`, its file.
    fn sandbox(&self, agent: &str) -> PathBuf {
        self.home
            .join("Link/Safe/Agents")
            .join(agent)
            .join("sandbox.toml")
    }

    fn grants(&self, harness: &str) -> PathBuf {
        self.home
            .join(".lnk/safe/agents/harness-grants")
            .join(format!("{harness}.toml"))
    }

    fn needs(&self, json: &str) {
        std::fs::write(self.bin.join("needs.json"), json).unwrap();
    }

    /// Whether a test that needs the sandbox can't run here, as the
    /// bubblewrap gate said when the world was made.
    fn skips(&self) -> bool {
        !self.sandboxed
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn ok(output: &Output) -> String {
    let text = text(output);
    assert!(output.status.success(), "{text}");
    text
}

fn failed(output: &Output) -> String {
    let text = text(output);
    assert!(!output.status.success(), "{text}");
    text
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn status(world: &World, agent: &str) -> serde_json::Value {
    let output = world.lnk(&["-a", agent, "show", "--json"]);
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{}", text(&output)))
}

/// A tool defined here, by the start of its name (`run-`): its command.
fn defined(world: &World, tool: &str) -> Option<Vec<String>> {
    let tools = world.home.join(".lnk/safe/sandboxes/tools");
    let file = std::fs::read_dir(tools).ok()?.flatten().find(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        name.starts_with(tool) && name.ends_with(".json")
    })?;
    let json: serde_json::Value = serde_json::from_str(&read(&file.path())).unwrap();
    serde_json::from_value(json["argv"].clone()).ok()
}
