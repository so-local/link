//! Box images with fakes: `lnk` (for `lnk cloud env`), a cloud adapter
//! (`lnk-fake`) and `ssh`. A box saved as an
//! image (credentials refused, stopped, saved, started again), a box made from it
//! with an identity and host key of its own, and the image listed and
//! removed.

use std::path::{Path, PathBuf};
use std::process::Command;

use link_test_kit::TempDir;

use link_test_kit::release;

fn script(path: &Path, body: &str) {
    link_test_kit::write_program(path, format!("#!/bin/sh\n{body}\n")).unwrap();
}

const KEY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIGx1pZlbXq0WyU1b4v5r0Ke+ZtrQ2mX5d9J8lJk3aB7c";
const KEY2: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIKEPTkeptKEPTkeptKEPTkeptKEPTkeptKEPTkeptKEPT";

const SAVED: &str = r#"{"images": [{"id": "img-1", "name": "web-ready", "place": "moon-1", "state": "ready", "disk_gb": 30, "arch": "x86_64", "created": "2026-10-01T10:00:00Z"}]}"#;

struct World {
    dir: TempDir,
    bin: PathBuf,
    /// The releases its boxes install lnk from.
    releases: release::Releases,
}

impl World {
    fn new(test: &str) -> Option<Self> {
        let dir = TempDir::new(&format!("images-{test}"));
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(dir.join("home/.lnk/safe/accounts")).unwrap();
        std::fs::write(dir.join("home/.lnk/safe/accounts/accounts.toml"), "").unwrap();
        let at = |name: &str| dir.join(name).display().to_string();
        script(
            &bin.join("lnk"),
            r#"case "$*" in
  "cloud env acct --json") echo '{"name": "acct", "kind": "fake", "place": "moon-1", "env": {}}' ;;
  "cloud list --json") echo '[{"name": "acct", "kind": "fake"}]' ;;
  "module list --json") echo '[{"name": "login", "installed": true, "answers": ["lnk.account"]}]' ;;
  *) echo "lnk: no $*" >&2; exit 1 ;;
esac"#,
        );
        // The login module, beside lnk: who is signed in, asked directly.
        script(
            &bin.join("lnk-login"),
            r#"case "$*" in
  "--link-answer lnk.account") cat > /dev/null; echo '{"protocol": 1, "answer": {"username": "me", "relay": "https://relay.example", "owner": "github-7"}}' ;;
  *) echo "lnk-login: no $*" >&2; exit 1 ;;
esac"#,
        );
        // Each box made is m-<n>, at 192.0.2.<9+n>, with a host key of its
        // own; an image saved is listed until it's removed.
        // A hash in the tags is said back, by an adapter that knows hashes.
        let says_key = r#"[ -n "$key" ] && saved=$(printf '%s' "$saved" | sed "s/\"created\"/\"hash\": \"$key\", \"created\"/")"#;
        let images = format!(
            r#"image-list) cat {images} 2>/dev/null || echo '{{}}' ;;
  image-save) key=$(printf '%s' "$request" | sed -n 's/.*"lnk-hash":"\([a-z0-9-]*\)".*/\1/p'); saved='{SAVED}'
{says_key}
echo "$saved" > {images}; echo "$saved" ;;
  image-remove) rm -f {images}; echo '{{}}' ;;"#,
            images = at("images.json"),
        );
        script(
            &bin.join("lnk-fake"),
            &format!(
                r#"request=$(cat); printf '%s %s\n' "$1" "$request" >> {adapter}
case "$request" in *'"id":"m-2"'*) n=2 ;; *) n=1 ;; esac
case "$1" in
  defaults) echo '{{"place": "moon-1", "size": "tiny"}}' ;;
  launch) n=$(ls {dir} | grep -c '^launched-'); n=$((n + 1)); touch {dir}/launched-$n; echo '{{"id": "m-'$n'"}}' ;;
  wait|start) echo '{{"ip": "192.0.2.1'$n'"}}' ;;
  state) echo '{{"state": "running"}}' ;;
  hostkeys) if [ $n = 2 ]; then echo '{{"host_keys": ["ssh-ed25519 {KEY2}"]}}'; else echo '{{"host_keys": ["ssh-ed25519 {KEY}"]}}'; fi ;;
  describe) echo '{{"boxes": [{{"id": "m-1", "name": "web-1", "place": "moon-1", "state": "running", "disk_gb": 20}}]}}' ;;
  {images}
  *) echo '{{}}' ;;
esac"#,
                adapter = at("adapter.log"),
                dir = dir.display(),
            ),
        );
        // ssh: logs what it's given; the box's lnk answers whoami (signed
        // in until logged out), with agents.json lists its agents, says
        // what its modules keep (kept.json, else nothing; none with
        // old-lnk), and the check of those prints held.
        script(
            &bin.join("ssh"),
            &format!(
                r#"[ "$1" = -G ] && exit 0
echo "$*" >> {ssh}
case "$*" in *"-O check"*) exit 255 ;; *"-O "*|*ControlMaster=yes*) exit 0 ;; esac
if [ ! -t 0 ]; then input=$(cat); printf '%s\n' "$input" >> {ssh}
  case "$input" in
    *"uninstall --parts --json"*) if [ -f {dir}/old-lnk ]; then echo "error: unexpected argument '--parts'" >&2; exit 2; fi; cat {dir}/kept.json 2>/dev/null || echo '{{"parts": []}}' ;;
    *"held()"*) cat {dir}/held 2>/dev/null ;;
    *"&& echo timed"*) [ -f {dir}/timed ] && echo timed ;;
    *"auth logout"*) [ -f {dir}/stays-signed-in ] || touch {dir}/signed-out; echo 'Logged out me.' ;;
    *"auth show"*) if [ -f {dir}/whoami-broken ]; then echo 'error: no'; exit 1; elif [ -f {dir}/signed-out ]; then echo '{{"username": null}}'; else echo '{{"username": "me", "owner": "github-7"}}'; fi ;;
    *"module list"*) if [ -f {dir}/harness ]; then echo '[{{"name": "auth", "installed": true}}, {{"name": "agent", "installed": true}}]'; else echo '[{{"name": "auth", "installed": true}}, {{"name": "agent", "installed": false}}]'; fi ;;
    *"agent list"*) cat {dir}/agents.json 2>/dev/null || exit 1 ;;
    *.git-credentials*) cat {dir}/secrets 2>/dev/null ;;
    *installation.toml*) echo 'lnk-installation abcdefabcdef' ;;
  esac
fi
exit 0"#,
                ssh = at("ssh.log"),
                dir = dir.display(),
            ),
        );
        let releases = release::serve(&format!("images-{test}"), &[])?;
        Some(World { dir, bin, releases })
    }

    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_lnk-box"))
            // lnk hands lnk-box the words after its namespace.
            .args(args.strip_prefix(&["box"][..]).unwrap_or(args))
            .env("HOME", self.dir.join("home"))
            .env("LNK_SERVICES", "off")
            .envs(link_test_kit::keychain::isolated(&self.dir.join("home")))
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("LNK_BOX_SSH")
            .env("LNK_EXE", self.bin.join("lnk"))
            .env("LNK_SSH", self.bin.join("ssh"))
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .envs(self.releases.env())
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "lnk {args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// What a command that must succeed said, on stdout and stderr.
    fn said(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(output.status.success(), "lnk {args:?} failed");
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }

    /// The error of a command that must fail.
    fn fails(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(!output.status.success(), "lnk {args:?} succeeded");
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.join(name)).unwrap_or_default()
    }

    /// The adapter commands run, in order, but `list`.
    fn commands(&self) -> Vec<String> {
        self.read("adapter.log")
            .lines()
            .map(|l| l.split(' ').next().unwrap().to_string())
            .filter(|c| c != "list")
            .collect()
    }

    /// The request of the last adapter command `command`.
    fn request(&self, command: &str) -> serde_json::Value {
        let log = self.read("adapter.log");
        let line = log
            .lines()
            .rfind(|l| l.starts_with(&format!("{command} ")))
            .unwrap_or_else(|| panic!("no {command} in {log}"));
        serde_json::from_str(&line[command.len() + 1..]).unwrap()
    }
}

#[test]
fn a_box_is_saved_and_a_box_made_from_it_is_its_own() {
    let Some(world) = World::new("life") else {
        return;
    };
    world.ok(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--name",
        "web-1",
        "--yes",
        "--lnk.login=false",
    ]);
    std::fs::remove_file(world.dir.join("adapter.log")).unwrap();

    // Saved: signed out, stopped, saved, started again.
    let said = world.said(&["box", "image", "create", "web-1", "web-ready", "--yes"]);
    assert!(
        said.contains("billed to that account by fake every month"),
        "{said}"
    );
    assert!(said.contains("(20 GB)"), "{said}");
    assert!(
        said.contains("The image web-ready is saved as img-1 in moon-1."),
        "{said}"
    );
    let commands = world.commands();
    let at = |c: &str| commands.iter().position(|x| x == c).unwrap();
    assert!(at("image-list") < at("stop"), "{commands:?}");
    assert!(
        at("stop") < at("image-save") && at("image-save") < at("start"),
        "{commands:?}"
    );
    let ssh = world.read("ssh.log");
    assert!(!ssh.contains("auth logout"), "{ssh}");
    // A deadline's timer stays out of the image: its boxes never end at it.
    assert!(
        ssh.contains("rm -f /etc/systemd/system/lnk-deadline.service"),
        "{ssh}"
    );
    let saved = world.request("image-save");
    assert_eq!(saved["id"], "m-1");
    assert_eq!(saved["image"], "web-ready");
    assert_eq!(
        saved["tags"],
        serde_json::json!({"lnk-image": "web-ready", "lnk-owner": "github-7"})
    );
    let listed: serde_json::Value =
        serde_json::from_str(&world.ok(&["box", "list", "--here", "--json"])).unwrap();
    assert_eq!(listed[0]["state"], "running");
    // One name once.
    let again = world.fails(&["box", "image", "create", "web-1", "web-ready", "--yes"]);
    assert!(again.contains("since one is in acct already"), "{again}");

    // A box from it: its region and disk the image's, raised and said.
    let made = world.said(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--image",
        "web-ready",
        "--name",
        "web-2",
        "--yes",
        "--lnk.login=false",
    ]);
    assert!(made.contains("as on image web-ready"), "{made}");
    assert!(made.contains("web-ready (img-1)"), "{made}");
    assert!(
        made.contains("Its disk is 30 GB, the image's size, rather than 20 GB."),
        "{made}"
    );
    assert!(made.contains("Your box web-2 is up"), "{made}");
    let launch = world.request("launch");
    assert_eq!(launch["image"], "img-1");
    assert_eq!(launch["place"], "moon-1");
    assert_eq!(launch["spec"]["disk_gb"], 30);
    assert_eq!(launch["spec"]["name"], "web-2");
    // Its own ID: the image's box's goes before the install writes one.
    let id = launch["spec"]["id"].as_str().unwrap();
    let ssh = world.read("ssh.log");
    let forget = ssh
        .rfind("rm -f \"$HOME/.lnk/safe/installation.toml\"")
        .unwrap();
    let written = ssh.rfind(&format!("|| id={id}\n")).unwrap();
    assert!(forget < written, "{ssh}");
    // Its own host key, from its own console, pinned under its own name:
    // the saved box's isn't trusted for it.
    let known = world.read("home/.lnk/safe/boxes/known_hosts");
    assert!(
        known.contains(&format!("lnk-fake-m-1 ssh-ed25519 {KEY}\n")),
        "{known}"
    );
    assert!(
        known.contains(&format!("lnk-fake-m-2 ssh-ed25519 {KEY2}\n")),
        "{known}"
    );
    assert!(
        !known.contains(&format!("lnk-fake-m-2 ssh-ed25519 {KEY}")),
        "{known}"
    );
    assert!(
        ssh.lines()
            .any(|l| l.contains("HostKeyAlias=lnk-fake-m-2") && l.contains("ubuntu@192.0.2.12")),
        "{ssh}"
    );

    // Listed, then removed.
    let images: serde_json::Value =
        serde_json::from_str(&world.ok(&["box", "image", "list", "--cloud", "acct", "--json"]))
            .unwrap();
    assert_eq!(images[0]["name"], "web-ready");
    assert_eq!(images[0]["disk_gb"], 30);
    assert!(
        world
            .ok(&["box", "image", "list", "--cloud", "acct"])
            .contains("web-ready")
    );
    world.ok(&[
        "box",
        "image",
        "delete",
        "web-ready",
        "--cloud",
        "acct",
        "--yes",
    ]);
    let removed = world.request("image-remove");
    assert_eq!(removed["image"], "img-1");
    assert_eq!(removed["place"], "moon-1");
    assert_eq!(removed["owner"], "github-7");
    assert!(
        world
            .ok(&["box", "image", "list", "--cloud", "acct"])
            .contains("No images in acct")
    );
    let gone = world.fails(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--image",
        "web-ready",
        "--yes",
    ]);
    assert!(
        gone.contains("Couldn't find an image named web-ready in acct"),
        "{gone}"
    );
    // An image doesn't restart a box that's made.
    let made = world.fails(&["box", "create", "--on", "web-1", "--image", "web-ready"]);
    assert!(
        made.contains("Couldn't make a box in web-1, which is a box already"),
        "{made}"
    );
}

/// A box made with a deadline keeps it through a save: its timer stays
/// out of the image and is put back, at the same time, once it runs again.
#[test]
fn a_saved_box_keeps_its_deadline() {
    let Some(world) = World::new("deadline") else {
        return;
    };
    world.ok(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--name",
        "web-1",
        "--deadline",
        "4h",
        "--yes",
        "--lnk.login=false",
    ]);
    let listed: serde_json::Value =
        serde_json::from_str(&world.ok(&["box", "list", "--here", "--json"])).unwrap();
    let until = listed[0]["deadline"].as_u64().unwrap();
    std::fs::write(world.dir.join("timed"), "").unwrap();
    std::fs::remove_file(world.dir.join("ssh.log")).unwrap();
    world.ok(&["box", "image", "create", "web-1", "web-ready", "--yes"]);
    let ssh = world.read("ssh.log");
    let removed = ssh
        .find("rm -f /etc/systemd/system/lnk-deadline.service")
        .unwrap_or_else(|| panic!("{ssh}"));
    let back = ssh
        .find(&format!("'OnCalendar=@{until}'"))
        .unwrap_or_else(|| panic!("{ssh}"));
    assert!(removed < back, "{ssh}");
    assert!(
        ssh[back..].contains("systemctl enable --now lnk-deadline.timer"),
        "{ssh}"
    );
}

#[test]
fn a_box_from_an_image_fits_it_or_is_refused() {
    let Some(world) = World::new("fit") else {
        return;
    };
    let arm = SAVED.replace("x86_64", "arm64");
    std::fs::write(world.dir.join("images.json"), &arm).unwrap();
    let refused = world.fails(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--image",
        "web-ready",
        "--yes",
    ]);
    assert!(refused.contains("image web-ready is arm64"), "{refused}");
    std::fs::write(world.dir.join("images.json"), SAVED).unwrap();
    let away = world.fails(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--image",
        "web-ready",
        "--zone",
        "moon-2",
        "--yes",
    ]);
    assert!(away.contains("image web-ready is in moon-1"), "{away}");
    // A disk bigger than the image's stays as asked.
    world.ok(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--image",
        "web-ready",
        "--machine.disk.min",
        "50GB",
        "--yes",
        "--lnk.login=false",
    ]);
    assert_eq!(world.request("launch")["spec"]["disk_gb"], 50);
    assert_eq!(
        world.commands().iter().filter(|c| *c == "launch").count(),
        1
    );
}

/// What runs on a box would run on every box from its image, as itself:
/// a box with agents isn't saved.
#[test]
fn a_box_with_agents_is_not_saved() {
    let Some(world) = World::new("agents") else {
        return;
    };
    world.ok(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--name",
        "web-1",
        "--yes",
        "--lnk.login=false",
    ]);
    std::fs::write(world.dir.join("harness"), "").unwrap();
    // The harness there, but its list fails: not taken as none.
    let unsure = world.fails(&["box", "image", "create", "web-1", "web-ready", "--yes"]);
    assert!(
        unsure.contains("Asking web-1's lnk which agents are on it"),
        "{unsure}"
    );
    std::fs::write(
        world.dir.join("agents.json"),
        r#"{"agents": [{"id": "p8r2c4n6w0zt", "name": "moltbot"}], "unreached": []}"#,
    )
    .unwrap();
    let refused = world.fails(&["box", "image", "create", "web-1", "web-ready", "--yes"]);
    assert!(
        refused.contains("Couldn't save web-1, which has agents on it")
            && refused.contains("moltbot"),
        "{refused}"
    );
    assert!(!world.commands().contains(&"stop".to_string()));
    assert!(!world.dir.join("signed-out").exists());
}

/// Every installed module's credentials are refused before saving an image.
#[test]
fn a_box_with_account_credentials_is_not_saved() {
    let Some(world) = World::new("account-credentials") else {
        return;
    };
    world.ok(&[
        "box", "create", "--cloud", "acct", "--name", "web-1", "--yes",
    ]);
    std::fs::write(
        world.dir.join("held"),
        "/home/ubuntu/.lnk/safe/accounts/accounts.toml\trelay\n",
    )
    .unwrap();
    let refused = world.fails(&["box", "image", "create", "web-1", "web-ready", "--yes"]);
    assert!(
        refused.contains("holds secrets") && refused.contains("lnk's relay"),
        "{refused}"
    );
    let commands = world.commands();
    assert!(!commands.contains(&"stop".to_string()), "{commands:?}");
    assert!(
        !commands.contains(&"image-save".to_string()),
        "{commands:?}"
    );
}

/// A name no cloud takes never reaches one.
#[test]
fn an_image_name_no_cloud_takes_is_refused_before_any_cloud_call() {
    let Some(world) = World::new("badname") else {
        return;
    };
    let bad = world.fails(&["box", "image", "create", "web-1", "Web Ready", "--yes"]);
    assert!(bad.contains("can't name an image"), "{bad}");
    assert!(world.commands().is_empty(), "{:?}", world.commands());
}

/// An adapter that answers none of the image commands
/// still makes boxes.
#[test]
fn an_adapter_that_answers_no_image_commands_still_makes_boxes() {
    let Some(world) = World::new("old") else {
        return;
    };
    script(
        &world.bin.join("lnk-fake"),
        &format!(
            r#"request=$(cat); printf '%s %s\n' "$1" "$request" >> {adapter}
case "$1" in
  defaults) echo '{{"place": "moon-1", "size": "tiny"}}' ;;
  launch) echo '{{"id": "m-1"}}' ;;
  wait|start) echo '{{"ip": "192.0.2.11"}}' ;;
  state) echo '{{"state": "running"}}' ;;
  hostkeys) echo '{{"host_keys": ["ssh-ed25519 {KEY}"]}}' ;;
  image-*) echo "lnk-fake: no $1" >&2; exit 2 ;;
  *) echo '{{}}' ;;
esac"#,
            adapter = world.dir.join("adapter.log").display(),
        ),
    );
    world.ok(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--name",
        "web-1",
        "--yes",
        "--lnk.login=false",
    ]);
    let refused = world.fails(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--image",
        "web-ready",
        "--yes",
    ]);
    assert!(refused.contains("Couldn't list images"), "{refused}");
    assert_eq!(
        world.commands().iter().filter(|c| *c == "launch").count(),
        1
    );
}

/// An image saved with a hash is found by it: listed, and a box started
/// from the newest ready one with that hash. One with another hash, or
/// none, isn't.
#[test]
fn an_image_saved_with_a_hash_is_found_by_it() {
    let Some(world) = World::new("hash") else {
        return;
    };
    world.ok(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--name",
        "web-1",
        "--yes",
        "--lnk.login=false",
    ]);
    let said = world.ok(&[
        "box",
        "image",
        "create",
        "web-1",
        "web-ready",
        "--hash",
        "3f2a9c",
        "--yes",
    ]);
    assert!(!said.contains("doesn't say an image's hash"), "{said}");
    assert_eq!(
        world.request("image-save")["tags"],
        serde_json::json!({"lnk-image": "web-ready", "lnk-hash": "3f2a9c", "lnk-owner": "github-7"})
    );
    let listed: serde_json::Value = serde_json::from_str(&world.ok(&[
        "box", "image", "list", "--cloud", "acct", "--hash", "3f2a9c", "--json",
    ]))
    .unwrap();
    assert_eq!(listed[0]["name"], "web-ready");
    assert_eq!(listed[0]["hash"], "3f2a9c");
    assert_eq!(
        world.ok(&[
            "box", "image", "list", "--cloud", "acct", "--hash", "0000", "--json"
        ]),
        "[]\n"
    );
    assert!(
        world
            .said(&["box", "image", "list", "--cloud", "acct", "--hash", "0000"])
            .contains("No images with the hash 0000 in acct")
    );

    // Three with one hash: the newest ready one is the box's.
    std::fs::write(
        world.dir.join("images.json"),
        r#"{"images": [
            {"id": "img-old", "name": "app-1", "place": "moon-1", "state": "ready", "created": "2026-09-01T10:00:00Z", "hash": "3f2a9c"},
            {"id": "img-new", "name": "app-2", "place": "moon-1", "state": "ready", "created": "2026-09-20T10:00:00Z", "hash": "3f2a9c"},
            {"id": "img-saving", "name": "app-3", "place": "moon-1", "state": "pending", "created": "2026-09-30T10:00:00Z", "hash": "3f2a9c"},
            {"id": "img-other", "name": "app-4", "place": "moon-1", "state": "ready", "created": "2026-10-01T10:00:00Z", "hash": "77aa"}
        ]}"#,
    )
    .unwrap();
    let listed: serde_json::Value = serde_json::from_str(&world.ok(&[
        "box", "image", "list", "--cloud", "acct", "--hash", "3f2a9c", "--json",
    ]))
    .unwrap();
    let names: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["app-3", "app-2", "app-1"]);
    let made = world.said(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--image-hash",
        "3f2a9c",
        "--name",
        "web-2",
        "--yes",
        "--lnk.login=false",
    ]);
    assert!(made.contains("app-2 (img-new)"), "{made}");
    assert_eq!(world.request("launch")["image"], "img-new");

    // None ready with that hash: nothing made.
    let launches = world.commands().iter().filter(|c| *c == "launch").count();
    let none = world.fails(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--image-hash",
        "0000",
        "--yes",
    ]);
    assert!(
        none.contains("Couldn't find a ready image with the hash 0000 in acct"),
        "{none}"
    );
    assert_eq!(
        world.commands().iter().filter(|c| *c == "launch").count(),
        launches
    );
    // A hash no cloud takes, or both ways of naming an image, refused.
    let bad = world.fails(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--image-hash",
        "3F2A",
        "--yes",
    ]);
    assert!(bad.contains("can't be an image's hash"), "{bad}");
    let bad = world.fails(&[
        "box",
        "image",
        "create",
        "web-1",
        "web-again",
        "--hash",
        "a b",
        "--yes",
    ]);
    assert!(bad.contains("can't be an image's hash"), "{bad}");
    let both = world.fails(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--image",
        "app-1",
        "--image-hash",
        "3f2a9c",
        "--yes",
    ]);
    // The parser refuses the pair, as its declaration says.
    assert!(both.contains("cannot be used with"), "{both}");
}

/// A box holding secrets in the usual places isn't saved: an image would
/// carry them to every box made from it.
#[test]
fn a_box_holding_secrets_is_not_saved() {
    let Some(world) = World::new("secrets") else {
        return;
    };
    world.ok(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--name",
        "web-1",
        "--yes",
        "--lnk.login=false",
    ]);
    std::fs::write(
        world.dir.join("secrets"),
        ".git-credentials\tgit credentials\napp/.npmrc\tan npm token\n.aws/credentials\tthe AWS CLI's login\n",
    )
    .unwrap();
    let refused = world.fails(&["box", "image", "create", "web-1", "web-ready", "--yes"]);
    assert!(
        refused.contains("Couldn't save web-1, which holds secrets an image would carry to every box made from it"),
        "{refused}"
    );
    for found in [
        "~/.git-credentials (git credentials)",
        "~/app/.npmrc (an npm token)",
        "~/.aws/credentials (the AWS CLI's login)",
    ] {
        assert!(refused.contains(found), "{found}: {refused}");
    }
    let commands = world.commands();
    assert!(!commands.contains(&"stop".to_string()), "{commands:?}");
    assert!(
        !commands.contains(&"image-save".to_string()),
        "{commands:?}"
    );
    assert!(!world.dir.join("signed-out").exists());
    // Gone, it's saved.
    std::fs::remove_file(world.dir.join("secrets")).unwrap();
    world.ok(&["box", "image", "create", "web-1", "web-ready", "--yes"]);
}

/// What Link's modules on the box keep is theirs to say: a key one keeps
/// is found where it says, and a box whose lnk can't say isn't saved.
#[test]
fn what_modules_on_a_box_keep_is_asked_before_its_saved() {
    let Some(world) = World::new("kept") else {
        return;
    };
    world.ok(&[
        "box",
        "create",
        "--cloud",
        "acct",
        "--name",
        "web-1",
        "--yes",
        "--lnk.login=false",
    ]);
    std::fs::write(
        world.dir.join("kept.json"),
        r#"{"parts": [{"name": "buckets", "about": "keys", "paths": ["/home/me/.lnk/safe/storage"]}]}"#,
    )
    .unwrap();
    std::fs::write(
        world.dir.join("held"),
        "/home/me/.lnk/safe/storage\tbuckets\n",
    )
    .unwrap();
    let refused = world.fails(&["box", "image", "create", "web-1", "web-ready", "--yes"]);
    assert!(
        refused.contains("/home/me/.lnk/safe/storage (lnk's buckets)"),
        "{refused}"
    );
    let ssh = world.read("ssh.log");
    assert!(
        ssh.contains("held '/home/me/.lnk/safe/storage' && found"),
        "{ssh}"
    );
    // An lnk that can't say what its modules keep: not saved.
    std::fs::remove_file(world.dir.join("held")).unwrap();
    std::fs::write(world.dir.join("old-lnk"), "").unwrap();
    let refused = world.fails(&["box", "image", "create", "web-1", "web-ready", "--yes"]);
    assert!(
        refused.contains("didn't say what its modules keep"),
        "{refused}"
    );
    assert!(!world.commands().contains(&"image-save".to_string()));
    // Said, and nothing held: saved.
    std::fs::remove_file(world.dir.join("old-lnk")).unwrap();
    world.ok(&["box", "image", "create", "web-1", "web-ready", "--yes"]);
}

/// Checking a spec's attachments says so in words to a person, and only
/// `--json` prints the JSON.
#[test]
fn checked_attachments_are_said_in_words_and_json_only_with_json() {
    let Some(world) = World::new("attachments") else {
        return;
    };
    let asked = ["box", "check", "--cloud", "acct"];
    let said = world.said(&asked);
    assert!(
        said.contains("attachments the spec names are present"),
        "{said}"
    );
    assert!(!said.contains('{'), "{said}");
    let json: serde_json::Value =
        serde_json::from_str(&world.ok(&[&asked[..], &["--json"]].concat())).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"kind": "attachments", "attachments": "present"})
    );
}
