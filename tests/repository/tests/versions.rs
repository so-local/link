//! No Link version past the workspace's own outside test code: a tracked
//! file naming one is listed in `versions.toml`, which may shrink, never
//! grow (the rule is `link_repository_check::versions`, which the private
//! workspace runs on its own root too).

use std::collections::BTreeMap;
use std::path::Path;

use link_repository_check::versions::{ahead, compare, is_test, skipped};

#[test]
fn no_link_release_past_the_workspaces_line() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = here.join("../..").canonicalize().unwrap();
    let problems = link_repository_check::versions::check(
        &root,
        &root.join("Cargo.toml"),
        &here.join("versions.toml"),
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Each context names a Link version, and one past the line is refused.
#[test]
fn every_context_past_the_line_is_found() {
    let line = (0, 0);
    for said in [
        "lnk 0.14.0 \u{b7} linux x86_64",
        "lnk-relay 0.13.0",
        "Link 0.1, the first public snapshot",
        "Link is Local Link 0.14.0's",
        "LNK_VERSION=0.14.0 sh",
        "--lnk.version 0.11.0",
        "lnk upgrade 0.14.0",
        "minimum_release = \"0.14.0\"",
        "SERVICE_RELEASE_MINIMUM = \"0.14.0\"",
        "a version such as `0.14.0-dev.202611090720.g1a2b3c4d5e6f`",
        "since = \"0.14.0\"",
        "since = '0.2.0'",
        "image = \"harness@0.14\"",
        "pub const FLOOR: Version = Version::new(0, 11, 0);",
        "Release 0.1.0 is the first real release",
        "the release candidate at 0.1.0",
    ] {
        assert_eq!(ahead(said, false, line).len(), 1, "{said}");
    }
}

/// A version on the line, a third party's, and one in no context of
/// Link's pass, as do a Rust file's tests.
#[test]
fn what_isnt_a_link_release_past_the_line_passes() {
    let line = (0, 0);
    for said in [
        "lnk 0.0.1 \u{b7} linux x86_64",
        "Link 0.0.1 is the first release that lasts",
        "LNK_VERSION=0.0.1 sh",
        "0.0.1-staging.202610131900",
        "since = \"0.0.0\"",
        "until = \"0.1.0\"",
        "signal-cli-0.14.8/bin/signal-cli",
        "bubblewrap 0.9.0",
        "qrcode = { version = \"0.14\" }",
        "listen = \"0.0.0.0:7600\"",
        "192.0.2.10 and 10.0.2.15",
        "7.4 core-seconds/hour for Link",
    ] {
        assert!(ahead(said, false, line).is_empty(), "{said}");
    }
    let rust = "fn f() {}\n#[cfg(test)]\nmod tests {\n    const V: &str = \"lnk 0.14.0\";\n}\n";
    assert!(ahead(rust, true, line).is_empty());
    assert_eq!(ahead(rust, false, line), [(4, "lnk 0.14.0".to_string())]);
    // A later line takes what was past an earlier one.
    assert!(ahead("lnk 0.14.0", false, (0, 14)).is_empty());
    assert_eq!(ahead("lnk 1.0.0", false, (0, 14)).len(), 1);
}

#[test]
fn test_code_and_history_are_left_out() {
    for test in [
        "modules/compute/box/tests/box/setup.rs",
        "packages/files/src/schema/tests.rs",
        "web/apps/docs/src/installer.test.mts",
        "scripts/checks/test_boundaries.py",
        "data/cases/core/case.sh",
        "packages/test-kit/fixtures/x.toml",
    ] {
        assert!(is_test(test), "{test}");
    }
    for not in [
        "modules/compute/box/src/spec.rs",
        "packages/test-kit/docs/fixtures/core.md",
        "docs/more/modules/guide.md",
    ] {
        assert!(!is_test(not), "{not}");
    }
    assert!(skipped("Cargo.lock"));
    assert!(skipped("data/logs/sessions/x.md"));
    assert!(!skipped("docs/more/modules/guide.md"));
}

/// The list may shrink, never grow, and keeps no file that names none.
#[test]
fn the_list_only_shrinks() {
    let line = (0, 0);
    let found = BTreeMap::from([("a.rs".to_string(), vec![(3, "lnk 0.14.0".to_string())])]);
    assert!(compare(&found, &BTreeMap::from([("a.rs".into(), 1)]), line).is_empty());
    assert!(compare(&found, &BTreeMap::new(), line)[0].contains("a.rs:3"));
    assert!(
        compare(&found, &BTreeMap::from([("a.rs".into(), 2)]), line)[0].contains("down from 2")
    );
    assert!(
        compare(
            &BTreeMap::new(),
            &BTreeMap::from([("a.rs".into(), 1)]),
            line
        )[0]
        .contains("take it out")
    );
}
