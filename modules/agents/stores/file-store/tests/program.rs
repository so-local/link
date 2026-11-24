//! `lnk-file-store`'s own screens, as a person or a script reads them: its
//! version and help on stdout, and a command it doesn't have a usage error
//! that says where its help is.

link_test_kit::program!(system);

use link_test_kit::TempDir;

fn run(args: &[&str]) -> std::process::Output {
    let dir = TempDir::new("file-program");
    link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-file-store"), &dir)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn it_prints_its_version_and_help_on_stdout_alone() {
    let version = run(&["--version"]);
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout),
        format!("lnk-file-store {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(version.stderr.is_empty());
    let help = run(&["help"]);
    assert!(help.status.success());
    let help_text = String::from_utf8_lossy(&help.stdout);
    // Shown as a person types it, under `lnk`: the program never reads
    // its own namespace.
    assert!(help_text.contains("Usage: lnk file-store"), "{help_text}");
    assert!(help_text.contains("--help"), "{help_text}");
    assert!(help.stderr.is_empty());
}

#[test]
fn another_command_is_a_usage_error_that_points_to_its_help() {
    let said = run(&["bogus"]);
    assert_eq!(said.status.code(), Some(2));
    assert!(said.stdout.is_empty());
    let err = String::from_utf8_lossy(&said.stderr);
    assert!(err.contains("unrecognized subcommand 'bogus'"), "{err}");
    assert!(err.contains("--help"), "{err}");
}
