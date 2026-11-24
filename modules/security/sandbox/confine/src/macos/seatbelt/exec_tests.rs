//! Which programs a Seatbelt profile lets run: the shell macOS's
//! `/bin/sh` runs, the developer tools, and folders written with a final `/`.

use super::*;

fn listing(programs: Vec<PathBuf>, developer_tools: bool) -> String {
    profile(
        &Common {
            commands: Some(programs),
            ..Default::default()
        },
        &Macos {
            developer_tools,
            ..Default::default()
        },
    )
}

/// The shell `/private/var/select/sh` names is the one macOS's
/// `/bin/sh` runs: the link resolved, and none without a link.
#[test]
fn the_selected_shell_is_the_links_target() {
    let root = link_test_kit::TempDir::new("select-sh");
    let bash = root.path().join("bash");
    std::fs::write(&bash, "").unwrap();
    let selected = root.path().join("sh");
    std::os::unix::fs::symlink(&bash, &selected).unwrap();
    assert_eq!(variant(&selected), Some(bash.canonicalize().unwrap()));
    assert_eq!(variant(&bash), None, "a file, not a link");
    assert_eq!(variant(&root.path().join("none")), None);
}

/// A folder given with a final `/` is matched without it, for reading
/// and running; the root stays `/`.
#[test]
fn a_folder_with_a_final_slash_matches_below_it() {
    let root = link_test_kit::TempDir::new("final-slash");
    let given = PathBuf::from(format!("{}/", root.path().display()));
    let profile = listing(vec![given], false);
    let bare = format!("(subpath {})", literal(&root.path().display().to_string()));
    let exec = &profile[profile.find("(allow process-exec").unwrap()..];
    assert!(exec.contains(&bare), "{exec}");
    assert!(
        !profile.contains(&format!("{}/\"", root.path().display())),
        "{profile}"
    );
    assert_eq!(folder(Path::new("/")), "/");
}

/// With `developer_tools`, the developer tools' programs run where
/// `[commands]` lists programs; without it, they don't.
#[test]
fn developer_tools_run_only_when_opened() {
    let git = vec![PathBuf::from("/usr/bin/git")];
    // The rule's own lines: up to the next rule, which starts a line.
    let exec = |p: &str| {
        let rule = &p[p.find("(allow process-exec").unwrap()..];
        rule[..rule.find("\n(").unwrap_or(rule.len())].to_string()
    };
    let opened = exec(&listing(git.clone(), true));
    assert!(opened.contains("(literal \"/usr/bin/git\")"), "{opened}");
    assert!(
        opened.contains("(subpath \"/Library/Developer\")"),
        "{opened}"
    );
    assert!(
        opened.contains(r#"(regex #"^/Applications/Xcode[^/]*\.app/")"#),
        "{opened}"
    );
    let closed = exec(&listing(git, false));
    assert!(!closed.contains("/Library/Developer"), "{closed}");
}
