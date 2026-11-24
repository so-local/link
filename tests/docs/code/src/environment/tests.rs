use super::*;
use scan::Name;

#[test]
fn follows_constants_import_aliases_local_bindings_and_read_helpers() {
    let reads = scan::from_code(
        r#"
        use std::env::{var as setting, var_os};
        const FIRST: &str = "LNK_FIRST";
        const SECOND: &str = concat!("LNK_", "SECOND");
        fn read(name: &str) { setting(name).ok(); }
        fn wrapper(name: &str) { read(name); }
        fn run() {
            let key = FIRST;
            var_os(key);
            wrapper(SECOND);
            let secret = |name: &str| Secret::from_env(name);
            secret("LNK_THIRD");
            anyhow::ensure!(std::env::var("LNK_IN_MACRO").is_ok(), "input");
        }
    "#,
    );
    let names: BTreeSet<_> = reads
        .into_iter()
        .filter_map(|read| {
            if let Name::Static(name) = read.name {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        names,
        BTreeSet::from(["LNK_FIRST", "LNK_SECOND", "LNK_THIRD", "LNK_IN_MACRO"].map(String::from))
    );
}

#[test]
fn ignores_comments_string_contents_and_test_only_code() {
    let reads = scan::from_code(
        r#"
        // std::env::var("LNK_COMMENT");
        const EXAMPLE: &str = "std::env::var(\"LNK_STRING\")";
        #[cfg(test)] mod tests { fn run() { std::env::var("LNK_FIXTURE"); } }
        #[test] fn isolated() { std::env::var("LNK_TEST"); }
        fn live() {
            std::env::var("LNK_LIVE");
            #[cfg(test)]
            std::env::var("LNK_STATEMENT");
        }
    "#,
    );
    assert_eq!(reads.len(), 1);
    assert_eq!(
        reads.iter().next().unwrap().name,
        Name::Static("LNK_LIVE".into())
    );
}

#[test]
fn keeps_dynamic_patterns_and_configured_reader_apis_visible() {
    let reads = scan::from_code(
        r#"
        fn selected(name: &str) { std::env::var(name); }
        fn tool(name: &str) {
            let key = format!("LNK_TOOL_{name}");
            std::env::var_os(key);
        }
    "#,
    );
    assert!(
        reads
            .iter()
            .any(|read| read.name == Name::Pattern("LNK_TOOL_*".into()))
    );
    assert!(
        reads
            .iter()
            .any(|read| read.api == "example::selected" && read.name == Name::Input)
    );
}

#[test]
fn follows_environment_reader_callbacks_into_their_literal_names() {
    let reads = scan::from_code(
        r#"
        fn apply(read: impl Fn(&str) -> Option<String>) { read("LNK_CALLBACK"); }
        fn run() { apply(|name| std::env::var(name).ok()); }
    "#,
    );
    assert!(
        reads
            .iter()
            .any(|r| r.name == Name::Static("LNK_CALLBACK".into()))
    );
}

#[test]
fn follows_names_in_constant_arrays_and_iterators() {
    let reads = scan::from_code(
        r#"
        const NAMES: &[&str] = &["LNK_FIRST", "LNK_SECOND"];
        fn run() {
            for name in NAMES { std::env::var(name); }
            NAMES.iter().filter_map(|name| std::env::var_os(name));
        }
    "#,
    );
    assert!(
        reads
            .iter()
            .any(|r| r.name == Name::Static("LNK_FIRST".into()))
    );
    assert!(
        reads
            .iter()
            .any(|r| r.name == Name::Static("LNK_SECOND".into()))
    );
    assert!(!reads.iter().any(|r| r.name == Name::Input));
}

#[test]
fn cyclic_reexports_do_not_recurse_without_bound() {
    let reads = scan::from_code(
        r#"
        mod a { pub use crate::b::read; }
        mod b { pub use crate::a::read; }
        fn run() { a::read("LNK_CYCLE"); }
    "#,
    );
    assert!(reads.is_empty());
}

fn tree(files: &[(&str, &str)]) -> Tree {
    let root = std::path::PathBuf::from("/environment-fixture");
    Tree {
        root: root.clone(),
        checkout: root.clone(),
        files: files
            .iter()
            .map(|(name, text)| (root.join(name), text.to_string()))
            .collect(),
        dirs: BTreeSet::new(),
        folders: vec![],
        websites: vec![],
        pages: BTreeMap::new(),
        limits: link_docs_check::Limits::read(),
        large: Default::default(),
    }
}

#[test]
fn production_graph_excludes_build_only_dev_only_and_fixture_members() {
    let tree = tree(&[
        (
            "app/Cargo.toml",
            "[package]\nname='app'\n[dependencies.shared]\npath='../shared'\n[build-dependencies.builder]\npath='../builder'\n[dev-dependencies.testkit]\npath='../testkit'",
        ),
        (
            "app/src/main.rs",
            "fn main() { std::env::var(\"LNK_LIVE\"); }",
        ),
        ("app/src/lib.rs", "#[cfg(all(test, unix))] mod probe;"),
        (
            "app/src/probe.rs",
            "fn run() { std::env::var(\"LNK_TEST_MODULE\"); }",
        ),
        (
            "app/build.rs",
            "fn main() { std::env::var(\"LNK_BUILD\"); }",
        ),
        (
            "app/tests/check.rs",
            "fn main() { std::env::var(\"LNK_TEST\"); }",
        ),
        ("shared/Cargo.toml", "[package]\nname='shared'"),
        (
            "shared/src/lib.rs",
            "fn run() { std::env::var(\"LNK_LIBRARY\"); }",
        ),
        ("builder/Cargo.toml", "[package]\nname='builder'"),
        (
            "builder/src/lib.rs",
            "fn run() { std::env::var(\"LNK_BUILD_ONLY\"); }",
        ),
        ("testkit/Cargo.toml", "[package]\nname='testkit'"),
        (
            "testkit/src/lib.rs",
            "fn run() { std::env::var(\"LNK_DEV_ONLY\"); }",
        ),
        (
            "fixture/Cargo.toml",
            "[package]\nname='fixture'\n[package.metadata.link]\nrole='fixture'",
        ),
        (
            "fixture/src/main.rs",
            "fn main() { std::env::var(\"LNK_FIXTURE\"); }",
        ),
    ]);
    let (reads, errors) = scan::reads(&tree);
    assert!(errors.is_empty());
    assert_eq!(reads.len(), 2);
    assert!(
        reads
            .iter()
            .all(|r| matches!(&r.name, Name::Static(n) if n == "LNK_LIVE" || n == "LNK_LIBRARY"))
    );
}

#[test]
fn namespace_and_generic_api_declarations_never_authorize_a_new_literal() {
    let tree = tree(&[
        ("app/Cargo.toml", "[package]\nname='app'"),
        (
            "app/variables.toml",
            "schema = 1\n[namespaces]\n'LNK_*'='Program-selected namespace'\n[readers]\nmain='Caller-selected variable names'",
        ),
        (
            "app/src/main.rs",
            "fn main() { std::env::var(\"LNK_UNDECLARED\"); }",
        ),
    ]);
    let failures = check(&tree);
    assert_eq!(failures.len(), 1);
    assert!(failures[0].text.contains("LNK_UNDECLARED"));
}

#[test]
fn generic_input_contracts_are_scoped_to_the_reader_api() {
    let tree = tree(&[
        ("app/Cargo.toml", "[package]\nname='app'"),
        (
            "app/variables.toml",
            "schema = 1\n[readers]\nselected='Variable named by the caller'",
        ),
        (
            "app/src/main.rs",
            "fn main() {} fn selected(name: &str) { std::env::var(name); } fn other(name: &str) { std::env::var(name); }",
        ),
    ]);
    let failures = check(&tree);
    assert_eq!(failures.len(), 1);
    assert!(failures[0].text.contains("app::other"));
}

#[test]
fn literal_format_parts_and_function_bindings_remain_static_reads() {
    let reads = scan::from_code(
        r#"
        fn run() {
            let read = std::env::var;
            let suffix = "NEW";
            read(format!("LNK_{suffix}"));
            std::env::var_os(format!("LNK_{}", "POSITIONAL"));
        }
    "#,
    );
    assert!(
        reads
            .iter()
            .any(|r| r.name == Name::Static("LNK_NEW".into()))
    );
    assert!(
        reads
            .iter()
            .any(|r| r.name == Name::Static("LNK_POSITIONAL".into()))
    );
}

#[test]
fn constant_cycles_fail_closed_instead_of_becoming_generic_inputs() {
    let reads = scan::from_code(
        r#"
        const A: &str = B;
        const B: &str = A;
        fn run() { std::env::var(A); }
    "#,
    );
    assert!(reads.iter().any(|r| matches!(r.name, Name::Unresolved(_))));
}

#[test]
fn a_computed_unknown_prefix_requires_an_explicit_reader_contract() {
    let tree = tree(&[
        ("app/Cargo.toml", "[package]\nname='app'"),
        (
            "app/src/main.rs",
            r#"fn main() {} fn read(prefix: &str) { std::env::var(format!("{prefix}_SETTING")); }"#,
        ),
    ]);
    let failures = check(&tree);
    assert_eq!(failures.len(), 1);
    assert!(failures[0].text.contains("app::read"));
}

#[test]
fn every_variable_read_is_declared_the_systems_too() {
    let source = "fn main() { std::env::var(\"PATH\"); std::env::var(\"XDG_RUNTIME_DIR\"); }";
    let tree = tree(&[
        ("app/Cargo.toml", "[package]\nname='app'"),
        (
            "app/variables.toml",
            "schema = 1\n[defines.PATH]\ntype='string'\ndescription='Where programs are'",
        ),
        ("app/src/main.rs", source),
    ]);
    let failures = check(&tree);
    assert_eq!(
        failures.len(),
        1,
        "{:?}",
        failures.iter().map(|f| &f.text).collect::<Vec<_>>()
    );
    assert!(failures[0].text.contains("XDG_RUNTIME_DIR"));
}

#[test]
fn a_crate_declares_each_variable_it_reads_even_one_another_defines() {
    let reads = "fn main() { std::env::var(\"PATH\"); }";
    let defines = "schema = 1\n[defines.PATH]\ntype='string'\ndescription='Where programs are'";
    let uses = "schema = 1\n[[uses]]\nname='PATH'\naccess=['read']\ncontext='runtime'";
    let undeclared = tree(&[
        ("core/Cargo.toml", "[package]\nname='core'"),
        ("core/variables.toml", defines),
        ("app/Cargo.toml", "[package]\nname='app'"),
        ("app/src/main.rs", reads),
    ]);
    let failures = check(&undeclared);
    assert_eq!(failures.len(), 1);
    assert!(
        failures[0].text.contains("its crate"),
        "{}",
        failures[0].text
    );
    let declared = tree(&[
        ("core/Cargo.toml", "[package]\nname='core'"),
        ("core/variables.toml", defines),
        ("app/Cargo.toml", "[package]\nname='app'"),
        ("app/variables.toml", uses),
        ("app/src/main.rs", reads),
    ]);
    assert!(check(&declared).is_empty());
}

#[test]
fn a_variable_has_one_home() {
    let defines = "schema = 1\n[defines.LNK_A]\ntype='string'\ndescription='A'";
    let failures = check(&tree(&[
        ("a/Cargo.toml", "[package]\nname='a'"),
        ("a/variables.toml", defines),
        ("b/Cargo.toml", "[package]\nname='b'"),
        ("b/variables.toml", defines),
        (
            "c/Cargo.toml",
            "[package]\nname='c'\n[package.metadata.link.environment.LNK_C]\ntype='string'\nhelp='C'",
        ),
    ]));
    let texts: Vec<_> = failures.iter().map(|f| f.text.as_str()).collect();
    // A flag's fallback is the catalog's to check, as it loads.
    assert_eq!(failures.len(), 2, "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("defined by both")));
    assert!(
        texts
            .iter()
            .any(|t| t.contains("moves to the crate's variables.toml"))
    );
}

#[test]
fn reads_each_use_with_its_access_and_time() {
    use link_actions::{Access, When};
    let reads = scan::from_code(include_str!("uses.txt"));
    let uses: BTreeSet<(Name, Access, When)> = reads
        .into_iter()
        .map(|read| (read.name, read.access, read.context))
        .collect();
    let named = |name: &str, access, when| (Name::Static(name.into()), access, when);
    assert_eq!(
        uses,
        BTreeSet::from([
            named("LNK_BUILT", Access::Read, When::Build),
            named("LNK_SET", Access::Set, When::Runtime),
            named("LNK_UNSET", Access::Unset, When::Runtime),
            named("LNK_CHILD", Access::Set, When::Runtime),
            named("LNK_GONE", Access::Unset, When::Runtime),
            named("LNK_PAIR", Access::Set, When::Runtime),
            named("CARGO_PKG_VERSION", Access::Read, When::Build),
            (Name::Input, Access::Read, When::Runtime),
        ])
    );
}

#[test]
fn cargos_own_variables_are_read() {
    assert!(cargo_sets("OUT_DIR") && cargo_sets("CARGO_PKG_NAME"));
    assert!(!cargo_sets("LNK_BUILD"));
}
