//! An environment's file: its forms, its hash, its spec and its export.
use link_env_fixture::*;

/// A file named as a word is not read: `--spec` names it.
#[test]
fn a_file_named_as_a_word_is_refused() {
    let world = World::new("old-forms");
    let old = world.run(&["env", "check", &world.env_arg()]);
    assert!(!old.status.success());
    assert!(
        text(&old.stderr).contains("--spec"),
        "{}",
        text(&old.stderr)
    );
    let neither = world.run(&["env", "check"]);
    assert!(!neither.status.success());
    assert!(
        text(&neither.stderr).contains("--spec"),
        "{}",
        text(&neither.stderr)
    );
}

#[test]
fn check_says_the_hash_which_only_what_makes_the_image_changes() {
    let world = World::new("check");
    let hash = |world: &World| {
        let out = world.run(&["env", "check", "--spec", &world.env_arg(), "--json"]);
        assert!(out.status.success(), "{}", text(&out.stderr));
        let checked: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(
            (&checked["os"], &checked["arch"]),
            (&"ubuntu-24.04".into(), &"x86_64".into())
        );
        checked["hash"].as_str().unwrap().to_string()
    };
    let first = hash(&world);
    std::fs::write(world.dir.join("project/README.md"), "changed\n").unwrap();
    assert_eq!(hash(&world), first);
    std::fs::write(world.dir.join("project/requirements.txt"), "flask==3\n").unwrap();
    assert_ne!(hash(&world), first);
    let out = world.run(&["env", "check", "--spec", &world.env_arg()]);
    let said = text(&out.stdout)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(said.contains("deps in the image in deps.toml"), "{said}");
    std::fs::remove_file(world.dir.join("project/db.toml")).unwrap();
    let out = world.run(&["env", "check", "--spec", &world.env_arg()]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("service db's sandbox db.toml isn't a file"),
        "{}",
        text(&out.stderr)
    );
}

/// `lnk env spec show` prints the file as Link reads it, each default
/// written in, and what it prints reads back as the same file.
#[test]
fn spec_prints_the_file_back_as_link_reads_it() {
    let world = World::new("spec");
    let out = world.run(&["env", "spec", "show", "--spec", &world.env_arg()]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let printed = text(&out.stdout);
    assert!(printed.contains("when = \"image\""), "{printed}");
    assert!(printed.contains("timeout = \"1m\""), "{printed}");
    let again = world.dir.join("project/again.toml");
    std::fs::write(&again, &printed).unwrap();
    let out = world.run(&[
        "env",
        "spec",
        "show",
        "--spec",
        &again.display().to_string(),
    ]);
    assert_eq!(text(&out.stdout), printed);
    let check = |path: &str| {
        let out = world.run(&["env", "check", "--spec", path, "--json"]);
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["hash"].clone()
    };
    assert_eq!(check(&again.display().to_string()), check(&world.env_arg()));
}

/// A field flag sets its key over the file as the file's own line would:
/// the same environment, the same hash, the same printed spec.
#[test]
fn a_field_flag_is_the_line_it_sets() {
    let world = World::new("field-flags");
    let env = world.env_arg();
    let run = |args: &[&str]| {
        let out = world.run(args);
        assert!(out.status.success(), "{}", text(&out.stderr));
        text(&out.stdout)
    };
    let flagged = run(&[
        "env",
        "spec",
        "show",
        "--spec",
        &env,
        "--variables.REGION",
        "eu-west-2",
    ]);
    assert!(flagged.contains("REGION = \"eu-west-2\""), "{flagged}");
    let written = world.dir.join("project/written.toml");
    std::fs::write(&written, &flagged).unwrap();
    let written = written.display().to_string();
    assert_eq!(run(&["env", "spec", "show", "--spec", &written]), flagged);
    // What a command reads is the effective spec, the flag's key in it.
    let checked = run(&[
        "env",
        "check",
        "--spec",
        &env,
        "--variables.REGION",
        "eu-west-2",
    ]);
    assert!(checked.contains("REGION"), "{checked}");
    assert!(!run(&["env", "check", "--spec", &env]).contains("REGION"));
    // A key the shape lacks is refused, naming its flag.
    let out = world.run(&["env", "check", "--spec", &env, "--colour", "red"]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("--colour"),
        "{}",
        text(&out.stderr)
    );
}

/// The environments' secrets file is declared, so an image of a home
/// holding it is refused and uninstalling names it.
#[test]
fn the_secrets_file_is_declared() {
    let world = World::new("secrets-file");
    let parts = world.run(&["uninstall", "--parts", "--json"]);
    assert!(parts.status.success(), "{}", text(&parts.stderr));
    let parts: serde_json::Value = serde_json::from_slice(&parts.stdout).unwrap();
    let part = &parts["parts"][0];
    let secrets = part["secrets"].as_array().unwrap();
    assert_eq!(secrets.len(), 1);
    assert!(
        secrets[0]
            .as_str()
            .unwrap()
            .ends_with(".lnk/safe/envs/secrets.toml"),
        "{part}"
    );
}

#[test]
fn an_environment_export_is_bounded_and_an_import_cannot_escape_or_grant_itself_trust() {
    let world = World::new("carried");
    let out = world.run(&["env", "export", "--spec", &world.env_arg()]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let packed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(packed["file"], "env.toml");
    assert!(packed["files"]["requirements.txt"].is_string());
    let import = |value: &serde_json::Value| {
        let mut child = world
            .command(&["env", "import"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(value).unwrap())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let unknown = import(&packed);
    assert!(!unknown.status.success());
    assert!(
        text(&unknown.stderr).contains("no matching reviewed copy"),
        "{}",
        text(&unknown.stderr)
    );
    for path in ["../escape", "/escape"] {
        let mut changed = packed.clone();
        changed["files"][path] = "planted".into();
        let out = import(&changed);
        assert!(!out.status.success(), "{path}");
        assert!(
            text(&out.stderr).contains("relative"),
            "{}",
            text(&out.stderr)
        );
    }
    let mut changed = packed.clone();
    changed["files"]["unasked-for"] = "planted".into();
    let out = import(&changed);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("only the files it names"),
        "{}",
        text(&out.stderr)
    );
    assert!(!world.dir.join("home/.lnk/safe/envs/envs/app").exists());
}
