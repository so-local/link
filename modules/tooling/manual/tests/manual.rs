//! The manual reads local answers and never installs a missing component.

link_test_kit::program!(system);

use link_test_kit::{TempDir, copy_program, write_program};
use std::{path::Path, process::Command};

fn descriptor(markdown: &str) -> serde_json::Value {
    serde_json::json!({
        "protocol":1,
        "component":{"id":"example","cargo_name":"link-example","program":"lnk-example","kind":"harness","version":"1.0.0","license":"MIT","description":"An example adapter"},
        "commands":{"health":{"help":"Check the example engine.","environment":{"EXAMPLE_ENDPOINT":{"type":"string","help":"The configured service endpoint."}}}},
        "pages":[{"path":"agents/harnesses/example","markdown":markdown}],
        "documentation":{"topic":"agents/harnesses/example","root":"docs"}
    })
}

fn fixture(name: &str) -> TempDir {
    let dir = TempDir::new(name);
    copy_program(
        Path::new(env!("CARGO_BIN_EXE_lnk-manual")),
        &dir.join("lnk-manual"),
    )
    .unwrap();
    let registry = serde_json::json!({"example":descriptor("")});
    write_program(&dir.join("lnk"),format!("#!/bin/sh\ncase \"$1\" in\n--link-registry) cat <<'REGISTRY'\n{registry}\nREGISTRY\n;;\n--link-documentation) echo '[]' ;;\n*) echo 'unexpected provisioning request' >&2; exit 99 ;;\nesac\n")).unwrap();
    dir
}

#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn read(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(dir.join("lnk-manual"))
        .args(args)
        .env_clear()
        .env("HOME", dir)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(dir))
        .env("PATH", "/usr/bin:/bin")
        .env("LNK_EXE", dir.join("lnk"))
        .envs(link_test_kit::keychain::isolated(dir))
        .output()
        .unwrap()
}

#[test]
fn missing_content_returns_install_information_and_reference_without_provisioning() {
    let dir = fixture("manual-missing");
    let result = read(&dir, &["agents/harnesses/example"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.contains("lnk module add example"));
    assert!(text.contains("https://docs.local.link/agents/harnesses/example"));
    assert!(!dir.join("lnk-example").exists());
    let result = read(&dir, &["agents/harnesses/example", "--commands"]);
    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("Check the example engine"));
    let result = read(&dir, &["agents/harnesses/example", "--variables"]);
    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("EXAMPLE_ENDPOINT"));
    assert!(!dir.join("lnk-example").exists());
}

#[test]
fn installed_content_comes_from_its_owner_and_shared_guides_work_offline() {
    let dir = fixture("manual-installed");
    let answer = descriptor("# Example\n\nThe installed owner's page.\n");
    write_program(&dir.join("lnk-example"),format!("#!/bin/sh\n[ \"$1\" = --link-describe ] || exit 99\ncat <<'DESCRIPTION'\n{answer}\nDESCRIPTION\n")).unwrap();
    let result = read(&dir, &["agents/harnesses/example"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("installed owner's page"));
    let result = read(&dir, &[]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        String::from_utf8_lossy(&result.stdout).starts_with(include_str!("../../../../README.md"))
    );
    let result = read(&dir, &["more/developing/testing/testing-kit"]);
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8_lossy(&result.stdout).trim(),
        include_str!("../../../../packages/test-kit/docs/more/developing/testing/testing-kit.md")
            .trim()
    );
    assert!(!read(&dir, &["../outside"]).status.success());
    assert!(
        !read(
            &dir,
            &["agents/harnesses/example", "--commands", "--variables"]
        )
        .status
        .success()
    );
}

#[test]
fn an_unknown_topic_is_a_usage_failure_that_suggests_the_list() {
    let dir = fixture("manual-unknown");
    let result = read(&dir, &["nowhere/at-all"]);
    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("lnk manual"));
}
