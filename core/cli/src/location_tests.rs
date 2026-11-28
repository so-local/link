//! Startup and failure reporting must not recreate a moved Link folder.

use std::path::Path;
use std::process::{Output, Stdio};

fn run(home: &Path, args: &[&str]) -> Output {
    link_test_kit::env::command(link_test_kit::programs().join("lnk"), home)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn a_refused_command_keeps_a_moved_folder_absent_until_it_is_selected() {
    let home = link_test_kit::TempDir::new("folder-relocation");
    let created = run(&home, &["location", "create"]);
    assert!(created.status.success(), "{}", said(&created));
    let original = home.join("Link");
    let settings = original.join("Safe/Agents/work");
    std::fs::create_dir_all(&settings).unwrap();
    std::fs::write(settings.join("agent.toml"), "harness = 'mini'\n").unwrap();
    let moved = home.join("moved");
    std::fs::rename(&original, &moved).unwrap();
    let refused = run(&home, &["up"]);
    assert!(!refused.status.success());
    assert!(
        said(&refused).contains("lnk location set link"),
        "{}",
        said(&refused)
    );
    assert!(
        !original.exists(),
        "failure reporting recreated the lost folder"
    );
    let machine = link_files::layout::Hidden::Settings.at_default(&home.join(".lnk"));
    let saved = std::fs::read(&machine).unwrap();
    std::fs::write(&machine, "not = [valid TOML").unwrap();
    let failed = run(&home, &["up"]);
    assert!(!failed.status.success());
    assert!(!original.exists(), "a bug record recreated the lost folder");
    std::fs::write(&machine, &saved).unwrap();
    let selected = run(&home, &["location", "set", "link", moved.to_str().unwrap()]);
    assert!(selected.status.success(), "{}", said(&selected));
    assert!(!original.exists());
    assert_eq!(
        std::fs::read_to_string(moved.join("Safe/Agents/work/agent.toml")).unwrap(),
        "harness = 'mini'\n"
    );
    std::fs::write(&machine, "not = [valid TOML").unwrap();
    let refused = run(&home, &["up"]);
    assert!(!refused.status.success());
    assert!(
        std::fs::read_dir(moved.join("Logs")).unwrap().any(|entry| {
            entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|ext| ext == "json")
        }),
        "an existing Link folder still keeps failure records"
    );
}

#[test]
fn a_migration_with_no_install_preserves_the_home_and_creates_no_agent() {
    let home = link_test_kit::TempDir::new("migration-no-install");
    let document = home.join("kept.txt");
    std::fs::write(&document, "original bytes").unwrap();
    let refused = run(&home, &["agent", "migrate", "--yes"]);
    assert!(!refused.status.success());
    assert!(
        said(&refused).contains("Found no install"),
        "{}",
        said(&refused)
    );
    assert_eq!(std::fs::read(document).unwrap(), b"original bytes");
    assert!(!home.join("Link").exists(), "refusal created a Link folder");
    assert!(
        !home.join(".lnk/safe/agents/journal").exists(),
        "refusal began a migration journal"
    );
}

#[test]
fn inspecting_a_spec_creates_neither_control_state_nor_a_link_folder() {
    for existing in [false, true] {
        let home = link_test_kit::TempDir::new("spec-first-need");
        if existing {
            std::fs::create_dir(home.join("Link")).unwrap();
            std::fs::write(home.join("Link/kept.txt"), "user bytes").unwrap();
        }
        for args in [
            &["sandbox", "spec", "show"][..],
            &[
                "sandbox",
                "spec",
                "show",
                "--spec.name",
                "first-need",
                "--spec.version",
                "1",
            ][..],
        ] {
            let output = run(&home, args);
            assert!(output.status.success(), "{}", said(&output));
            assert!(
                !home.join(".lnk").exists(),
                "inspection created machine state"
            );
            assert!(
                !home.join("Link/Safe").exists(),
                "inspection created control state"
            );
            if !existing {
                assert!(!home.join("Link").exists());
            } else {
                assert_eq!(
                    std::fs::read(home.join("Link/kept.txt")).unwrap(),
                    b"user bytes"
                );
            }
        }
    }
}
