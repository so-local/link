//! Link's persistent private state uses `.lnk`, in the person's or a
//! harness's home, and nowhere else: this workspace's Rust, pages,
//! scripts and settings (the rule is `link_repository_check::state_roots`,
//! which the private workspace runs on its own root too).

use std::path::Path;

use link_repository_check::state_roots::{against, check, rust as violations, text};

/// The files and folders of Link's still made outside `.lnk`, each
/// `<file>: <name>`, from a workspace's `state-roots.toml`.
fn kept(path: &Path) -> Vec<String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct File {
        kept: Vec<String>,
    }
    let text = std::fs::read_to_string(path).unwrap();
    toml::from_str::<File>(&text).unwrap().kept
}

#[test]
fn persistent_state_has_one_private_root() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    // The test kit creates isolated fixture homes, not installed user state.
    let found = check(
        &root,
        &["core", "packages", "modules", "interop"],
        &["packages/test-kit"],
    );
    let problems = against(
        &found,
        &kept(&root.join("tests/repository/state-roots.toml")),
    );
    assert!(
        problems.is_empty(),
        "persistent Link state belongs under .lnk:\n{}",
        problems.join("\n")
    );
}

#[test]
fn a_written_path_names_no_hidden_folder_beside_lnk() {
    let file = Path::new("docs/page.md");
    for wrong in [
        "LNK_TEST_STATE_DIR=/root/.lnk-test",
        "export LNK_TEST_STATE_DIR=\"$HOME/.lnk-test-runtime\"",
        "It runs in `~/.lnk-extra`.",
        "CARGO_HOME=/workspace/.link-env/cargo",
        "a home of `/home/me/.link`",
        "`${HOME}/.link-example`",
    ] {
        assert_eq!(text(file, wrong).len(), 1, "{wrong}");
    }
    for right in [
        "LNK_TEST_STATE_DIR=/root/.lnk/tests",
        "`~/.lnk/safe/agents/main`",
        "the socket `~/.openclaw/.link-store.sock`",
        "a backup beside it, `$HOME/.claude/Backups-{n}`",
        "`/var/lib/lnk/data` as root",
        "the `link-agent-contract` crate",
        "`~/.zshrc` and `~/.ssh`",
    ] {
        assert!(text(file, right).is_empty(), "{right}");
    }
}

#[test]
fn a_new_root_fails_even_in_the_state_file() {
    let file = Path::new("packages/files/src/state.rs");
    assert!(violations(file, r#"home.join(".lnk/exec/harnesses/example")"#).is_empty());
    assert_eq!(violations(file, r#"home.join(".link-example")"#).len(), 1);
    assert_eq!(violations(file, r#"home.join(".link-new")"#).len(), 1);
    assert_eq!(
        violations(
            Path::new("module/src/main.rs"),
            r#"home.join(".link-example")"#
        )
        .len(),
        1
    );
    assert_eq!(violations(file, r#"home.join(".surprise")"#).len(), 1);
    assert_eq!(violations(file, r#"home().join(".lnk-extra")"#).len(), 1);
    assert_eq!(
        violations(file, r#"Trace::new(Kind::Home, "~/.link-other")"#).len(),
        1
    );
    assert_eq!(
        violations(file, "home_dir()\n .join(\".new-state\")").len(),
        1
    );
    assert_eq!(violations(file, r##"home.join(r#".new-state"#)"##).len(), 1);
    assert_eq!(
        violations(file, r#"home.join(format!(".private-{id}"))"#).len(),
        1
    );
}

#[test]
fn comments_fixtures_native_paths_and_ephemeral_files_are_not_state_roots() {
    let file = Path::new("module/src/main.rs");
    for source in [
        r#"home.join(".lnk/exec/harnesses/example")"#,
        r#"home.join(".aws/config")"#,
        r#"home.join(".config/systemd/user")"#,
        r#"home.join(format!("lnk-check-{tag}"))"#,
        r#"const TMP: &str = "lnk-tmp-";"#,
        r#"machine.join(".lnk/safe/agents/stopped-services/.config/systemd/user/x.service")"#,
        r#"args(["labels.lnk-owner:*"])"#,
        r#"let url = "https://local.link/example";"#,
        "// home.join(\".link-no\")\n/* \"~/.link-no\" */",
        "#[cfg(test)]\nmod tests { let home = \"~/.link-old\"; }",
    ] {
        assert!(violations(file, source).is_empty(), "{source}");
    }
    assert_eq!(violations(file, "#[cfg(test)]\nmod tests { let x = \"~/.link-old\"; }\nfn outside() { home.join(\".link-new\"); }").len(), 1);
    assert_eq!(
        violations(file, r#"let quote = b'"'; home.join(".new-state");"#).len(),
        1
    );
}

#[test]
fn native_adapter_paths_are_recognized_below_their_workspace() {
    let source = r#"root.join(".vendor.json")"#;
    for file in [
        "interop/agents/harnesses/example/src/main.rs",
        "workspace/interop/agents/harnesses/example/src/main.rs",
    ] {
        assert!(violations(Path::new(file), source).is_empty(), "{file}");
    }
    assert_eq!(violations(Path::new("module/src/main.rs"), source).len(), 1);
}

#[test]
fn a_file_or_folder_of_links_outside_lnk_fails() {
    let file = Path::new("module/src/main.rs");
    for wrong in [
        r#"prefix.join(".lnk-tools.json")"#,
        r#"const OFF: &str = ".lnk-off";"#,
        r#"const BACKUP: &str = ".lnk-backup";"#,
        r#"home.join(".openclaw/.link-store.sock")"#,
    ] {
        assert_eq!(violations(file, wrong).len(), 1, "{wrong}");
    }
}

#[test]
fn the_kept_list_may_shrink_never_grow() {
    let found = vec![
        "a/src/main.rs: .lnk-off is a file or folder of Link's outside .lnk".to_string(),
        "b/src/main.rs:3: .link-x is a private root outside .lnk".to_string(),
    ];
    let listed = vec![
        "a/src/main.rs: .lnk-off".to_string(),
        "c/src/main.rs: .lnk-backup".to_string(),
    ];
    let problems = against(&found, &listed);
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems[0].contains(".link-x is a private root"),
        "{problems:?}"
    );
    assert!(
        problems[1].contains("c/src/main.rs: .lnk-backup: no longer made"),
        "{problems:?}"
    );
}

#[test]
fn hidden_children_and_temporary_names_have_no_exemption() {
    let file = Path::new("modules/agents/example/src/lib.rs");
    for source in [
        r#"agents().join(".moving")"#,
        r#"root.join(".unexpected")"#,
        r#"dir.join(format!(".lnk-install-{id}"))"#,
        r#"home.join("Link/Agents/.memory")"#,
    ] {
        assert!(!violations(file, source).is_empty(), "{source}");
    }
    for source in [
        r#"home.join(".lnk/safe/agents/main")"#,
        r#"dir.join(format!("lnk-install-{id}"))"#,
        r#"dir.join("Config/settings.toml")"#,
    ] {
        assert!(violations(file, source).is_empty(), "{source}");
    }
}

#[test]
fn fixed_paths_are_selected_from_the_shared_catalogue() {
    let file = Path::new("modules/example/src/lib.rs");
    for source in [
        r#"link_files::installation_dir().join("new")"#,
        r#"link_core::try_link_dir()?.join("Elsewhere")"#,
    ] {
        assert!(!violations(file, source).is_empty(), "{source}");
    }
    for source in [
        r#"link_files::layout::Hidden::Agents.path().join(name)"#,
        r#"link_files::layout::Home::Teams.try_path()?.join(name)"#,
    ] {
        assert!(violations(file, source).is_empty(), "{source}");
    }
}
