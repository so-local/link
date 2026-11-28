//! What `lnk storage` says and how it exits where no cloud is needed:
//! a list that's empty, a mistake in what was typed, a JSON reply, and
//! the yes asked before a bucket is made, of a stand-in rclone.

link_test_kit::program!(system);

use link_test_kit::{TempDir, env, write_program};

fn storage(home: &TempDir, args: &[&str]) -> std::process::Output {
    let program = std::env::var_os("LNK_TEST_STORAGE_PROGRAM")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_lnk-storage").into());
    env::command(program, home).args(args).output().unwrap()
}

fn said(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn no_clouds_is_a_list_that_says_how_to_make_one() {
    let home = TempDir::new("storage-empty");
    let out = storage(&home, &["list"]);
    assert!(out.status.success(), "{}", said(&out.stderr));
    assert!(said(&out.stdout).contains("No clouds are connected yet."));
    assert!(said(&out.stderr).contains("lnk storage create <type>"));
    let json = storage(&home, &["list", "--json"]);
    assert_eq!(said(&json.stdout).trim(), "[]");
}

/// Each bucket is listed by its ID, `bucket:<id>`, beside its name; a
/// file with a bucket that has none is refused, naming how to fix it.
#[test]
fn a_bucket_is_listed_by_its_id() {
    let home = TempDir::new("storage-bucket-id");
    let safe = home.join(".lnk/safe/storage");
    std::fs::create_dir_all(&safe).unwrap();
    std::fs::create_dir_all(home.join("Link/Files")).unwrap();
    let disk = format!(
        "schema = 1\n[[cloud]]\nname = \"disk\"\nkind = \"drive\"\npath = \"{0}/disk\"\n\
         [cloud.buckets.personal]\nid = \"a1b2c3d4e5f6\"\nname = \"local-link/personal\"\n\n",
        home.display()
    );
    std::fs::write(safe.join("clouds.toml"), &disk).unwrap();
    let json = storage(&home, &["list", "--json"]);
    assert!(json.status.success(), "{}", said(&json.stderr));
    let listed: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    link_test_kit::ids::assert_named(&listed);
    let bucket = &listed[0]["buckets"][0];
    assert_eq!(bucket["resource"], "bucket:a1b2c3d4e5f6", "{listed}");
    assert_eq!(bucket["bucket"], "local-link/personal");
    assert!(bucket["owner"].is_null(), "{listed}");
    for bucket in ["", "id = \"old\"\n"] {
        std::fs::write(
            safe.join("clouds.toml"),
            format!(
                "{disk}[[cloud]]\nname = \"old\"\nkind = \"drive\"\npath = \"{0}/old\"\n\
                 [cloud.buckets.personal]\n{bucket}name = \"local-link/personal\"\n",
                home.display()
            ),
        )
        .unwrap();
        let refused = storage(&home, &["list"]);
        assert!(!refused.status.success());
        let why = said(&refused.stderr);
        assert!(
            why.contains("remove the [[cloud]] entry named old"),
            "{why}"
        );
    }
}

#[test]
fn a_mistake_exits_with_the_usage_code_and_a_next_on_stderr() {
    let home = TempDir::new("storage-usage");
    for (args, text) in [
        (vec!["delete", "../x"], "is no cloud's name"),
        (vec!["disconnect", "nope"], "No cloud nope is connected"),
        (vec!["show", "nope"], "No cloud nope is connected"),
        (vec!["keys", "--owner", "notanid"], "isn't an owner's ID"),
        (
            vec!["leave", "--owner", "x:Not-An-Id"],
            "isn't an owner's ID",
        ),
        (
            vec!["create", "aws", "--path", "/x"],
            "--path is for a drive cloud",
        ),
    ] {
        let out = storage(&home, &args);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{args:?}: {}",
            said(&out.stderr)
        );
        assert!(out.stdout.is_empty(), "{args:?}");
        let err = said(&out.stderr);
        assert!(err.contains(text), "{args:?}: {err}");
        assert!(err.contains("Next"), "{args:?} has no next: {err}");
    }
}

#[test]
fn what_a_program_reads_stays_json_on_stdout() {
    let home = TempDir::new("storage-json");
    let found = storage(&home, &["find"]);
    assert!(found.status.success(), "{}", said(&found.stderr));
    assert_eq!(said(&found.stdout).trim(), "[]");
}

/// A `lnk` that lists one module, `shelf`, keeping owners of the kind
/// `job` and nothing else: no module keeps an `agent`. Its program,
/// `lnk-shelf`, is beside it.
fn shelf(home: &TempDir) -> std::path::PathBuf {
    let jobs = home.join("Jobs/build");
    std::fs::create_dir_all(&jobs).unwrap();
    let lnk = home.join("lnk");
    write_program(
        &lnk,
        r#"#!/bin/sh
case "$*" in
  "module list --json")
    echo '[{"name": "shelf", "installed": true, "answers": ["storage.folders", "storage.owner"]}]' ;;
  *) exit 1 ;;
esac
"#,
    )
    .unwrap();
    write_program(
        &home.join("lnk-shelf"),
        format!(
            r#"#!/bin/sh
case "$*" in
  "--link-answer storage.folders")
    cat > /dev/null
    echo '{{"protocol": 1, "answer": {{"owned": [{{"owner": "job:k3v9q2x7m1ab", "name": "build", "root": "{jobs}", "running": false}}]}}}}' ;;
  "--link-answer storage.owner")
    read -r asked
    case "$asked" in
      *'"kind":"job"'*'"name":"build"'*|*'"kind":"job"'*'"name":"k3v9q2x7m1ab"'*)
        found='{{"here": {{"owner": "job:k3v9q2x7m1ab", "name": "build"}}}}' ;;
      *'"kind":"job"'*) found='"none"' ;;
      *) found='"unkept"' ;;
    esac
    echo '{{"protocol": 1, "answer": '"$found"'}}' ;;
  *) exit 1 ;;
esac
"#,
            jobs = jobs.display()
        ),
    )
    .unwrap();
    lnk
}

fn storage_with(home: &TempDir, lnk: &std::path::Path, args: &[&str]) -> std::process::Output {
    let program = std::env::var_os("LNK_TEST_STORAGE_PROGRAM")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_lnk-storage").into());
    env::command(program, home)
        .env("LNK_EXE", lnk)
        .args(args)
        .output()
        .unwrap()
}

/// With no agent module, only another module keeping owners of its own
/// kind, `lnk storage` lists, finds, exports and lets go of that kind's
/// folders by their IDs, and refuses a kind no module keeps, saying so.
#[test]
fn owners_of_any_kind_work_with_no_agent_module() {
    let home = TempDir::new("storage-owners");
    let lnk = shelf(&home);
    let run = |args: &[&str]| storage_with(&home, &lnk, args);
    let listed = run(&["list", "--json"]);
    assert!(listed.status.success(), "{}", said(&listed.stderr));
    assert_eq!(said(&listed.stdout).trim(), "[]");
    let tree = run(&["list", "--tree", "--json"]);
    assert!(tree.status.success(), "{}", said(&tree.stderr));
    let tree: serde_json::Value = serde_json::from_slice(&tree.stdout).unwrap();
    assert!(tree.get("job:k3v9q2x7m1ab").is_some(), "{tree}");
    let exported = run(&["export", "--owner", "job:k3v9q2x7m1ab"]);
    assert!(exported.status.success(), "{}", said(&exported.stderr));
    assert_eq!(said(&exported.stdout).trim(), "null");
    let left = run(&["leave", "--owner", "job:k3v9q2x7m1ab"]);
    assert!(left.status.success(), "{}", said(&left.stderr));
    let found = run(&["find", "--owner", "job:build"]);
    assert!(found.status.success(), "{}", said(&found.stderr));
    assert_eq!(said(&found.stdout).trim(), "[]");
    for (owner, says) in [
        (
            "agent:main",
            "no module here keeps owners of the kind agent",
        ),
        ("job:other", "no job named other"),
    ] {
        let refused = run(&[
            "connect",
            "drive",
            "--path",
            "/nowhere",
            "--bucket",
            "b",
            "--owner",
            owner,
            "--owner-bucket",
            "c",
        ]);
        assert_eq!(refused.status.code(), Some(2), "{}", said(&refused.stderr));
        assert!(
            said(&refused.stderr).contains(says),
            "{}",
            said(&refused.stderr)
        );
    }
}

/// A `lnk` that lists one module, `broken`, which answers
/// `storage.folders` by failing, saying why on stderr.
fn broken(home: &TempDir) -> std::path::PathBuf {
    let lnk = home.join("lnk");
    write_program(
        &lnk,
        r#"#!/bin/sh
case "$*" in
  "module list --json")
    echo '[{"name": "broken", "installed": true, "answers": ["storage.folders"]}]' ;;
  *) exit 1 ;;
esac
"#,
    )
    .unwrap();
    write_program(
        &home.join("lnk-broken"),
        r#"#!/bin/sh
cat > /dev/null
echo "starting" >&2
echo "Error: its settings.toml is missing" >&2
exit 3
"#,
    )
    .unwrap();
    lnk
}

/// Your bucket and an owner's whose module isn't here, in one cloud.
fn two_buckets(home: &TempDir) {
    let safe = home.join(".lnk/safe/storage");
    std::fs::create_dir_all(&safe).unwrap();
    std::fs::create_dir_all(home.join("Link/Files")).unwrap();
    std::fs::write(
        safe.join("clouds.toml"),
        format!(
            "schema = 1\n[[cloud]]\nname = \"disk\"\nkind = \"drive\"\npath = \"{}/disk\"\n\
             [cloud.buckets.personal]\nid = \"p1e2r3s4o5n6\"\nname = \"local-link/personal\"\n\
             [cloud.buckets.\"agent:k3v9q2x7m1ab\"]\nid = \"a1g2e3n4t5b6\"\nname = \"local-link/agent-k3v9q2x7m1ab\"\nencrypted = true\n",
            home.display()
        ),
    )
    .unwrap();
}

/// Listing and showing clouds reads only their settings: a module that
/// fails to say which folders it keeps doesn't stop them, and an owner's
/// bucket is listed by its ID whether or not its module is here.
#[test]
fn clouds_are_listed_whatever_a_module_answers() {
    let home = TempDir::new("storage-list-broken");
    let lnk = broken(&home);
    two_buckets(&home);
    let run = |args: &[&str]| storage_with(&home, &lnk, args);
    let listed = run(&["list", "--json"]);
    assert!(listed.status.success(), "{}", said(&listed.stderr));
    let listed: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    let buckets = &listed[0]["buckets"];
    assert_eq!(buckets.as_array().map(Vec::len), Some(2), "{listed}");
    assert!(buckets[0]["owner"].is_null(), "{listed}");
    assert_eq!(
        buckets[1]["owner"],
        serde_json::json!({"workload": "agent:k3v9q2x7m1ab", "name": "agent:k3v9q2x7m1ab"}),
        "{listed}"
    );
    let table = run(&["list"]);
    assert!(table.status.success(), "{}", said(&table.stderr));
    assert!(
        said(&table.stdout).contains("agent:k3v9q2x7m1ab's files"),
        "{}",
        said(&table.stdout)
    );
    let shown = run(&["show", "disk"]);
    assert!(shown.status.success(), "{}", said(&shown.stderr));
    assert!(said(&shown.stdout).contains("agent:k3v9q2x7m1ab's files"));
}

/// A command that touches your files goes on without a module that fails
/// to say which folders it keeps, warning of it with what it said.
#[test]
fn a_failed_answer_is_a_warning_naming_the_module_and_why() {
    let home = TempDir::new("storage-tree-broken");
    let lnk = broken(&home);
    two_buckets(&home);
    let tree = storage_with(&home, &lnk, &["list", "--tree", "--json"]);
    let err = said(&tree.stderr);
    assert!(tree.status.success(), "{err}");
    let tree: serde_json::Value = serde_json::from_slice(&tree.stdout).unwrap();
    assert!(tree.get("personal").is_some(), "{tree}");
    assert!(
        err.contains("broken") && err.contains("its settings.toml is missing"),
        "{err}"
    );
}

/// A person's `list` and `show` are readouts, never JSON: each cloud by
/// its name, each folder it holds, and whether its bucket is encrypted;
/// `--json` is the reply programs read.
#[test]
fn list_and_show_are_readouts_for_a_person() {
    let home = TempDir::new("storage-readout");
    two_buckets(&home);
    let listed = storage(&home, &["list"]);
    assert!(listed.status.success(), "{}", said(&listed.stderr));
    let text = said(&listed.stdout);
    assert!(!text.trim_start().starts_with(['[', '{']), "{text}");
    for says in [
        "disk (drive at",
        "Your files",
        "agent:k3v9q2x7m1ab's files",
        "local-link/personal",
        "rclone crypt",
        "off",
    ] {
        assert!(text.contains(says), "{says}: {text}");
    }
    let shown = storage(&home, &["show", "disk"]);
    assert!(shown.status.success(), "{}", said(&shown.stderr));
    let text = said(&shown.stdout);
    assert!(!text.trim_start().starts_with(['[', '{']), "{text}");
    for says in [
        "Name",
        "disk",
        "Your files",
        "agent:k3v9q2x7m1ab's files",
        "Encryption",
        "rclone crypt, file and folder names too",
        "Password",
    ] {
        assert!(text.contains(says), "{says}: {text}");
    }
    let json = storage(&home, &["list", "--json"]);
    let listed: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(listed[0]["name"], "disk", "{listed}");
    assert_eq!(listed[0]["buckets"][1]["encrypted"], true, "{listed}");
}

/// A stand-in rclone, `rclone` in `home`, writing each run's arguments to
/// `rclone.log`: storage of the rclone type `b2`, which holds buckets.
fn bucket_rclone(home: &TempDir) -> std::path::PathBuf {
    let rclone = home.join("rclone");
    write_program(
        &rclone,
        format!(
            r#"#!/bin/sh
echo "$*" >> "{log}"
case "$*" in
  *"config providers"*) echo '[{{"Name": "b2", "Options": []}}]' ;;
  *"backend features"*) echo '{{"Features": {{"BucketBased": true}}}}' ;;
esac
"#,
            log = home.join("rclone.log").display()
        ),
    )
    .unwrap();
    rclone
}

/// A bucket is billed by its cloud, so `lnk storage create` and the first
/// `lnk storage push` to a cloud make one only after a yes: with no
/// terminal to ask and no `--yes`, each makes nothing and names `--yes`;
/// with it, the bucket is made.
#[test]
fn a_bucket_is_made_only_after_a_yes() {
    let home = TempDir::new("storage-billed");
    let rclone = bucket_rclone(&home);
    let run = |args: &[&str]| {
        let program = std::env::var_os("LNK_TEST_STORAGE_PROGRAM")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_lnk-storage").into());
        env::command(program, &home)
            .env("LNK_RCLONE", &rclone)
            .args(args)
            .output()
            .unwrap()
    };
    let made = || {
        std::fs::read_to_string(home.join("rclone.log"))
            .unwrap_or_default()
            .lines()
            .any(|line| line.contains(" mkdir b2:"))
    };
    let create = ["create", "b2", "--setting", "account=x"];
    let refused = run(&create);
    assert_eq!(refused.status.code(), Some(2), "{}", said(&refused.stderr));
    let says = said(&refused.stderr);
    assert!(
        says.contains("Make a bucket for your files in b2, billed by that cloud?")
            && says.contains("--yes"),
        "{says}"
    );
    assert!(!made(), "a bucket was made without a yes");
    run(&[&create[..], &["--yes"]].concat());
    assert!(made(), "--yes makes the bucket");

    // A cloud with no bucket for your files yet: the first push makes it.
    std::fs::remove_file(home.join("rclone.log")).unwrap();
    let safe = home.join(".lnk/safe/storage");
    std::fs::write(
        safe.join("clouds.toml"),
        "schema = 1\n[[cloud]]\nname = \"b2\"\nkind = \"rclone\"\nbackend = \"b2\"\n",
    )
    .unwrap();
    std::fs::write(safe.join("rclone.conf"), "[b2]\ntype = b2\n").unwrap();
    std::fs::create_dir_all(home.join("Link/Files")).unwrap();
    std::fs::write(home.join("Link/Files/a.txt"), "a\n").unwrap();
    let file = home.join("Link/Files/a.txt").display().to_string();
    let refused = run(&["push", &file]);
    assert_eq!(refused.status.code(), Some(2), "{}", said(&refused.stderr));
    let says = said(&refused.stderr);
    assert!(
        says.contains("billed by that cloud") && says.contains("--yes"),
        "{says}"
    );
    assert!(!made(), "a bucket was made without a yes");
    let pushed = run(&["push", &file, "--yes"]);
    assert!(made(), "{}", said(&pushed.stderr));
}

/// A Keychain that can't keep an encrypted bucket's password (locked, or
/// asking for a person) fails `lnk storage create`: no bucket is made, and
/// the password is in no file.
#[test]
#[cfg(unix)]
fn a_failing_keychain_fails_create_and_keeps_no_file() {
    let home = TempDir::new("storage-refusing");
    let rclone = home.join("rclone");
    write_program(
        &rclone,
        format!(
            r#"#!/bin/sh
echo "$*" >> "{log}"
case "$*" in
  *"config providers"*) echo '[{{"Name": "b2", "Options": []}}]' ;;
  *"backend features"*) echo '{{"Features": {{"BucketBased": true}}}}' ;;
  *obscure*) cat > /dev/null; echo OBSCURED-PASSWORD ;;
esac
"#,
            log = home.join("rclone.log").display()
        ),
    )
    .unwrap();
    let refusing = link_test_kit::keychain::refusing(&home.join("keychain"));
    let program = std::env::var_os("LNK_TEST_STORAGE_PROGRAM")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_lnk-storage").into());
    let out = env::command(program, &home)
        .env("LNK_RCLONE", &rclone)
        .env("LNK_SECURITY", &refusing)
        .args(["create", "b2", "--setting", "account=x"])
        .args(["--encrypt", "--yes"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{}", said(&out.stdout));
    assert!(
        said(&out.stderr).contains("Keychain"),
        "{}",
        said(&out.stderr)
    );
    let log = std::fs::read_to_string(home.join("rclone.log")).unwrap_or_default();
    assert!(!log.lines().any(|l| l.contains(" mkdir ")), "{log}");
    let conf =
        std::fs::read_to_string(home.join(".lnk/safe/storage/rclone.conf")).unwrap_or_default();
    assert!(!conf.contains("OBSCURED-PASSWORD"), "{conf}");
}
