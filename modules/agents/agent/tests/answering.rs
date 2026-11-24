//! `lnk-agent --link-answer <question>`: the questions of the modules below this
//! one, asked of its program directly as they ask it, each in its
//! envelope, its other modules a stand-in.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use link_test_kit::TempDir;

/// A machine whose `lnk` is a stand-in: your boxes, each box's `lnk`, who
/// is signed in, and this module's own list and status.
struct World {
    dir: TempDir,
}

/// The version a question's own protocol declares current, from the
/// contract that declares it: each question is versioned on its own.
fn current(question: &str) -> u32 {
    let removed = link_core::answers::Question::<(), ()>::new(
        link_box_contract::REMOVED,
        link_box_contract::REMOVED_PROTOCOL,
    );
    let declared = [
        (
            link_secret_contract::COPIES.name,
            link_secret_contract::COPIES.protocol(),
        ),
        (
            link_secret_contract::GRANTEE.name,
            link_secret_contract::GRANTEE.protocol(),
        ),
        (removed.name, removed.protocol()),
    ];
    declared
        .into_iter()
        .find(|(name, _)| *name == question)
        .map_or(1, |(_, protocol)| protocol.current)
}

impl World {
    fn new(test: &str) -> World {
        let dir = TempDir::new(&format!("answering-{test}"));
        std::fs::create_dir_all(dir.join("home")).unwrap();
        link_test_kit::write_core(
            &dir.join("lnk"),
            r#"#!/bin/sh
here=$(dirname "$0")
echo "$*" >> "$here/lnk.log"
case "$*" in
  # Your boxes: aws-1 up, gcp-1 stopped, bare-1 without the secret
  # module, new-1 made again under its name, old-1 gone.
  "box list --json") echo '[{"name": "aws-1"}, {"name": "gcp-1"}, {"name": "bare-1"}, {"name": "new-1", "resource": "box:n3w4m5c6h7d8"}]' ;;
  "box run aws-1 -- secret delete --here "*) echo removed ;;
  "box run gcp-1 -- secret delete --here "*)
    echo "gcp-1 is stopped: lnk box start gcp-1" >&2; exit 1 ;;
  "box run bare-1 -- secret delete "*)
    # lnk, saying its module isn't installed, as it exits for that alone.
    echo "\`lnk secret\` is part of the secret module (lnk-secret), which isn't installed" >&2; exit 4 ;;
  "module list --json") echo '[{"name": "login", "installed": true, "answers": ["lnk.account"]}]' ;;
  # The key a box was given for an agent's buckets: revoked, but one's.
  "storage revoke --owner agent:a1b2c3d4e5f6") exit 0 ;;
  "storage revoke --owner agent:b2c3d4e5f6a1") echo "AccessDenied" >&2; exit 1 ;;
  # This module's list, where a test writes one.
  "agent list --json") [ -x "$here/agent-list" ] && exec "$here/agent-list" || exit 1 ;;
  *) exit 1 ;;
esac
"#,
        )
        .unwrap();
        // The login module beside it: who's signed in, as the test says.
        link_test_kit::write_program(
            &dir.join("lnk-login"),
            r#"#!/bin/sh
here=$(dirname "$0")
[ "$*" = "--link-answer lnk.account" ] || exit 1
cat > /dev/null
[ -e "$here/whoami.json" ] || exit 1
printf '{"protocol": 1, "answer": '; cat "$here/whoami.json"; echo '}'
"#,
        )
        .unwrap();
        World { dir }
    }

    /// `question` asked with `input`, in its envelope at the version its
    /// own protocol declares current: its answer, out of the envelope that
    /// says that version, or what it said on stderr.
    fn ask(&self, question: &str, input: serde_json::Value) -> Result<serde_json::Value, String> {
        let version = current(question);
        let asked = serde_json::json!({"protocol": version, "question": input});
        let answered = self.ask_raw(question, asked)?;
        assert_eq!(answered["protocol"], version, "{answered}");
        Ok(answered["answer"].clone())
    }

    /// `question` asked with `asked`, as it's sent: what it answers, as
    /// it's sent, or what it said on stderr.
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn ask_raw(
        &self,
        question: &str,
        asked: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_lnk-agent"))
            .args(["--link-answer", question])
            .env_clear()
            .env("HOME", self.dir.join("home"))
            .env(
                "XDG_RUNTIME_DIR",
                link_test_kit::env::made_runtime(&self.dir.join("home")),
            )
            .env("PATH", "/usr/bin:/bin")
            .env("LNK_EXE", self.dir.join("lnk"))
            .env("LNK_SERVICES", "off")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(asked.to_string().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        match out.status.success() {
            true => Ok(serde_json::from_slice(&out.stdout).unwrap()),
            false => Err(String::from_utf8_lossy(&out.stderr).into_owned()),
        }
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.join("lnk.log")).unwrap_or_default()
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

/// An agent's key a move gave to boxes and a cloud account: each box it
/// can reach, removed there as that agent's own key; a box that's no
/// longer yours, or one made again under its name, gone with it and never
/// touched; a box it can't reach, why; the account's, held with the agent.
#[test]
fn a_secrets_copies_are_reached_on_each_box() {
    let world = World::new("copies");
    let owner = serde_json::json!({"principal": "service:s1s2s3s4s5s6", "as": {"kind": "own", "purpose": "model"}});
    let copy = |place: serde_json::Value| serde_json::json!({"owner": owner, "place": place});
    let reached = world
        .ask(
            "secret.copies",
            serde_json::json!({"copies": [
                copy(serde_json::json!({"name": "aws-1"})),
                copy(serde_json::json!({"name": "bare-1"})),
                copy(serde_json::json!({"name": "gcp-1"})),
                copy(serde_json::json!({"name": "new-1", "resource": "box:o1dm2c3h4i5d"})),
                copy(serde_json::json!({"name": "old-1"})),
                copy(serde_json::json!({"cloud": true, "name": "cloud-1"})),
            ]}),
        )
        .unwrap();
    assert_eq!(
        reached,
        serde_json::json!([
            "removed",
            {"unreached": "it has no secret module: lnk box run bare-1 -- module add secret"},
            {"unreached": "gcp-1 is stopped: lnk box start gcp-1"},
            "removed",
            "removed",
            "with_service",
        ])
    );
    let log = world.log();
    assert!(
        log.contains(
            "box run aws-1 -- secret delete --here --service service:s1s2s3s4s5s6 --own model"
        ),
        "{log}"
    );
    assert!(!log.contains("box run new-1"), "{log}");
    assert!(!log.contains("box run old-1"), "{log}");
    // Your boxes asked once, for every copy.
    assert_eq!(log.matches("box list").count(), 1, "{log}");
}

/// A key a move gave to a cloud account that hosts agents: held there
/// with the agent, and gone only once the account, asked for the agents
/// of the owner it went there as, answers without it. Never on another's
/// list, signed out, or a reply that doesn't say.
#[test]
fn an_accounts_copy_goes_once_its_owners_list_lacks_the_agent() {
    let world = World::new("account");
    link_test_kit::write_program(
        &world.path("agent-list"),
        r#"#!/bin/sh
here=$(dirname "$0")
host='{"name": "host", "cloud": true}'
[ -e "$here/silent" ] && { echo "{\"agents\": [], \"unreached\": [{\"resource\": null, \"name\": \"host\", \"cloud\": true, \"error\": \"it answered without its agents\"}], \"answered\": []}"; exit 0; }
[ -e "$here/main-removed" ] && { echo "{\"agents\": [], \"unreached\": [], \"answered\": [$host]}"; exit 0; }
echo "{\"agents\": [{\"workload\": \"agent:a1b2c3d4e5f6\", \"principal\": \"service:s1s2s3s4s5s6\", \"name\": \"main\", \"machine\": {\"name\": \"host\", \"cloud\": true}}], \"unreached\": [], \"answered\": [$host]}"
"#,
    )
    .unwrap();
    let reach = || {
        world
            .ask(
                "secret.copies",
                serde_json::json!({"copies": [{
                    "owner": {"principal": "service:s1s2s3s4s5s6", "as": {"kind": "own", "purpose": "model"}},
                    "place": {"cloud": true, "name": "host", "owner": "github-1"},
                }]}),
            )
            .unwrap()
    };
    let whoami = |owner: Option<&str>| match owner {
        Some(owner) => std::fs::write(
            world.path("whoami.json"),
            format!(r#"{{"owner": "{owner}"}}"#),
        )
        .unwrap(),
        None => {
            let _ = std::fs::remove_file(world.path("whoami.json"));
        }
    };
    let held = serde_json::json!(["with_service"]);
    whoami(Some("github-1"));
    assert_eq!(reach(), held);
    std::fs::write(world.path("main-removed"), "").unwrap();
    whoami(Some("github-2"));
    assert_eq!(reach(), held);
    whoami(None);
    assert_eq!(reach(), held);
    whoami(Some("github-1"));
    std::fs::write(world.path("silent"), "").unwrap();
    assert_eq!(reach(), held);
    std::fs::remove_file(world.path("silent")).unwrap();
    assert_eq!(reach(), serde_json::json!(["removed"]));
}

/// Where an agent is, by its name: here by its principal, here unstarted,
/// on a box, nowhere, or of a kind it doesn't keep.
#[test]
fn where_an_agent_is() {
    let world = World::new("agent");
    link_test_kit::write_program(
        &world.path("agent-list"),
        r#"#!/bin/sh
echo '{"agents": [{"workload": "agent:a1b2c3d4e5f6", "principal": "service:s1s2s3s4s5s6", "name": "main"}, {"name": "work", "location": "aws-1"}, {"name": "fresh"}], "unreached": [], "answered": []}'
"#,
    )
    .unwrap();
    let at = |name: &str| {
        world
            .ask("secret.grantee", serde_json::json!(name))
            .unwrap()
    };
    assert_eq!(
        at("agent:main"),
        serde_json::json!({"here": {"workload": "agent:a1b2c3d4e5f6", "principal": "service:s1s2s3s4s5s6"}})
    );
    assert_eq!(
        at("agent:work"),
        serde_json::json!({"on": {"place": "aws-1"}})
    );
    assert_eq!(
        at("agent:fresh"),
        serde_json::json!({"unstarted": {"how": "start it once (lnk agent start)"}})
    );
    assert_eq!(at("agent:nobody"), serde_json::json!("none"));
    // A workload of another kind is another module's.
    assert_eq!(at("job:main"), serde_json::json!("unkept"));
    assert!(
        world
            .ask("secret.nothing", serde_json::json!(null))
            .is_err()
    );
}

/// An asker below the oldest version this module accepts of a question's
/// protocol is refused by name, and a bare question, out of its
/// envelope, isn't one.
#[test]
fn an_asker_below_the_oldest_protocol_is_refused_by_name() {
    let world = World::new("older");
    let refused = world
        .ask_raw(
            "secret.grantee",
            serde_json::json!({"protocol": 1, "question": "agent:main"}),
        )
        .unwrap_err();
    assert!(
        refused.contains(
            "the module asking secret.grantee speaks the secret grantee answers protocol 1"
        ),
        "{refused}"
    );
    assert!(
        world
            .ask_raw("secret.grantee", serde_json::json!("agent:main"))
            .is_err()
    );
}

/// A box deleted (`box.removed`): each agent this computer's note puts on
/// it went with it, so the key that box had for its buckets is revoked and
/// the note ends; one whose key won't go keeps its note and is said; an
/// agent on another box, or on a box made again under the name, is left.
#[test]
fn a_deleted_boxs_agents_lose_their_bucket_keys_and_notes() {
    let world = World::new("box-removed");
    let moved = link_files::layout::Hidden::Moved.in_home(&world.path("home"));
    std::fs::create_dir_all(&moved).unwrap();
    let note = |id: &str, name: &str, to: &str, machine: &str| {
        std::fs::write(
            moved.join(format!("{id}.toml")),
            format!(
                "schema = 1\nid = \"{id}\"\nname = \"{name}\"\nat = 1\n\n[to]\nname = \"{to}\"\nresource = \"box:{machine}\"\n"
            ),
        )
        .unwrap();
    };
    note("a1b2c3d4e5f6", "main", "b1", "b0x1a2w3s4b5");
    note("b2c3d4e5f6a1", "side", "b1", "b0x1a2w3s4b5");
    note("c3d4e5f6a1b2", "other", "b2", "b0x2a2w3s4b5");
    note("d4e5f6a1b2c3", "again", "b1", "b0x9a9w9s9b9");
    let released = world
        .ask(
            "box.removed",
            serde_json::json!({"name": "b1", "machine_id": "b0x1a2w3s4b5"}),
        )
        .unwrap();
    let stays = released["stays"].as_array().unwrap();
    assert_eq!(stays.len(), 1, "{released}");
    assert!(
        stays[0]
            .as_str()
            .unwrap()
            .contains("lnk storage revoke --owner agent:b2c3d4e5f6a1"),
        "{released}"
    );
    let log = world.log();
    assert!(
        log.contains("storage revoke --owner agent:a1b2c3d4e5f6"),
        "{log}"
    );
    assert!(!log.contains("agent:c3d4e5f6a1b2"), "{log}");
    assert!(!log.contains("agent:d4e5f6a1b2c3"), "{log}");
    // Its note ends with its key; the rest stay.
    assert!(!moved.join("a1b2c3d4e5f6.toml").exists());
    for kept in ["b2c3d4e5f6a1", "c3d4e5f6a1b2", "d4e5f6a1b2c3"] {
        assert!(moved.join(format!("{kept}.toml")).exists(), "{kept}");
    }
}
