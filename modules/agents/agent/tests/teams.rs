//! `lnk agent team`: a team made with its agents, a member's purpose and
//! folders changed, one taken off, a team deleted; what each member's
//! sandbox file grants of the shared folder; the teams its harness is
//! told of; and `lnk agent check` reading a member's team's folder, not
//! another team's. With a fake harness program that keeps what it's
//! handed.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use link_test_kit::TempDir;

use crate::common;

/// Keeps what `configure` is handed in its home, and serves nothing.
const TOLD: &str = r#"#!/bin/sh
case "$1" in
  contract) printf '{"version":1,"proxy":false,"ports":0}' ;;
  configure) cat > "$HOME/told.json" ;;
  command) printf '{"argv":["/bin/true"],"ports":[]}' ;;
  *) exit 0 ;;
esac
"#;

/// A harness like [`TOLD`] that reads who a message is from, so it
/// honors the scheduler a member messages its teammates with.
const HEARD: &str = r#"#!/bin/sh
case "$1" in
  contract) printf '{"version":1,"proxy":false,"ports":0,"from":true}' ;;
  configure) cat > "$HOME/heard.json" ;;
  command) printf '{"argv":["/bin/true"],"ports":[]}' ;;
  *) exit 0 ;;
esac
"#;

struct World {
    _root: TempDir,
    home: PathBuf,
    bin: PathBuf,
    core: PathBuf,
    sandboxed: bool,
    ports: std::cell::RefCell<Vec<link_test_kit::HeldNumber>>,
}

impl World {
    fn new(test: &str) -> Self {
        let root = TempDir::new(&format!("teams-{test}"));
        let (home, bin) = (root.join("home"), root.join("bin"));
        std::fs::create_dir_all(home.join(".lnk")).unwrap();
        common::own_machine(&home);
        std::fs::create_dir_all(&bin).unwrap();
        link_test_kit::write_program(&bin.join("lnk-told"), TOLD).unwrap();
        link_test_kit::write_program(&bin.join("lnk-heard"), HEARD).unwrap();
        let core = common::core_with_sandbox(&bin);
        let sandboxed = link_test_kit::gate::sandbox_with_landlock();
        World {
            _root: root,
            home,
            bin,
            core,
            sandboxed,
            ports: Default::default(),
        }
    }

    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn lnk(&self, args: &[&str]) -> Output {
        Command::new(common::harness(&self.bin))
            .args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env(
                "XDG_RUNTIME_DIR",
                link_test_kit::env::made_runtime(&self.home),
            )
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("LNK_EXE", &self.core)
            .env("LNK_SERVICES", "off")
            .envs(link_test_kit::keychain::isolated(&self.home))
            .env("ANTHROPIC_API_KEY", "sk-ant-test-key")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// An agent of this team fixture, with a block held for its whole
    /// lifetime below Link's production allocation and system ports.
    fn add(&self, name: &str) {
        self.add_on(name, "told");
    }

    /// An agent of this fixture on harness `harness`.
    fn add_on(&self, name: &str, harness: &str) {
        let ports = link_test_kit::HeldNumber::first("ports", (9000..11000).step_by(2), |base| {
            (base..base + 2)
                .all(|port| std::net::TcpListener::bind(("127.0.0.1", port as u16)).is_ok())
        });
        common::write_agent(
            &self.home,
            name,
            &format!(
                "harness = \"{harness}\"\nport = {}\nblock = 2\n",
                ports.number()
            ),
        );
        self.ports.borrow_mut().push(ports);
        self.start(&["start", "--agent", name]);
    }

    /// Starts an agent in the foreground, in its default files folder.
    fn start(&self, args: &[&str]) -> String {
        let name = args[args.len() - 1];
        let files = self.home.join("Link/Agents").join(name).join("home");
        let files = files.display().to_string();
        let mut args = args.to_vec();
        args.extend(["--foreground", "--run.files", &files]);
        if !self.sandboxed {
            args.push("--no-sandbox");
        }
        ok(&self.lnk(&args))
    }

    fn config(&self, path: &str) -> String {
        read(
            &link_files::layout::Home::TeamSpecs
                .at(&self.home.join("Link"))
                .join(path),
        )
    }

    fn teams(&self) -> PathBuf {
        self.home.join("Link/Teams")
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

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn id_of(world: &World, agent: &str) -> String {
    let identity = crate::common::record(&world.home, agent, "identity.toml");
    identity["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{identity:?}"))
        .to_string()
}

/// Whether agent `agent`'s spec turns its scheduler on.
fn scheduler_on(world: &World, agent: &str) -> bool {
    let spec: toml::Table = toml::from_str(&read(
        &world
            .home
            .join(format!("Link/Safe/Agents/{agent}/agent.toml")),
    ))
    .unwrap_or_default();
    spec.get("behavior")
        .and_then(|b| b.get("tool"))
        .and_then(|t| t.get("scheduler"))
        .and_then(|s| s.get("enabled"))
        .and_then(toml::Value::as_bool)
        == Some(true)
}

/// A member messages its teammates through its scheduler, off for a new
/// agent: putting one whose harness honors it on a team turns it on and
/// says so, once; one whose harness doesn't keeps it off.
#[test]
fn a_member_gets_its_scheduler_for_its_teammates() {
    let world = World::new("scheduler");
    world.add_on("scout", "heard");
    world.add_on("writer", "heard");
    world.add("outsider");
    for agent in ["scout", "writer", "outsider"] {
        assert!(!scheduler_on(&world, agent));
    }
    let made = ok(&world.lnk(&[
        "team", "create", "research", "--with", "scout", "--with", "writer", "--with", "outsider",
    ]));
    assert!(
        made.contains("its scheduler on for messages to its teammates"),
        "{made}"
    );
    assert!(scheduler_on(&world, "scout") && scheduler_on(&world, "writer"));
    assert!(!scheduler_on(&world, "outsider"));
    // On it already: a second team changes nothing more about it.
    let again = ok(&world.lnk(&["team", "create", "press", "--with", "scout"]));
    assert!(!again.contains("its scheduler on"), "{again}");
}

#[test]
fn a_team_is_made_changed_and_reached_as_granted() {
    let world = World::new("made");
    for agent in ["scout", "writer", "outsider"] {
        world.add(agent);
    }
    let (scout, writer) = (id_of(&world, "scout"), id_of(&world, "writer"));

    // Made with two agents, each writing the whole shared folder, as
    // `--write` grants them.
    let made = ok(&world.lnk(&[
        "team", "create", "research", "--with", "scout", "--with", "writer", "--write", ".",
    ]));
    assert!(
        made.contains("Team research is made, sharing ~/Link/Teams/research/shared."),
        "{made}"
    );
    let file: toml::Table = toml::from_str(&world.config("research.toml")).unwrap();
    assert_eq!(file["member"]["scout"]["id"].as_str(), Some(scout.as_str()));
    assert_eq!(
        file["member"]["writer"]["id"].as_str(),
        Some(writer.as_str())
    );
    assert_eq!(
        file["files"]["shared"].as_str(),
        Some("~/Link/Teams/research/shared")
    );
    assert!(world.teams().join("research/shared").is_dir());
    let grants = read(&world.home.join("Link/Safe/Agents/scout/sandbox.toml"));
    assert!(
        grants.contains("[teams.research]\nwrite = [\".\"]"),
        "{grants}"
    );
    assert!(failed(&world.lnk(&["team", "create", "research"])).contains("already"));
    // One folder written and read is refused: writing includes reading.
    let both = failed(&world.lnk(&[
        "team", "create", "press", "--with", "outsider", "--write", "notes", "--read", "notes",
    ]));
    assert!(
        both.contains("writing a folder includes reading it"),
        "{both}"
    );
    assert!(world.config("press.toml").is_empty());
    // A team spec's field flags without its file would be dropped.
    let flagged = &[
        "team",
        "create",
        "press",
        "--with",
        "scout",
        "--files.shared",
        "/srv/press",
    ];
    assert!(failed(&world.lnk(flagged)).contains("--spec"));

    // A team, and one member of it, as typed blocks: the diagnosis of a
    // `member`; an agent by name the same for an `agent`.
    for args in [
        &["team", "show", "research", "--json"][..],
        &["team", "show", "research", "--member", "scout", "--json"],
        &["show", "scout", "--json"],
    ] {
        let said: serde_json::Value = serde_json::from_str(&ok(&world.lnk(args))).unwrap();
        assert!(said["blocks"].is_array(), "{args:?}: {said}");
    }
    let member = ok(&world.lnk(&["team", "show", "research", "--member", "scout"]));
    assert!(
        member.contains("scout") && !member.contains("writer"),
        "{member}"
    );
    let refused = failed(&world.lnk(&["team", "show", "research", "--member", "outsider"]));
    assert!(refused.contains("no member"), "{refused}");
    assert!(failed(&world.lnk(&["team", "show", "nowhere"])).contains("no team"));

    // The writer's purpose and folders, changed: it writes drafts/ and reads
    // sources/, nothing else of it.
    ok(&world.lnk(&[
        "team",
        "add",
        "research",
        "--member",
        "writer",
        "--purpose",
        "drafts the report",
        "--write",
        "drafts",
        "--read",
        "sources",
    ]));
    let file: toml::Table = toml::from_str(&world.config("research.toml")).unwrap();
    assert_eq!(
        file["member"]["writer"]["purpose"].as_str(),
        Some("drafts the report")
    );
    let grants = read(&world.home.join("Link/Safe/Agents/writer/sandbox.toml"));
    let granted: toml::Table = toml::from_str(&grants).unwrap();
    assert_eq!(
        granted["teams"]["research"],
        toml::Value::Table(toml::from_str("write = [\"drafts\"]\nread = [\"sources\"]").unwrap()),
        "{grants}"
    );
    assert!(world.teams().join("research/shared/drafts").is_dir());
    for bad in [
        &[
            "team", "add", "research", "--member", "writer", "--write", "../x",
        ][..],
        &["team", "add", "research", "--member", "nobody"],
        &["team", "add", "nowhere", "--member", "writer"],
        &[
            "team", "add", "research", "--member", "outsider", "--as", "writer",
        ],
        &[
            "team", "add", "research", "--member", "writer", "--role", "drafts",
        ],
    ] {
        assert!(!world.lnk(bad).status.success(), "{bad:?}");
    }

    // Another team, the outsider's, named with no folder flag: on it, and
    // granted none of its folder.
    ok(&world.lnk(&["team", "create", "press", "--with", "outsider"]));
    let grants = read(&world.home.join("Link/Safe/Agents/outsider/sandbox.toml"));
    assert!(!grants.contains("press"), "{grants}");
    let list = ok(&world.lnk(&["team", "list", "--json"]));
    let teams: Vec<serde_json::Value> = list
        .lines()
        .filter(|l| l.starts_with('{'))
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    teams.iter().for_each(link_test_kit::ids::assert_named);
    assert_eq!(teams.len(), 2);
    assert_eq!(teams[1]["name"], "research");
    let writer_listed = &teams[1]["members"][1];
    assert_eq!(writer_listed["agent"], "writer");
    assert_eq!(
        writer_listed["workload"].as_str(),
        Some(format!("agent:{writer}").as_str())
    );
    assert_eq!(writer_listed["purpose"], "drafts the report");
    assert_eq!(writer_listed["write"], serde_json::json!(["drafts"]));
    let text = ok(&world.lnk(&["team", "list"]));
    assert!(
        text.contains("drafts the report")
            && text.contains("stopped")
            && text.contains("writes drafts/, reads sources/"),
        "{text}"
    );
    // Each agent's teams, by their IDs, as `lnk measure` reads them.
    let listed: serde_json::Value =
        serde_json::from_slice(&world.lnk(&["list", "--json"]).stdout).unwrap();
    link_test_kit::ids::assert_named(&listed);
    let teams_of = |name: &str| {
        listed["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == name)
            .unwrap()["teams"]
            .clone()
    };
    let resource = |team: &serde_json::Value| serde_json::json!([{"resource": team["resource"]}]);
    assert!(teams[1]["resource"].as_str().unwrap().starts_with("group:"));
    assert_eq!(teams_of("scout"), resource(&teams[1]));
    assert_eq!(teams_of("outsider"), resource(&teams[0]));

    // What its harness is told at its next start: its team, and who else
    // is on it.
    world.start(&["start", "-a", "scout"]);
    let told: serde_json::Value = serde_json::from_str(&read(
        &world
            .home
            .join(".lnk/data/agents/scout/harnesses/told/told.json"),
    ))
    .unwrap();
    let place = &told["environment"]["teams"][0];
    assert_eq!(place["name"], "research");
    assert_eq!(place["member"], "scout");
    assert_eq!(place["teammates"][0]["name"], "writer");
    assert_eq!(place["teammates"][0]["id"], writer.as_str());
    assert_eq!(place["teammates"][0]["purpose"], "drafts the report");
    assert_eq!(
        told["environment"]["folders"]["teams"]["research"]["write"],
        serde_json::json!(["."])
    );

    // Its sandbox reads its team's folder, and not another team's.
    if world.sandboxed {
        let check = ok(&world.lnk(&["-a", "scout", "harness", "check"]));
        assert!(
            check.contains("read: ~/Link/Teams/research/shared"),
            "{check}"
        );
        assert!(!check.to_lowercase().contains("missed"), "{check}");
        assert!(
            check.contains("blocked  read what isn't its own: ~/Link/Teams/press/shared"),
            "{check}"
        );
        assert!(
            check.contains("blocked  read what's hidden: ~/Link/Teams"),
            "{check}"
        );
        let check = ok(&world.lnk(&["-a", "writer", "harness", "check"]));
        assert!(
            check.contains("read: ~/Link/Teams/research/shared/drafts"),
            "{check}"
        );
        assert!(
            check.contains("read: ~/Link/Teams/research/shared/sources"),
            "{check}"
        );
    }

    // Off the team: its grant goes too.
    ok(&world.lnk(&["team", "remove", "research", "--member", "writer"]));
    assert!(!world.config("research.toml").contains(&writer));
    assert!(!read(&world.home.join("Link/Safe/Agents/writer/sandbox.toml")).contains("research"));
    assert!(
        failed(&world.lnk(&["team", "remove", "research", "--member", "writer"]))
            .contains("isn't on team")
    );

    // An agent deleted is off its teams.
    ok(&world.lnk(&["team", "add", "research", "--member", "outsider"]));
    let deleted = ok(&world.lnk(&["delete", "outsider", "--yes"]));
    assert!(deleted.contains("off team press and research"), "{deleted}");
    assert!(!world.config("research.toml").contains("outsider"));
    let press: toml::Table = toml::from_str(&world.config("press.toml")).unwrap();
    assert!(!press.contains_key("member"), "{press}");
    ok(&world.lnk(&["team", "show", "press"]));
    ok(&world.lnk(&["team", "add", "press", "--member", "scout"]));

    // Deleted: its members' grants go, its folder stays.
    ok(&world.lnk(&["team", "delete", "press", "--yes"]));
    assert!(world.config("press.toml").is_empty());
    assert!(!read(&world.home.join("Link/Safe/Agents/scout/sandbox.toml")).contains("press"));
    assert!(world.teams().join("press/shared").is_dir());
}

/// `lnk agent delete` takes the agent off its teams, and its running
/// teammates take that at a start of their own: one in a terminal is told
/// so, as when a member is taken off with `team remove`.
#[test]
fn a_removed_agent_leaves_its_teams_and_its_teammates_are_told() {
    let world = World::new("removed");
    for agent in ["scout", "writer"] {
        world.add(agent);
    }
    ok(&world.lnk(&[
        "team", "create", "research", "--with", "scout", "--with", "writer",
    ]));
    // The writer runs in a terminal: its lock held, as a start holds it.
    let dir = world.home.join(".lnk/safe/agents/writer");
    std::fs::create_dir_all(&dir).unwrap();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("foreground.lock"))
        .unwrap();
    lock.try_lock().unwrap();

    let removed = ok(&world.lnk(&["delete", "scout", "--yes"]));
    assert!(
        removed.contains("scout is off team research now."),
        "{removed}"
    );
    assert!(
        removed.contains("writer runs in a terminal, so it takes the team's change"),
        "{removed}"
    );
    let file: toml::Table = toml::from_str(&world.config("research.toml")).unwrap();
    assert!(file["member"].get("scout").is_none());
    drop(lock);
}

/// Both streams of `output`, whether it succeeded or not.
fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// A team's memory is off until it's turned on: a member's start
/// installs no engine for it. Turned on, its engine, when it isn't on the
/// machine, is said before it's installed: nobody on the team chose it.
/// An install that fails leaves it off.
#[test]
fn a_teams_memory_is_off_until_turned_on() {
    let world = World::new("memory");
    world.add("scout");
    ok(&world.lnk(&[
        "team", "create", "research", "--with", "scout", "--write", ".",
    ]));
    let installs = "Team research's memory is kept by its engine lnk-memory-tool, which isn't \
                    on this machine: installing it from Link's release.";
    let started = world.start(&["start", "--agent", "scout"]);
    assert!(!started.contains(installs), "{started}");
    assert!(!started.contains("research isn't served"), "{started}");

    // The fixture's lnk adds no module, so the engine's install fails, and
    // the team's file is put back as it was: still off.
    let turned = world.lnk(&["team-memory", "start", "research"]);
    let text = said(&turned);
    assert!(
        !turned.status.success() && text.contains(installs),
        "{text}"
    );
    let file: toml::Table = toml::from_str(&world.config("research.toml")).unwrap();
    assert!(file.get("settings").is_none(), "{file}");
    let off = ok(&world.lnk(&["team-memory", "stop", "research"]));
    assert!(off.contains("off already"), "{off}");

    // A team with no folder has none to keep a memory of.
    ok(&world.lnk(&["team", "create", "solo", "--no-folder"]));
    let refused = failed(&world.lnk(&["team-memory", "start", "solo"]));
    assert!(refused.contains("no shared folder"), "{refused}");
    let flagged = failed(&world.lnk(&["team", "create", "duo", "--no-folder", "--write", "."]));
    assert!(flagged.contains("--write"), "{flagged}");
}

/// A team's ID is reserved in its ledger, as an agent's is: kept live
/// while the team is here, retired by `lnk agent team delete`, and never
/// given to a team made again with its name.
#[test]
fn a_deleted_teams_id_is_never_given_again() {
    let world = World::new("team-ids");
    let id = |world: &World| {
        let file: toml::Table = toml::from_str(&world.config("solo.toml")).unwrap();
        file["id"].as_str().unwrap().to_string()
    };
    let ledger = |world: &World| read(&world.home.join(".lnk/safe/agents/group-ids.toml"));
    ok(&world.lnk(&["team", "create", "solo", "--no-folder"]));
    let first = id(&world);
    assert!(ledger(&world).contains(&first), "{}", ledger(&world));
    ok(&world.lnk(&["team", "delete", "solo", "--yes"]));
    let kept: toml::Table = toml::from_str(&ledger(&world)).unwrap();
    let retired = kept["retired"].as_array().unwrap();
    assert!(
        retired.iter().any(|r| r.as_str() == Some(first.as_str())),
        "{kept}"
    );
    ok(&world.lnk(&["team", "create", "solo", "--no-folder"]));
    assert_ne!(id(&world), first);
}
