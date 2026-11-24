use super::*;
use std::path::Path;

/// A spec with a top-level `os`, a `machine` table, a `engine` record
/// whose shorthand is its `name`, and the spec's own `name`.
fn binding() -> Binding {
    Binding {
        schema: serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "name": { "type": "string" },
                "os": { "type": "string" },
                "machine": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": { "size": { "type": "string" } }
                },
                "engine": { "$ref": "#/$defs/engine" }
            },
            "$defs": {
                "engine": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["name"],
                    "x-link-shorthand": "name",
                    "properties": {
                        "name": { "type": "string" },
                        "home": { "type": "string" }
                    }
                }
            }
        }),
        format: crate::schema::Format::parse(
            "schema = 1\nkind = \"short\"\ncurrent = 1\ncommand = \"lnk x spec show\"\n",
        )
        .unwrap(),
    }
}

fn split(words: &[&str], runtime: &[&str]) -> anyhow::Result<(FieldFlags, Vec<OsString>)> {
    let root = Path::new("/");
    let runtime: Vec<String> = runtime.iter().map(|r| r.to_string()).collect();
    super::super::given::split_from(args(words), Some(&binding()), None, &runtime, root, root)
}

#[test]
fn a_top_level_fields_flag_is_its_own_name_and_the_specs_own_take_spec() {
    let (flags, rest) = split(
        &[
            "--os",
            "linux",
            "--machine.size=large",
            "--spec.name",
            "web",
        ],
        &[],
    )
    .unwrap();
    assert!(rest.is_empty(), "{rest:?}");
    assert_eq!(flags.table()["os"].as_str(), Some("linux"));
    assert_eq!(flags.table()["machine"]["size"].as_str(), Some("large"));
    assert_eq!(flags.table()["name"].as_str(), Some("web"));
    let listed: Vec<String> = fields(&binding().schema)
        .into_iter()
        .map(|f| f.flag)
        .collect();
    for flag in ["os", "machine.size", "spec.name", "engine", "engine.name"] {
        assert!(listed.contains(&flag.to_string()), "{flag} in {listed:?}");
    }
}

#[test]
fn a_runtime_flag_stays_the_commands() {
    // `--name` is a runtime flag here, and the spec's own name is
    // `--spec.name`, so the two never meet.
    let (flags, rest) = split(&["--name", "box-1", "--os", "linux"], &["name"]).unwrap();
    assert_eq!(rest, args(&["--name", "box-1"]));
    assert!(flags.table().get("name").is_none());
    // A flag no field names is the runtime parser's to judge.
    let (_, rest) = split(&["--json"], &["json"]).unwrap();
    assert_eq!(rest, args(&["--json"]));
}

#[test]
fn a_runtime_flag_sharing_a_fields_name_is_refused() {
    let said = format!(
        "{:#}",
        split(&["--machine.size", "x"], &["os"]).unwrap_err()
    );
    assert!(
        said.contains("--os is a runtime flag and a field's flag"),
        "{said}"
    );
    assert_eq!(
        clashes(&binding().schema, &["os".into(), "name".into()]).len(),
        1
    );
}

#[test]
fn the_old_spec_prefix_is_refused_with_the_flag_now() {
    let said = format!("{:#}", split(&["--spec.os", "linux"], &[]).unwrap_err());
    assert!(said.contains("--os"), "{said}");
    let said = format!(
        "{:#}",
        split(&["--spec.machine.size", "x"], &[]).unwrap_err()
    );
    assert!(said.contains("--machine.size"), "{said}");
}

#[test]
fn a_shorthand_takes_a_bare_value_and_a_table_sets_the_record() {
    let (flags, _) = split(&["--engine", "v8"], &[]).unwrap();
    assert_eq!(flags.table()["engine"]["name"].as_str(), Some("v8"));
    let (flags, _) = split(&["--engine", "{ name = \"quickjs\", home = \"h\" }"], &[]).unwrap();
    assert_eq!(flags.table()["engine"]["name"].as_str(), Some("quickjs"));
    assert_eq!(flags.table()["engine"]["home"].as_str(), Some("h"));
    assert_eq!(flag_takes(&binding().schema, "--engine"), Some(true));
    assert_eq!(flag_takes(&binding().schema, "--os"), Some(true));
    assert_eq!(flag_takes(&binding().schema, "--spec.os"), None);
}

#[test]
fn every_field_has_its_key_path_as_its_flag() {
    let flags: Vec<String> = fields(&crate::schema::<Example>())
        .into_iter()
        .map(|f| f.flag)
        .collect();
    // Only the spec's own properties take `spec.`: every other top-level
    // field's flag is its key alone.
    for flag in flags.iter().filter(|f| f.starts_with("spec.")) {
        assert!(
            ["spec.name", "spec.version"].contains(&flag.as_str()),
            "{flag}"
        );
    }
    let mut sorted = flags.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        [
            "env",
            "files.write",
            "files.write_dot_files",
            "model.thinking",
            "service",
            "spec.name",
            "spec.version",
            "tool.<name>.enabled",
            "tool.<name>.limit",
            "variables.<name>",
        ]
    );
}
