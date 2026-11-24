//! `lnk secret`, run as `lnk-secret` in a home of its own: a secret kept
//! with how it's given and where it's granted, listed and removed, never
//! printed.

link_test_kit::program!(system);

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};

use link_test_kit::TempDir;

mod answers;
mod copies;
mod keeper_guards;

const VALUE: &str = "sk_live_kept_value";

/// The agent `main` here, as the agent module answers for it.
/// The service agent main acts as.
const MAIN: &str = "s1s2s3s4s5s6";

/// Agent main's workload, as a grant names it: its own ID, as the stand-in
/// `lnk` says it.
const MAIN_WORKLOAD: &str = "agent:m1m2m3m4m5m6";

#[test]
fn bootstrap_is_recipient_sealed_tpm_bound_and_restartable() {
    use std::io::{BufRead, Read};
    let dir = TempDir::new("secret-bootstrap");
    let fake = dir.join("tpm2");
    let device = dir.join("device");
    std::fs::create_dir(&device).unwrap();
    link_test_kit::write_program(
        &fake,
        format!(
            r#"#!/bin/sh
set -e
device='{device}'
action=$1
[ "$2 $3" = "-T device:/dev/tpmrm0" ] || exit 9
case $action in
createprimary) printf primary > primary.ctx ;;
create)
  given=; previous=
  for arg in "$@"; do
    [ "$arg" = -G ] && exit 9
    [ "$previous" = -i ] && [ "$arg" = - ] && given=1
    previous=$arg
  done
  [ -n "$given" ] || exit 9
  name=$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')
  cat > "$device/$name"
  printf this-device > secret.pub
  printf %s "$name" > secret.priv ;;
load)
  [ "$(cat secret.pub)" = this-device ] || exit 9
  cat secret.priv > object.ctx ;;
unseal)
  [ -e "$device/wrong-tpm" ] && exit 7
  cat "$device/$(cat object.ctx)" ;;
*) exit 8 ;;
esac
"#,
            device = device.display()
        ),
    )
    .unwrap();
    let cache = dir.join("cache/boot.cred");
    let command = || {
        let mut command = link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-secret"), &dir);
        command
            .args(["bootstrap", "--instance", "box-17", "--file"])
            .arg(&cache)
            .env("LNK_TEST_BOOTSTRAP", "1")
            .env("LNK_TEST_TPM", &fake)
            .env("LNK_TEST_TPM_RUN", dir.join("run"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    };
    let mut child = link_test_kit::ChildGroup::spawn(&mut command()).unwrap();
    let mut out = std::io::BufReader::new(child.take_stdout().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    let to: link_secretive::SealTo = line
        .strip_prefix("lnk-seal-to ")
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let plain =
        serde_json::to_vec(&std::collections::BTreeMap::from([("credential", VALUE)])).unwrap();
    let sealed = link_secretive::Sealed::seal(
        &link_secretive::Secret::new(plain.clone()),
        &to,
        link_secretive::About {
            owner: "box-17".into(),
            kind: "box-bootstrap".into(),
            version: 1,
        },
    )
    .unwrap();
    serde_json::to_writer(child.take_stdin().unwrap(), &sealed).unwrap();
    let mut opened = Vec::new();
    out.read_to_end(&mut opened).unwrap();
    let status = link_test_kit::eventually(
        "bootstrap closes after its sealed handoff",
        std::time::Duration::from_secs(5),
        || child.try_status().unwrap(),
    );
    assert!(status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&opened).unwrap(),
        serde_json::from_slice::<serde_json::Value>(&plain).unwrap()
    );
    let disk = std::fs::read(&cache).unwrap();
    assert!(disk.starts_with(b"LNK-TPM1\n"));
    assert!(!disk.windows(VALUE.len()).any(|w| w == VALUE.as_bytes()));
    let restarted = command().output().unwrap();
    assert!(
        restarted.status.success(),
        "{}",
        String::from_utf8_lossy(&restarted.stderr)
    );
    assert!(restarted.stdout.starts_with(b"lnk-boot-ready\n"));
    std::fs::write(device.join("wrong-tpm"), "").unwrap();
    let wrong = command().output().unwrap();
    assert!(!wrong.status.success());
    assert!(
        wrong.stdout.is_empty(),
        "a broken cache must never request another handoff"
    );
    assert!(!String::from_utf8_lossy(&wrong.stderr).contains(VALUE));
    std::fs::remove_file(device.join("wrong-tpm")).unwrap();
    std::fs::write(&cache, &plain).unwrap();
    assert!(command().output().unwrap().status.success());
    assert!(std::fs::read(&cache).unwrap().starts_with(b"LNK-TPM1\n"));
    std::fs::write(&cache, format!("{{\"credential\":[\"{VALUE}\"]}}")).unwrap();
    let malformed = command().output().unwrap();
    assert!(!malformed.status.success());
    assert!(malformed.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&malformed.stderr).contains(VALUE));
    if !Path::new("/dev/tpmrm0").exists() {
        let missing = command().env_remove("LNK_TEST_TPM").output().unwrap();
        assert!(!missing.status.success());
        assert!(missing.stdout.is_empty());
        assert!(String::from_utf8_lossy(&missing.stderr).contains("requires its vTPM"));
    }
}

struct World {
    dir: TempDir,
    /// A stand-in for the Mac's `security`, when the Keychain's on.
    security: Option<PathBuf>,
    /// The keeper's socket, once there's one, with a stand-in for `sudo`.
    keeper: Option<PathBuf>,
    /// More of the environment `lnk-secret` runs in.
    env: Vec<(&'static str, PathBuf)>,
}

impl World {
    fn new(test: &str) -> World {
        let dir = TempDir::new(&format!("secret-{test}"));
        std::fs::create_dir_all(dir.join("home")).unwrap();
        // `lnk`, listing the harness module; and its program beside it,
        // answering secret's questions: main here, work moved to the box
        // aws-1; each copy reached as the test says.
        link_test_kit::write_program(
            &dir.join("lnk"),
            r#"#!/bin/sh
here=$(dirname "$0")
echo "$*" >> "$here/lnk.log"
case "$*" in
  "module list --json")
    [ -e "$here/no-agent" ] && { echo '[]'; exit 0; }
    echo '[{"name": "agent", "installed": true, "answers": ["secret.copies", "secret.grantee"]}]' ;;
  "uninstall --parts --json") echo '{"parts": []}' ;;
  *) exit 1 ;;
esac
"#,
        )
        .unwrap();
        link_test_kit::write_program(
            &dir.join("lnk-agent"),
            r#"#!/bin/sh
here=$(dirname "$0")
echo "agent $*" >> "$here/lnk.log"
case "$*" in
  "--link-answer secret.grantee")
    read -r asked
    case "$asked" in
      '{"protocol":2,"question":"agent:main"}') at='{"here": {"workload": "agent:m1m2m3m4m5m6", "principal": "service:s1s2s3s4s5s6"}}' ;;
      '{"protocol":2,"question":"agent:work"}') at='{"on": {"place": "aws-1"}}' ;;
      '{"protocol":2,"question":"agent:fresh"}') at='{"unstarted": {"how": "start it once"}}' ;;
      *) at='"none"' ;;
    esac
    echo '{"protocol": 2, "answer": '"$at"'}' ;;
  "--link-answer secret.copies")
    cat > "$here/asked.json"
    printf '{"protocol": 2, "answer": '; cat "$here/reached.json"; echo '}' ;;
  *) exit 1 ;;
esac
"#,
        )
        .unwrap();
        World {
            dir,
            security: None,
            keeper: None,
            env: Vec::new(),
        }
    }

    /// [`World::new`], with a Keychain: a stand-in for `security`.
    fn with_keychain(test: &str) -> World {
        let mut world = World::new(test);
        world.security = Some(link_test_kit::keychain::stand_in(&world.dir));
        world
    }

    /// `lnk-secret <args>`, `input` its input.
    fn run_with(&self, args: &[&str], input: &str) -> Output {
        let mut command =
            link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-secret"), &self.dir.join("home"));
        command
            .args(args)
            .current_dir(self.dir.join("home"))
            .env("HOME", self.dir.join("home"))
            .env_remove("XDG_CONFIG_HOME")
            .env("LNK_SERVICES", "off")
            .env("LNK_EXE", self.dir.join("lnk"))
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.dir.join("adapters").display()),
            );
        command.envs(self.env.iter().map(|(name, value)| (name, value)));
        if let Some(keeper) = &self.keeper {
            command
                .env("LNK_TEST_KEEPER", keeper)
                .env("LNK_SUDO", self.dir.join("sudo"));
        }
        match &self.security {
            Some(security) => command.env("LNK_SECURITY", security),
            // A Mac keeps secrets in its Keychain alone: a stand-in there.
            None => command
                .env_remove("LNK_SECURITY")
                .envs(link_test_kit::keychain::isolated(&self.dir.join("home"))),
        };
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // A refusal comes before its input is read, and it ends without
        // reading it: its pipe closed then is no error of the test's.
        let wrote = child.stdin.take().unwrap().write_all(input.as_bytes());
        if let Err(err) = wrote {
            assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe, "{err}");
        }
        let out = child.wait_with_output().unwrap();
        let said = format!("{}{}", text(&out.stdout), text(&out.stderr));
        assert!(!said.contains(VALUE), "the value printed: {said}");
        out
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with(args, "")
    }

    /// `lnk secret create`, then `lnk secret add` for the grants among
    /// `args` (a name, then flags): what both said, and how the last
    /// ended.
    fn add(&self, args: &[&str]) -> Output {
        let (name, flags) = args.split_first().unwrap();
        let (mut grants, mut sealing) = (Vec::new(), Vec::new());
        let mut flags = flags.iter();
        while let Some(flag) = flags.next() {
            let to = match *flag {
                "--to" => &mut grants,
                _ => &mut sealing,
            };
            to.push(*flag);
            if let Some(value) = flags.next() {
                to.push(*value);
            }
        }
        let created = self.run_with(
            &[&["create", name], sealing.as_slice()].concat(),
            &format!("{VALUE}\n"),
        );
        if grants.is_empty() || !created.status.success() {
            return created;
        }
        let given = self.run(&[&["add", name], grants.as_slice()].concat());
        Output {
            status: given.status,
            stdout: [created.stdout, given.stdout].concat(),
            stderr: [created.stderr, given.stderr].concat(),
        }
    }

    fn store(&self) -> PathBuf {
        self.dir.join("home/.lnk/safe/secrets/secrets.toml")
    }

    /// What `lnk secret list --json` lists, each ID as `<kind>:<id>` under
    /// its axis.
    fn listed(&self) -> serde_json::Value {
        let out = self.run(&["list", "--json"]);
        assert!(out.status.success(), "{}", text(&out.stderr));
        let listed = serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap();
        link_test_kit::ids::assert_named(&listed);
        listed["secrets"].clone()
    }

    /// The ID of the secret listed first, `<id>` of its `secret:<id>`.
    fn first_id(&self) -> String {
        let listed = self.listed();
        let resource = listed[0]["resource"].as_str().unwrap();
        resource.strip_prefix("secret:").unwrap().to_string()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn a_secret_is_kept_with_how_its_given_and_where_its_granted() {
    let world = World::new("add");
    let out = world.add(&[
        "STRIPE_KEY",
        "--to",
        "agent:main",
        "--internet-host",
        "api.stripe.com",
        "--header",
        "X-Api-Key",
    ]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).contains(&format!("is given to main ({MAIN_WORKLOAD})")),
        "{}",
        text(&out.stdout)
    );
    // Kept by an ID of its own, which creating it said.
    let first = world.first_id();
    assert!(
        text(&out.stdout).contains(&format!("as secret:{first}")),
        "{}",
        text(&out.stdout)
    );
    let mut listed = world.listed();
    listed[0].as_object_mut().unwrap().remove("resource");
    assert_eq!(
        listed,
        serde_json::json!([{
            "kind": "named",
            "name": "STRIPE_KEY",
            "host": "api.stripe.com",
            "header": "X-Api-Key",
            "format": "{}",
            "to": [{"principal": format!("service:{MAIN}"), "action": "secret.secret.pass", "resource": format!("secret:{first}"), "workload": MAIN_WORKLOAD, "name": "main", "as": {"kind": "variable"}}],
            "copies": [],
            "kept": true,
        }])
    );
    // Kept only for this user, the value beside its grant.
    let store = world.store();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&store).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    assert!(std::fs::read_to_string(&store).unwrap().contains(VALUE));
    let shown = world.run(&["list"]);
    assert!(
        text(&shown.stdout).contains(&format!(
            "STRIPE_KEY  sealed, for api.stripe.com, in X-Api-Key: {{}}, to main ({MAIN_WORKLOAD})"
        )),
        "{}",
        text(&shown.stdout)
    );

    // Created again, it's replaced whole, under a new ID: for another host
    // now, saying what it was, and given to no one, the first's grant gone
    // with its ID.
    let out = world.add(&["STRIPE_KEY", "--internet-host", "api.example.com"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("STRIPE_KEY replaced, which was sealed, for api.stripe.com"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(world.listed()[0]["host"], "api.example.com");
    assert_eq!(world.listed()[0]["to"], serde_json::json!([]));
    let second = world.first_id();
    assert_ne!(first, second);
    let store_text = std::fs::read_to_string(&store).unwrap();
    assert!(
        store_text.contains(&format!("retired = [\"{first}\"]")),
        "{store_text}"
    );
    // The first's ID names nothing now, and every command takes an ID.
    let out = world.run(&["add", &format!("secret:{first}"), "--to", "agent:main"]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("there is no secret"),
        "{}",
        text(&out.stderr)
    );
    let out = world.run(&["add", &format!("secret:{second}"), "--to", "agent:main"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let out = world.run(&["show", &format!("secret:{second}")]);
    assert!(
        text(&out.stdout).contains(&format!("secret:{second}")),
        "{}",
        text(&out.stdout)
    );

    // Removed, value and grants.
    let out = world.run(&["delete", &format!("secret:{second}")]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(world.listed(), serde_json::json!([]));
    assert!(!std::fs::read_to_string(&store).unwrap().contains(VALUE));
    let out = world.run(&["delete", "STRIPE_KEY"]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("There is no secret STRIPE_KEY here"));
}

/// A secret is given to an agent alone, by the service it acts as:
/// never to an environment or a job, which `lnk env approval` gives it.
#[test]
fn a_secret_is_given_to_no_environment_or_job() {
    let world = World::new("no-workloads");
    for flag in ["--job", "--env"] {
        let out = world.add(&["NPM_TOKEN", flag, "books"]);
        assert!(!out.status.success(), "{flag}");
        assert!(text(&out.stderr).contains(flag), "{}", text(&out.stderr));
    }
}

#[test]
fn a_secret_is_granted_only_to_an_agent() {
    let world = World::new("grants");
    // Given nowhere, so refused.
    let out = world.run(&["add", "NPM_TOKEN"]);
    assert!(!out.status.success());
    // Where it is given is a flag of its type: a box's user, a team and a
    // word that is not a grant have none.
    for to in ["--user", "--box", "--team", "--agent"] {
        let out = world.add(&[
            "NPM_TOKEN",
            to,
            "ann",
            "--internet-host",
            "registry.npmjs.org",
        ]);
        assert!(!out.status.success(), "{to}");
        assert!(text(&out.stderr).contains(to), "{}", text(&out.stderr));
    }
    // An ID that names no secret here, or isn't one.
    for which in ["secret:a1b2c3d4e5f6", "secret:", "a1b2c3d4e5f6"] {
        let out = world.run(&["show", which]);
        assert!(!out.status.success(), "{which}");
    }
    // A header that can't hold a secret.
    let out = world.add(&[
        "STRIPE_KEY",
        "--to",
        "agent:main",
        "--internet-host",
        "api.stripe.com",
        "--header",
        "Cookie",
    ]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("a sealed secret goes in Authorization"));
    // Nothing refused was kept.
    assert!(!world.store().exists());
}

#[test]
fn uninstalling_names_the_store_and_its_keychain_items() {
    let world = World::new("uninstall");
    let out = world.add(&[
        "NPM_TOKEN",
        "--to",
        "agent:main",
        "--internet-host",
        "api.example.com",
    ]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let parts = world.run(&["uninstall", "--parts", "--json"]);
    assert!(parts.status.success(), "{}", text(&parts.stderr));
    let parts: serde_json::Value = serde_json::from_slice(&parts.stdout).unwrap();
    let part = &parts["parts"][0];
    assert_eq!(part["name"], "secrets");
    assert_eq!(part["keys"], serde_json::json!(["secret/"]));
    let held = &part["credentials"][0];
    assert_eq!(held["of"], "NPM_TOKEN");
    assert!(
        held["may"]
            .as_str()
            .unwrap()
            .contains(&format!("main ({MAIN_WORKLOAD})")),
        "{held}"
    );
}

#[test]
fn with_a_keychain_the_value_is_there_and_never_in_the_file() {
    let world = World::with_keychain("keychain");
    let out = world.add(&[
        "NPM_TOKEN",
        "--to",
        "agent:main",
        "--internet-host",
        "api.example.com",
    ]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let store = std::fs::read_to_string(world.store()).unwrap();
    assert!(
        !store.contains(VALUE) && store.contains("NPM_TOKEN"),
        "{store}"
    );
    // Its Keychain account is its ID's, never its name's.
    let id = world.first_id();
    let item = world.dir.join(format!("items/secret/{id}"));
    assert_eq!(std::fs::read_to_string(&item).unwrap().trim(), VALUE);
    let parts = world.run(&["uninstall", "--parts", "--json"]);
    let parts: serde_json::Value = serde_json::from_slice(&parts.stdout).unwrap();
    let kept = parts["parts"][0]["credentials"][0]["kept"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        kept.contains(&format!("the Keychain (link:secret/{id})")),
        "{kept}"
    );
    // Removed, from the Keychain too.
    let out = world.run(&["delete", "NPM_TOKEN"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(!item.exists());
}

/// A Keychain that can't keep the value (locked, or asking for a person)
/// fails `lnk secret create`: nothing is saved, and the value is in no
/// file, never kept in the store instead.
#[test]
fn a_failing_keychain_fails_create_and_keeps_no_file() {
    let mut world = World::new("refusing");
    world.security = Some(link_test_kit::keychain::refusing(&world.dir));
    let out = world.add(&["NPM_TOKEN", "--to", "agent:main"]);
    assert!(!out.status.success(), "create succeeded");
    assert!(
        text(&out.stderr).contains("Keychain"),
        "{}",
        text(&out.stderr)
    );
    let store = std::fs::read_to_string(world.store()).unwrap_or_default();
    assert!(!store.contains(VALUE), "{store}");
    assert!(!store.contains("NPM_TOKEN"), "{store}");
}

/// A Keychain that won't let a deleted secret's item go fails `lnk
/// secret delete` naming the item, which nothing here names any more,
/// for you to delete with Keychain Access.
#[test]
fn a_keychain_keeping_a_deleted_item_names_it() {
    let mut world = World::with_keychain("refusing-delete");
    let out = world.add(&["NPM_TOKEN"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let id = world.first_id();
    world.security = Some(link_test_kit::keychain::refusing(&world.dir));
    let out = world.run(&["delete", "NPM_TOKEN"]);
    assert!(!out.status.success(), "delete succeeded");
    let said = text(&out.stderr);
    assert!(said.contains(&format!("link:secret/{id}")), "{said}");
    assert!(said.contains("Keychain Access"), "{said}");
    assert!(!said.contains(VALUE), "{said}");
}

#[test]
fn adds_at_once_each_keep_their_secret() {
    let world = World::new("together");
    let names: Vec<String> = (0..8).map(|i| format!("TOKEN_{i}")).collect();
    std::thread::scope(|s| {
        for name in &names {
            let world = &world;
            s.spawn(move || {
                let out = world.add(&[
                    name,
                    "--to",
                    "agent:main",
                    "--internet-host",
                    "api.example.com",
                ]);
                assert!(out.status.success(), "{}", text(&out.stderr));
            });
        }
    });
    let listed: Vec<String> = world
        .listed()
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(listed, names);
}

#[test]
fn a_store_that_cant_be_read_is_said_never_shown_as_none() {
    let world = World::new("broken");
    std::fs::create_dir_all(world.store().parent().unwrap()).unwrap();
    std::fs::write(
        world.store(),
        format!("[entry.{MAIN}]\nname = \"NPM_TOKEN\"\nto = \"{VALUE}\"\n"),
    )
    .unwrap();
    let out = world.run(&["list"]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("line 3: not as lnk secret create writes it"));
    let parts = world.run(&["uninstall", "--parts", "--json"]);
    assert!(parts.status.success(), "{}", text(&parts.stderr));
    let parts: serde_json::Value = serde_json::from_slice(&parts.stdout).unwrap();
    let held = &parts["parts"][0]["credentials"][0];
    assert_eq!(held["what"], "Secrets you keep, unreadable", "{held}");
}

/// A store an earlier `lnk` wrote, each secret by its name, is refused,
/// telling you to add them again: nothing reads or converts it.
#[test]
fn a_store_of_the_shape_before_ids_is_refused() {
    let world = World::new("before-ids");
    std::fs::create_dir_all(world.store().parent().unwrap()).unwrap();
    std::fs::write(
        world.store(),
        format!(
            "[NPM_TOKEN]\nvalue = \"{VALUE}\"\n\
             to = [{{ kind = \"agent\", id = \"{MAIN}\", name = \"main\", as = \"variable\" }}]\n"
        ),
    )
    .unwrap();
    for args in [&["list"][..], &["add", "NPM_TOKEN", "--to", "agent:main"]] {
        let out = world.run(args);
        assert!(!out.status.success(), "{args:?}");
        assert!(
            text(&out.stderr).contains("add your secrets again with lnk secret create"),
            "{}",
            text(&out.stderr)
        );
    }
    assert!(
        std::fs::read_to_string(world.store())
            .unwrap()
            .contains("[NPM_TOKEN]")
    );
    // Where there's a Keychain, the refusal names the items it leaves
    // there, which nothing reads now; where the file held the values,
    // there are none to name.
    let mut world = world;
    let said = |world: &World| text(&world.run(&["list"]).stderr);
    #[cfg(not(target_os = "macos"))]
    assert!(!said(&world).contains("link:secret/"), "{}", said(&world));
    world.security = Some(link_test_kit::keychain::stand_in(&world.dir));
    let refused = said(&world);
    assert!(
        refused.contains("delete link:secret/NPM_TOKEN"),
        "{refused}"
    );
}

#[test]
fn an_agent_is_granted_a_secret_sealed_by_its_id_and_only_here() {
    let world = World::new("agents");
    let out = world.add(&[
        "STRIPE_KEY",
        "--to",
        "agent:main",
        "--internet-host",
        "api.stripe.com",
    ]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(
        world.listed()[0]["to"],
        serde_json::json!([{"principal": format!("service:{MAIN}"), "action": "secret.secret.pass", "resource": world.listed()[0]["resource"], "workload": MAIN_WORKLOAD, "name": "main", "as": {"kind": "variable"}}])
    );
    // Plain, it would be the model's to read and send anywhere: a secret
    // is added sealed, for its host, or not at all.
    let out = world.add(&["NPM_TOKEN", "--to", "agent:main"]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("--internet-host"),
        "{}",
        text(&out.stderr)
    );
    // On a box: granted there, never pushed from here.
    let out = world.add(&[
        "OTHER_KEY",
        "--to",
        "agent:work",
        "--internet-host",
        "api.stripe.com",
    ]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("work is on aws-1: a secret is granted to it there"));
    let out = world.add(&[
        "OTHER_KEY",
        "--to",
        "agent:nobody",
        "--internet-host",
        "api.stripe.com",
    ]);
    assert!(!out.status.success());
    let said = text(&out.stderr);
    assert!(
        said.contains("nothing here is") && said.contains(":nobody"),
        "{said}"
    );
    let out = world.add(&[
        "OTHER_KEY",
        "--to",
        "agent:fresh",
        "--internet-host",
        "api.stripe.com",
    ]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("fresh has no ID yet: start it once"));
    // A refused grant leaves the secret kept, given to no one.
    let given = |listed: &serde_json::Value| {
        listed
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| !l["to"].as_array().unwrap().is_empty())
            .count()
    };
    assert_eq!(given(&world.listed()), 1);
}

/// The keeper, as its service runs it, on a socket in `world`, each
/// user's store in its `lib/`: stopped when it's dropped.
struct Keeper(link_test_kit::ChildGroup);

impl Keeper {
    fn start(world: &World) -> (Keeper, PathBuf) {
        let run = world.dir.join("run");
        std::fs::create_dir_all(&run).unwrap();
        let socket = run.join("keeper.sock");
        let mut command =
            link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-secret"), &world.dir.join("home"));
        command
            .args(["keep", "--socket"])
            .arg(&socket)
            .arg("--store")
            .arg(world.dir.join("lib"))
            .env("LNK_TEST_KEEPER", &socket)
            .stdin(Stdio::null());
        let child = link_test_kit::ChildGroup::spawn(&mut command).unwrap();
        link_test_kit::eventually("keeper socket", std::time::Duration::from_secs(5), || {
            socket.exists()
        });
        (Keeper(child), socket)
    }
}

impl Drop for Keeper {
    fn drop(&mut self) {
        self.0.stop();
    }
}

#[test]
fn the_keeper_takes_your_secrets_and_keeps_them_out_of_your_files() {
    let mut world = World::new("keeper");
    // The first, with no terminal to ask in: your files, said so.
    let out = world.add(&[
        "NPM_TOKEN",
        "--to",
        "agent:main",
        "--internet-host",
        "api.example.com",
    ]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("any program you run can read"),
        "{}",
        text(&out.stderr)
    );
    assert!(
        std::fs::read_to_string(world.store())
            .unwrap()
            .contains(VALUE)
    );
    let machine = world.dir.join("home/.lnk/safe/installation.toml");
    assert!(
        !std::fs::read_to_string(&machine)
            .unwrap_or_default()
            .contains("keeper")
    );

    // `lnk secret keeper create` never takes your file: refused while it holds
    // a secret, before anything is made.
    let (_keeper, socket) = Keeper::start(&world);
    link_test_kit::write_program(
        &world.dir.join("sudo"),
        "#!/bin/sh\necho \"$*\" >> \"$(dirname \"$0\")/sudo.log\"\n",
    )
    .unwrap();
    world.env = vec![("LNK_SUDO", world.dir.join("sudo"))];
    let out = world.run(&["keeper", "create"]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("remove the file, then add them again with lnk secret create"),
        "{}",
        text(&out.stderr)
    );
    assert!(!world.dir.join("sudo.log").exists());
    // Removed: its program and unit put in place as root, its service
    // started.
    std::fs::remove_file(world.store()).unwrap();
    world.env.clear();
    world.keeper = Some(socket);
    let out = world.run(&["keeper", "create"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let sudo = std::fs::read_to_string(world.dir.join("sudo.log")).unwrap();
    for line in [
        "-m 755",
        "/usr/local/lib/lnk/lnk-secret",
        "/etc/systemd/system/lnk-secrets.service",
        "systemctl enable lnk-secrets.service",
        "systemctl restart lnk-secrets.service",
    ] {
        assert!(sudo.contains(line), "{line}: {sudo}");
    }
    // Kept by it from now on, in this user's store (the test's folder is),
    // never your file: added again, listed and given as before.
    let out = world.add(&[
        "NPM_TOKEN",
        "--to",
        "agent:main",
        "--internet-host",
        "api.example.com",
    ]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let uid = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(&world.dir).unwrap());
    let kept = world
        .dir
        .join("lib")
        .join(uid.to_string())
        .join("secrets.toml");
    assert!(std::fs::read_to_string(&kept).unwrap().contains(VALUE));
    assert!(!world.store().exists());
    assert_eq!(world.listed()[0]["name"], "NPM_TOKEN");
    let out = world.add(&[
        "PYPI_TOKEN",
        "--to",
        "agent:main",
        "--internet-host",
        "api.example.com",
    ]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(!world.store().exists());
    assert_eq!(world.listed().as_array().unwrap().len(), 2);
    let parts = world.run(&["uninstall", "--parts", "--json"]);
    let parts: serde_json::Value = serde_json::from_slice(&parts.stdout).unwrap();
    let held = &parts["parts"][0]["credentials"][0];
    assert!(
        held["kept"].as_str().unwrap().contains("lnk-secrets"),
        "{held}"
    );
    // Uninstalled, the keeper forgets yours.
    let out = world.run(&["uninstall", "--delete", "secrets"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(!kept.exists());
}

/// A TPM's device that's root's alone, as on Google Cloud's Ubuntu image:
/// setup gives it a group of its own, now and at every boot, and the
/// keeper's service joins that group. One it can't give is refused.
#[test]
fn the_keeper_gives_a_root_only_tpm_a_group_of_its_own() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let mut world = World::new("keeper-tpm");
    let device = world.dir.join("tpmrm0");
    std::fs::write(&device, "").unwrap();
    std::fs::set_permissions(&device, std::fs::Permissions::from_mode(0o600)).unwrap();
    // As sudo would: the group asked becomes a non-root one this user
    // may give (root may give any).
    let given = if unsafe { libc::geteuid() } == 0 {
        65534
    } else {
        unsafe { libc::getegid() }
    };
    let sudo = |refuse: bool| {
        link_test_kit::write_program(
            &world.dir.join("sudo"),
            format!(
                "#!/bin/sh\necho \"$*\" >> \"$(dirname \"$0\")/sudo.log\"\n{}",
                if refuse {
                    String::new()
                } else {
                    format!(
                        "case \"$1\" in\n  chgrp) chgrp {given} \"$3\" ;;\n  chmod) chmod \"$2\" \"$3\" ;;\nesac\n"
                    )
                }
            ),
        )
        .unwrap();
    };
    sudo(false);
    let (_keeper, socket) = Keeper::start(&world);
    world.keeper = Some(socket);
    world.env = vec![
        ("LNK_TEST_TPM", world.dir.join("tpm2")),
        ("LNK_TEST_TPM_DEVICE", device.clone()),
    ];
    let out = world.run(&["keeper", "create"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let log = std::fs::read_to_string(world.dir.join("sudo.log")).unwrap();
    let at = |line: &str| log.find(line).unwrap_or_else(|| panic!("{line}: {log}"));
    at("apt-get -o Acquire::Retries="); // as the sandbox's install, retrying a slow mirror
    assert!(at("install -y -q tpm2-tools") < at("groupadd --system --force lnk-tpm"));
    at("/etc/udev/rules.d/70-lnk-tpm.rules");
    at(&format!("chgrp lnk-tpm {}", device.display()));
    at(&format!("chmod 0660 {}", device.display()));
    let found = std::fs::metadata(&device).unwrap();
    assert_eq!((found.gid(), found.mode() & 0o777), (given, 0o660));
    assert!(
        text(&out.stderr).contains("seals its store to this machine's TPM"),
        "{}",
        text(&out.stderr)
    );

    // A device sudo didn't give: refused, never kept unsealed.
    std::fs::set_permissions(&device, std::fs::Permissions::from_mode(0o600)).unwrap();
    sudo(true);
    let out = world.run(&["keeper", "create"]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("needs a dedicated non-root group"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn the_keeper_runs_only_as_its_own_user() {
    let world = World::new("keep");
    let out = world.run(&["keep"]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("runs as lnk-secrets alone"),
        "{}",
        text(&out.stderr)
    );
}

/// A grant is the connection: `lnk secret remove` takes it away and the
/// secret stays kept, `lnk secret delete` takes the secret.
#[test]
fn a_grant_is_taken_away_and_the_secret_stays_kept_until_deleted() {
    let world = World::new("ungrant");
    let out = world.add(&[
        "STRIPE_KEY",
        "--to",
        "agent:main",
        "--internet-host",
        "api.stripe.com",
    ]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let out = world.run(&["remove", "STRIPE_KEY", "--to", "agent:main"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).contains(&format!("no longer given to main ({MAIN_WORKLOAD})")),
        "{}",
        text(&out.stdout)
    );
    let listed = world.listed();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["to"], serde_json::json!([]));
    // Given again, then the secret deleted with its grants.
    let out = world.run(&["add", "STRIPE_KEY", "--to", "agent:main"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let out = world.run(&["delete", "STRIPE_KEY"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(world.listed().as_array().unwrap().len(), 0);
}
