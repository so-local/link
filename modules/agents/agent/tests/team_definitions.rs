//! Applying a desired roster is resumable and never replaces an agent ID.
use link_test_kit::{TempDir, env, gate};
use std::{path::PathBuf, process::Output};

const DEFINITION: &str = r#"# Keep this roster
schema = 1
name = "research"
lead = "scout"
[member.scout.agent.turns]
rounds = 12
[member.writer.agent.model]
default = "test/model"
[files]
shared = "~/Link/Teams/research/shared"
[settings.agent.turns]
calls = 8
"#;

struct Fixture {
    home: TempDir,
    spec: PathBuf,
    bin: PathBuf,
}
impl Fixture {
    fn new(test: &str) -> Option<Self> {
        let home = TempDir::new(test);
        let spec = home.join("team.toml");
        link_files::write_private(&spec, DEFINITION.as_bytes()).unwrap();
        // The production reader owns this prerequisite; do not relax it
        // for a CLI fixture whose ancestor ownership the host cannot supply.
        if let Err(err) = link_spec::read::<toml::Table>(&spec) {
            gate::lacks("trusted spec filesystem", err, None);
            return None;
        }
        if let Err(err) =
            link_files::settings::read::<toml::Table>(&home.join(".lnk/safe/installation.toml"))
        {
            gate::lacks("trusted settings filesystem", err, None);
            return None;
        }
        let bin = home.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        // The core answers the release's history, empty here, which a
        // spec's names follow; it refuses everything else.
        link_test_kit::write_core(
            &bin.join("lnk"),
            "#!/bin/sh\necho 'fixture refuses installing external modules' >&2\nexit 1\n",
        )
        .unwrap();
        Some(Self { home, spec, bin })
    }
    fn run(&self, args: &[&str]) -> Output {
        let args = ["team"]
            .into_iter()
            .chain(args.iter().copied())
            .collect::<Vec<_>>();
        self.run_agent(&args)
    }
    fn run_agent(&self, args: &[&str]) -> Output {
        env::command(env!("CARGO_BIN_EXE_lnk-agent"), &self.home)
            .args(args)
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("LNK_EXE", self.bin.join("lnk"))
            .env("LNK_SERVICES", "off")
            .output()
            .unwrap()
    }
    fn apply(&self) -> Output {
        self.run(&["create", "--spec", self.spec.to_str().unwrap()])
    }
    fn kept(&self) -> PathBuf {
        link_files::layout::Home::TeamSpecs
            .at(&self.home.join("Link"))
            .join("research.toml")
    }
    fn agent(&self, name: &str) -> PathBuf {
        self.home
            .join(format!("Link/Safe/Agents/{name}/agent.toml"))
    }
}

fn success(output: Output) -> String {
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn a_roster_applies_repeats_and_stands_down_without_losing_identity_or_files() {
    let Some(fixture) = Fixture::new("team-definition-lifecycle") else {
        return;
    };
    let first = success(fixture.apply());
    let first = link_agent_spec::parse_team(&first).unwrap();
    assert_eq!(first.lead.as_deref(), Some("scout"));
    let scout = fixture.agent("scout");
    assert!(scout.is_file(), "{}", scout.display());
    let identity = crate::common::record(&fixture.home, "scout", "identity.toml");
    assert_eq!(
        identity["id"].as_str(),
        Some(first.members["scout"].id.as_str())
    );
    let effective = success(fixture.run_agent(&["spec", "show", "scout"]));
    assert!(effective.contains("rounds = 12"), "{effective}");
    // The team's spec, by its name, is the file it was made from.
    let team = success(fixture.run(&["spec", "show", "research"]));
    assert!(team.contains("lead = \"scout\""), "{team}");
    assert!(
        std::fs::read_to_string(&fixture.spec)
            .unwrap()
            .contains("# Keep this roster")
    );
    let report = fixture.home.join("Link/Teams/research/shared/report.md");
    link_files::write_private(&report, b"work survives standing down").unwrap();
    let second = link_agent_spec::parse_team(&success(fixture.apply())).unwrap();
    assert_eq!(first, second);
    success(fixture.run(&["stop", "research"]));
    assert_eq!(
        std::fs::read_to_string(report).unwrap(),
        "work survives standing down"
    );
    assert!(scout.exists() && fixture.agent("writer").exists() && fixture.kept().exists());
    let kept = std::fs::read_to_string(fixture.kept()).unwrap();
    assert_eq!(link_agent_spec::parse_team(&kept).unwrap(), first);
}

#[test]
fn a_collision_is_refused_before_publishing_and_partial_apply_reuses_ids() {
    let Some(fixture) = Fixture::new("team-definition-recovery") else {
        return;
    };
    link_agent_contract::testing::write_agent(&fixture.home, "scout", "harness = 'link'\n");
    let original = std::fs::read(fixture.agent("scout")).unwrap();
    assert!(!fixture.apply().status.success());
    assert!(!fixture.kept().exists());
    assert_eq!(std::fs::read(fixture.agent("scout")).unwrap(), original);

    let Some(retry) = Fixture::new("team-definition-retry") else {
        return;
    };
    let failing = DEFINITION.replace("[member.writer.agent.model]", "[member.writer.agent]\nharness = { name = 'unavailable-fixture' }\n[member.writer.agent.model]");
    link_files::write_private(&retry.spec, failing.as_bytes()).unwrap();
    assert!(!retry.apply().status.success());
    let saved =
        link_agent_spec::parse_team(&std::fs::read_to_string(retry.kept()).unwrap()).unwrap();
    assert!(retry.agent("scout").exists());
    link_files::write_private(&retry.spec, DEFINITION.as_bytes()).unwrap();
    let resumed = link_agent_spec::parse_team(&success(retry.apply())).unwrap();
    assert_eq!(resumed.id, saved.id);
    for (name, member) in saved.members {
        assert_eq!(resumed.members[&name].id, member.id);
    }
}

/// A team ID a definition gives is reserved as a new one is: refused while
/// the ledger holds it, as for a team moved to a hosted account.
#[test]
fn a_given_team_id_the_ledger_holds_is_refused() {
    let Some(fixture) = Fixture::new("team-definition-held-id") else {
        return;
    };
    let given = DEFINITION.replacen("schema = 1\n", "schema = 1\nid = \"t1e2a3m4i5d6\"\n", 1);
    link_files::write_private(&fixture.spec, given.as_bytes()).unwrap();
    link_files::write_private(
        &fixture.home.join(".lnk/safe/agents/team-ids.toml"),
        b"schema = 1\nlive = [\"t1e2a3m4i5d6\"]\n",
    )
    .unwrap();
    let refused = fixture.apply();
    assert!(!refused.status.success(), "{refused:?}");
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(said.contains("exists"), "{said}");
    assert!(!fixture.kept().exists());
    // Claimed once its identity is saved, the refused claim takes the
    // identity with it: once the ledger lets it go, a retry takes the ID.
    assert_eq!(identities(&fixture.home), Vec::<PathBuf>::new());
    link_files::write_private(
        &fixture.home.join(".lnk/safe/agents/team-ids.toml"),
        b"schema = 1\n",
    )
    .unwrap();
    let applied = link_agent_spec::parse_team(&success(fixture.apply())).unwrap();
    assert_eq!(applied.id, "t1e2a3m4i5d6");
}

/// A team ID the team's kept definition names is checked as its
/// identity's is: refused once it's retired here.
#[test]
fn a_kept_definitions_retired_team_id_is_refused() {
    let Some(fixture) = Fixture::new("team-definition-kept-id") else {
        return;
    };
    let given = DEFINITION.replacen("schema = 1\n", "schema = 1\nid = \"t1e2a3m4i5d6\"\n", 1);
    link_files::write_private(&fixture.kept(), given.as_bytes()).unwrap();
    link_files::write_private(
        &fixture.home.join(".lnk/safe/agents/team-ids.toml"),
        b"schema = 1\nretired = [\"t1e2a3m4i5d6\"]\n",
    )
    .unwrap();
    let refused = fixture.apply();
    assert!(!refused.status.success(), "{refused:?}");
    assert_eq!(identities(&fixture.home), Vec::<PathBuf>::new());
}

/// Every team identity saved under `home`.
fn identities(home: &std::path::Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![home.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else if path
                .file_name()
                .is_some_and(|name| name == "definition.toml")
            {
                found.push(path);
            }
        }
    }
    found
}
