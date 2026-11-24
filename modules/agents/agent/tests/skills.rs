//! `lnk agent skill add`, `list` and `remove`, run as you would: a skill copied into an
//! agent's files, named as its SKILL.md says, with a fake harness
//! adapter that doesn't read `skills/`.

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use link_test_kit::TempDir;

use crate::common;

/// A harness that runs nothing, and takes none of Link's instructions;
/// its port the first of its agent's block, which it never serves.
const ADAPTER: &str = r#"#!/bin/sh
case "$1" in
  command) printf '{"argv":["/bin/true"],"ports":[%s],"serve":[]}' "$LNK_PORT" ;;
  *) exit 0 ;;
esac
"#;

struct World {
    root: TempDir,
    home: PathBuf,
    bin: PathBuf,
    core: PathBuf,
}

impl World {
    fn new(test: &str) -> Self {
        let root = TempDir::new(&format!("skills-{test}"));
        let (home, bin) = (root.join("home"), root.join("bin"));
        std::fs::create_dir_all(home.join(".lnk")).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        common::write_adapter(&bin.join("lnk-fake"), ADAPTER).unwrap();
        let core = common::core_with_sandbox(&bin);
        World {
            root,
            home,
            bin,
            core,
        }
    }

    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn lnk(&self, args: &[&str]) -> Output {
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
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn files(&self) -> PathBuf {
        self.home.join("Link/Agents/main/home")
    }

    /// The agent's own folder, where its skills are.
    fn own(&self) -> PathBuf {
        self.home.join("Link/Agents/main/agent")
    }
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

fn failed(output: &Output) -> String {
    assert!(!output.status.success());
    String::from_utf8_lossy(&output.stderr).to_string()
}

/// `lnk agent skill add` copies a skill into the agent's files, named as
/// its SKILL.md says, and says a harness that doesn't read `skills/`
/// won't see it.
#[test]
fn a_skill_is_added_to_the_agents_files() {
    let world = World::new("add");
    let files = world.files().display().to_string();
    ok(&world.lnk(&[
        "create",
        "main",
        "--harness",
        "fake",
        "--foreground",
        "--no-sandbox",
        "--run.files",
        &files,
    ]));
    let download = world.root.join("downloads/tide-tables");
    std::fs::create_dir_all(&download).unwrap();
    std::fs::write(
        download.join("SKILL.md"),
        "---\nname: tides\ndescription: Read a tide table for sailing.\n---\n# Tides\n",
    )
    .unwrap();
    let source = download.display().to_string();
    let said = ok(&world.lnk(&["skill", "add", &source]));
    assert!(
        said.contains("Added skill tides in ~/Link/Agents/main/agent/skills/tides: Read a tide"),
        "{said}"
    );
    assert!(said.contains("fake doesn't read skills/"), "{said}");
    let copied = std::fs::read_to_string(world.own().join("skills/tides/SKILL.md")).unwrap();
    assert!(copied.contains("# Tides"), "{copied}");
    // Once: again only with --replace.
    assert!(failed(&world.lnk(&["skill", "add", &source])).contains("--replace"));
    let said = ok(&world.lnk(&["skill", "add", &source, "--replace"]));
    assert!(said.contains("Replaced skill tides"), "{said}");
    // What isn't a skill: refused, saying why.
    let refused = failed(&world.lnk(&["skill", "add", &world.root.display().to_string()]));
    assert!(refused.contains("holds no SKILL.md"), "{refused}");

    // Listed by name, and removed by it.
    let listed = ok(&world.lnk(&["skill", "list"]));
    assert!(
        listed.contains("tides") && listed.contains("Read a tide table"),
        "{listed}"
    );
    let removed = ok(&world.lnk(&["skill", "remove", "tides"]));
    assert!(removed.contains("Removed skill tides"), "{removed}");
    assert!(!world.own().join("skills/tides").exists());
    assert!(ok(&world.lnk(&["skill", "list"])).contains("There are no skills yet"));
    assert!(failed(&world.lnk(&["skill", "remove", "tides"])).contains("no skill tides"));
    assert!(failed(&world.lnk(&["skill", "remove", "../Home"])).contains("isn't a skill's name"));
}
