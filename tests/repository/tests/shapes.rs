//! No command's arguments and no spec's shape is written in Rust: each is
//! declared in TOML and generated (`link_repository_check::shapes`, by
//! `shapes.toml`).

use std::path::Path;

use link_repository_check::shapes::{problems, refused, statements, test_code};

#[test]
fn no_shape_is_written_in_rust_but_the_generators_and_the_held() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = here.join("../..").canonicalize().unwrap();
    let problems = problems(&root, &here.join("shapes.toml"));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn a_clap_type_a_typed_inventory_and_a_specification_are_refused() {
    assert!(refused("#[derive(Parser)]").is_some());
    assert!(refused("#[derive(clap::Args, Serialize, Deserialize)]").is_some());
    assert!(refused("#[derive(Debug, Subcommand)]").is_some());
    assert!(refused("#[derive(Clone, Copy, clap::ValueEnum)]").is_some());
    assert!(refused("#[derive(Debug, ValueEnum, Serialize)]").is_some());
    assert!(refused("impl clap::ValueEnum for Os {").is_some());
    assert!(refused("    typed::<Home>(path),").is_some());
    assert!(refused("use link_spec::typed;").is_some());
    assert!(refused("impl Specification for Bundle {").is_some());
    assert!(refused("impl link_files::schema::Specification for Bundle {").is_some());
    assert!(refused("#[derive(Debug, Clone, Serialize)]").is_none());
    assert!(refused("// #[derive(Parser)] as it was").is_none());
    assert!(refused("let args = Arguments::parse();").is_none());
}

#[test]
fn a_json_schema_derived_or_written_in_rust_is_refused() {
    assert!(refused("#[derive(Clone, Serialize, schemars::JsonSchema)]").is_some());
    assert!(refused("#[derive(Deserialize, JsonSchema)]").is_some());
    assert!(refused("impl schemars::JsonSchema for Size {").is_some());
    assert!(refused("impl<T: JsonSchema> JsonSchema for Stage<T> {").is_some());
    assert!(refused("fn check<T: schemars::JsonSchema>() {").is_none());
}

#[test]
fn a_derive_spread_over_lines_is_read_whole() {
    let spread = statements("#[derive(\n    Clone,\n    schemars::JsonSchema,\n)]\nstruct A;\n");
    assert_eq!(spread[0].0, 1);
    assert!(refused(&spread[0].1).is_some());
    assert_eq!(spread[1], (5, "struct A;".to_string()));
    let clap = statements("#[derive(\n    clap::Parser,\n)]\n");
    assert!(refused(&clap[0].1).is_some());
}

#[test]
fn test_code_may_build_a_parser() {
    assert!(test_code("packages/spec/src/entry/tests.rs"));
    assert!(test_code("modules/x/tests/protocol.rs"));
    assert!(test_code("packages/spec/src/io/schema_tests.rs"));
    assert!(test_code("modules/x/examples/refresh.rs"));
    assert!(!test_code("modules/x/src/cli.rs"));
}

/// A declaration naming a Rust type is refused unless `[rust]` lists it,
/// and a listed one that names none any more is refused too.
#[test]
fn a_rust_type_named_in_a_declaration_is_listed() {
    let root = link_test_kit::TempDir::new("shapes-rust");
    let git = |args: &[&str]| {
        let done = link_test_kit::env::command("git", &root)
            .args(args)
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(
            done.status.success(),
            "{}",
            String::from_utf8_lossy(&done.stderr)
        );
    };
    git(&["init", "--quiet"]);
    let declaration = root.join("modules/x/types.toml");
    std::fs::create_dir_all(declaration.parent().unwrap()).unwrap();
    std::fs::write(&declaration, "[a]\nx-link-rust = \"::x::A\"\n").unwrap();
    git(&["add", "modules/x/types.toml"]);
    let rules = root.join("shapes.toml");
    std::fs::write(&rules, "[allowed]\n").unwrap();
    let found = problems(&root, &rules);
    assert!(
        found.len() == 1 && found[0].contains("modules/x/types.toml names a Rust type"),
        "{found:?}"
    );
    std::fs::write(
        &rules,
        "[allowed]\n[rust]\n\"modules/x/types.toml\" = \"why\"\n",
    )
    .unwrap();
    assert_eq!(problems(&root, &rules), Vec::<String>::new());
    std::fs::write(&declaration, "[a]\ntype = \"string\"\n").unwrap();
    let found = problems(&root, &rules);
    assert!(
        found.len() == 1 && found[0].contains("names no Rust type now"),
        "{found:?}"
    );
}
