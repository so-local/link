use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use link_test_kit::temp::TempDir;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::given::split_from;
use super::*;
use crate::{FieldFlags, Show};

/// A spec with one of each shape a field takes.
#[derive(Deserialize, Serialize, JsonSchema, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    /// Its version, a number of its own.
    pub version: Option<u32>,
    /// Its name.
    pub name: Option<String>,
    pub model: Option<Model>,
    #[serde(default, skip_serializing_if = "Files::is_empty")]
    pub files: Files,
    /// Variables it runs with, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tool: BTreeMap<String, Tool>,
    /// Services it starts, each a whole element.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub service: Vec<Service>,
    /// The environment it runs in.
    #[schemars(extend("format" = "path"))]
    pub env: Option<PathBuf>,
}

#[derive(Deserialize, Serialize, JsonSchema, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Model {
    /// How much the model reasons before it answers.
    pub thinking: Option<Thinking>,
}

#[derive(Deserialize, Serialize, JsonSchema, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Thinking {
    Off,
    Low,
    High,
}

#[derive(Deserialize, Serialize, JsonSchema, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Files {
    /// Folders it may write.
    #[serde(default)]
    #[schemars(extend("items" = {"type": "string", "format": "path"}))]
    pub write: Vec<String>,
    /// Whether it may write dot files.
    #[serde(default)]
    pub write_dot_files: bool,
}

impl Files {
    fn is_empty(&self) -> bool {
        self.write.is_empty() && !self.write_dot_files
    }
}

#[derive(Deserialize, Serialize, JsonSchema, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    /// Whether the tool is on.
    pub enabled: Option<bool>,
    pub limit: Option<u32>,
}

#[derive(Deserialize, Serialize, JsonSchema, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub name: String,
    pub run: String,
}

mod agent_kind {
    crate::specification!(
        crate::fields::tests::Agent,
        include_str!("../../tests/fixtures/agent-schema.toml"),
        check = |agent: &crate::fields::tests::Agent| {
            anyhow::ensure!(
                agent.name.as_deref() != Some("unchecked"),
                "no agent is named unchecked"
            );
            Ok(())
        }
    );
}

/// The same spec with one optional field added, in no fixture.
#[derive(Deserialize, Serialize, JsonSchema, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Grown {
    #[serde(flatten)]
    pub agent: Agent,
    pub added: Option<u16>,
}

fn args(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

fn here() -> (PathBuf, PathBuf) {
    (PathBuf::from("/work"), PathBuf::from("/home/me"))
}

fn given(words: &[&str]) -> anyhow::Result<(FieldFlags, Vec<OsString>)> {
    let (cwd, home) = here();
    split_from(
        args(words),
        Some(&Binding::of::<Agent>()),
        None,
        &[],
        &cwd,
        &home,
    )
}

fn flags(words: &[&str]) -> FieldFlags {
    given(words).unwrap().0
}

fn refused(words: &[&str]) -> String {
    format!("{:#}", given(words).unwrap_err())
}

#[test]
fn an_optional_field_added_has_its_flag_with_no_other_change() {
    let schema = crate::schema::<Grown>();
    assert!(fields(&schema).iter().any(|f| f.flag == "added"));
    let (cwd, home) = here();
    let binding = Binding {
        schema,
        format: <Agent as crate::Specification>::format().clone(),
    };
    let (flags, _) = split_from(
        args(&["--added", "7"]),
        Some(&binding),
        None,
        &[],
        &cwd,
        &home,
    )
    .unwrap();
    assert_eq!(flags.table()["added"].as_integer(), Some(7));
}

#[test]
fn values_are_toml_with_text_where_a_string_is_wanted() {
    let table = flags(&[
        "--model.thinking",
        "high",
        "--spec.name=123",
        "--spec.version",
        "3",
        "--tool.code.enabled=false",
        "--files.write_dot_files",
    ])
    .table()
    .clone();
    assert_eq!(table["model"]["thinking"].as_str(), Some("high"));
    // Text where a string is wanted, though TOML would read a number.
    assert_eq!(table["name"].as_str(), Some("123"));
    assert_eq!(table["version"].as_integer(), Some(3));
    assert_eq!(table["tool"]["code"]["enabled"].as_bool(), Some(false));
    // A switch alone is true, and never takes the next word.
    assert_eq!(table["files"]["write_dot_files"].as_bool(), Some(true));
}

#[test]
fn repeats_build_a_list_and_a_list_replaces_whole() {
    let table = flags(&["--files.write", "/a", "--files.write", "/b"])
        .table()
        .clone();
    assert_eq!(
        table["files"]["write"],
        toml::Value::Array(vec!["/a".into(), "/b".into()])
    );
    let empty = flags(&["--files.write=[]"]).table().clone();
    assert_eq!(empty["files"]["write"], toml::Value::Array(Vec::new()));
    assert!(refused(&["--files.write=[]", "--files.write=[]"]).contains("given twice"));
    assert!(refused(&["--spec.name", "a", "--spec.name", "b"]).contains("given twice"));
}

#[test]
fn free_keys_are_addressed_by_the_key_itself_quoted_as_toml_quotes_it() {
    let table = flags(&[
        "--variables.REGION",
        "eu-west-2",
        "--tool.\"social.post:k7m2\".limit",
        "4",
    ])
    .table()
    .clone();
    assert_eq!(table["variables"]["REGION"].as_str(), Some("eu-west-2"));
    assert_eq!(
        table["tool"]["social.post:k7m2"]["limit"].as_integer(),
        Some(4)
    );
    assert!(
        fields(&crate::schema::<Agent>())
            .iter()
            .any(|f| f.flag == "variables.<name>")
    );
    assert_eq!(
        flag_of(&["tool".into(), "social.post:k7m2".into(), "limit".into()]),
        "tool.\"social.post:k7m2\".limit"
    );
}

#[test]
fn a_list_of_records_takes_whole_inline_table_elements() {
    let table = flags(&[
        "--service",
        "{ name = \"web\", run = \"./serve\" }",
        "--service",
        "{ name = \"db\", run = \"./db\" }",
    ])
    .table()
    .clone();
    let services = table["service"].as_array().unwrap();
    assert_eq!(services.len(), 2);
    assert_eq!(services[1]["name"].as_str(), Some("db"));
}

#[test]
fn an_unknown_or_reserved_path_is_refused_by_name() {
    let said = refused(&["--model.thinkin", "high"]);
    assert!(said.contains("the nearest is --model.thinking"), "{said}");
    let said = refused(&["--schema", "2"]);
    assert!(said.contains("takes no flag"), "{said}");
    let said = refused(&["--kind", "box"]);
    assert!(said.contains("takes no flag"), "{said}");
    let said = refused(&["--spec.model.thinking", "high"]);
    assert!(said.contains("--model.thinking"), "{said}");
    // A list's elements have no names, so no flags of their own.
    assert!(refused(&["--service.name", "web"]).contains("no field"));
    // A field a step renamed is answered with its flag now.
    let said = refused(&["--title", "x"]);
    assert_eq!(
        said,
        "--title is gone since agent schema 2. Use --spec.name."
    );
    let said = refused(&["--model.thinking"]);
    assert!(said.contains("needs a value"), "{said}");
    let said = refused(&["--model.thinking", "--json"]);
    assert!(said.contains("needs a value"), "{said}");
}

#[test]
fn runtime_arguments_and_everything_after_a_double_dash_pass_through() {
    let (flags, rest) = given(&[
        "lnk-agent",
        "start",
        "main",
        "--spec",
        "a.toml",
        "--model.thinking",
        "low",
        "--json",
        "--",
        "--spec.name",
        "x",
    ])
    .unwrap();
    assert_eq!(flags.table()["model"]["thinking"].as_str(), Some("low"));
    assert_eq!(
        rest,
        args(&[
            "lnk-agent",
            "start",
            "main",
            "--spec",
            "a.toml",
            "--json",
            "--",
            "--spec.name",
            "x"
        ])
    );
    let (cwd, home) = here();
    let none = split_from(
        args(&["run", "--files.write", "x"]),
        None,
        None,
        &[],
        &cwd,
        &home,
    )
    .unwrap_err();
    assert!(none.to_string().contains("takes no field flags"), "{none}");
}

#[test]
fn a_flags_relative_path_is_from_the_current_folder() {
    let table = flags(&[
        "--env",
        "env.toml",
        "--files.write",
        "out",
        "--files.write=~/x",
    ])
    .table()
    .clone();
    assert_eq!(table["env"].as_str(), Some("/work/env.toml"));
    assert_eq!(
        table["files"]["write"],
        toml::Value::Array(vec!["/work/out".into(), "/home/me/x".into()])
    );
    assert!(refused(&["--env", "../env.toml"]).contains(".."));
}

/// A spec file of `text`, naming this release's schema where it names
/// none, as a spec does.
fn file(dir: &TempDir, text: &str) -> PathBuf {
    let path = dir.path().join("agent.toml");
    let text = match text.lines().any(|line| line.starts_with("schema")) {
        true => text.to_string(),
        false => format!("schema = 2\n{text}"),
    };
    std::fs::write(&path, text).unwrap();
    path
}

fn real(dir: &TempDir) -> PathBuf {
    dir.path().canonicalize().unwrap()
}

#[test]
fn a_file_and_the_same_flags_read_to_equal_specs() {
    let dir = TempDir::new("spec-fields");
    let text = "name = \"main\"\nversion = 2\nenv = \"/e.toml\"\n[model]\nthinking = \"high\"\n\
                [files]\nwrite = [\"/a\"]\nwrite_dot_files = true\n[variables]\nREGION = \"eu\"\n\
                [tool.code]\nenabled = false\n[[service]]\nname = \"web\"\nrun = \"./serve\"\n";
    let from_file = read_with::<Agent>(Some(&file(&dir, text)), &FieldFlags::default())
        .unwrap()
        .spec;
    let from_flags = read_with::<Agent>(
        None,
        &flags(&[
            "--spec.name",
            "main",
            "--spec.version",
            "2",
            "--env",
            "/e.toml",
            "--model.thinking",
            "high",
            "--files.write",
            "/a",
            "--files.write_dot_files",
            "--variables.REGION",
            "eu",
            "--tool.code.enabled=false",
            "--service",
            "{ name = \"web\", run = \"./serve\" }",
        ]),
    )
    .unwrap()
    .spec;
    assert_eq!(from_file, from_flags);
}

/// Flags given again as `args` are the same flags, so a command run
/// elsewhere reads the same spec, and `over` lays them on a spec already
/// made as on a file.
#[test]
fn flags_given_again_are_the_same_and_lay_over_a_made_spec() {
    let given = flags(&[
        "--spec.name",
        "main",
        "--files.write",
        "/a",
        "--files.write",
        "/b",
        "--files.write_dot_files",
        "--variables.REGION",
        "eu",
        "--service",
        "{ name = \"web\", run = \"./serve\" }",
    ]);
    let again: Vec<String> = given
        .args()
        .into_iter()
        .map(|a| a.into_string().unwrap())
        .collect();
    let words: Vec<&str> = again.iter().map(String::as_str).collect();
    assert_eq!(flags(&words), given);
    assert_eq!(
        given.get(&["variables", "REGION"]),
        Some(&toml::Value::String("eu".into()))
    );
    assert_eq!(given.get(&["model", "thinking"]), None);
    let base = read_with::<Agent>(
        None,
        &flags(&["--model.thinking", "high", "--spec.name", "x"]),
    )
    .unwrap()
    .spec;
    let made = over(&base, &given).unwrap();
    assert_eq!(made.name.as_deref(), Some("main"));
    assert_eq!(made.files.write, ["/a", "/b"]);
    assert_eq!(made, over(&made, &FieldFlags::default()).unwrap());
    assert_eq!(made.model.and_then(|m| m.thinking), Some(Thinking::High));
}

#[test]
fn flags_replace_their_keys_and_leave_the_rest_of_the_file() {
    let dir = TempDir::new("spec-fields");
    let path = file(
        &dir,
        "name = \"main\"\n[files]\nwrite = [\"a\", \"b\"]\nwrite_dot_files = true\n[tool.code]\nlimit = 3\n",
    );
    let made = read_with::<Agent>(
        Some(&path),
        &flags(&["--files.write", "/c", "--tool.code.enabled=true"]),
    )
    .unwrap();
    assert_eq!(made.spec.name.as_deref(), Some("main"));
    assert_eq!(made.spec.files.write, ["/c"]);
    assert!(made.spec.files.write_dot_files);
    assert_eq!(made.spec.tool["code"].limit, Some(3));
    assert_eq!(made.spec.tool["code"].enabled, Some(true));
    assert_eq!(made.dir, real(&dir));
    // A file's relative paths are from its own folder.
    let alone = read_with::<Agent>(Some(&path), &FieldFlags::default()).unwrap();
    let base = real(&dir);
    assert_eq!(
        alone.spec.files.write,
        [base.join("a"), base.join("b")].map(|p| p.display().to_string())
    );
}

#[test]
fn a_bad_value_is_refused_alike_from_either_input() {
    let dir = TempDir::new("spec-fields");
    let path = file(&dir, "[model]\nthinking = \"lots\"\n");
    let from_file = format!(
        "{:#}",
        read_with::<Agent>(Some(&path), &FieldFlags::default()).unwrap_err()
    );
    let from_flags = format!(
        "{:#}",
        read_with::<Agent>(None, &flags(&["--model.thinking", "lots"])).unwrap_err()
    );
    for said in [&from_file, &from_flags] {
        assert!(said.contains("unknown variant `lots`"), "{said}");
    }
    assert!(from_file.contains("line 3"), "{from_file}");
    let flagged = format!(
        "{:#}",
        read_with::<Agent>(Some(&path), &flags(&["--spec.name", "a"])).unwrap_err()
    );
    assert!(flagged.contains("line 3"), "{flagged}");
    let bad_flag = format!(
        "{:#}",
        read_with::<Agent>(
            Some(&file(&dir, "name = \"a\"\n")),
            &flags(&["--model.thinking", "lots"])
        )
        .unwrap_err()
    );
    assert!(
        bad_flag.contains("with --model.thinking over"),
        "{bad_flag}"
    );
}

#[test]
fn an_old_files_steps_apply_before_the_flags() {
    let dir = TempDir::new("spec-fields");
    let path = file(&dir, "schema = 1\ntitle = \"old\"\n");
    let made = read_with::<Agent>(Some(&path), &flags(&["--spec.version", "1"])).unwrap();
    assert_eq!(made.spec.name.as_deref(), Some("old"));
    assert_eq!(made.spec.version, Some(1));
}

#[test]
fn spec_show_prints_what_the_reader_takes_back() {
    let dir = TempDir::new("spec-fields");
    let path = file(&dir, "name = \"main\"\n");
    let show_file = Show {
        spec: Some(path.clone()),
        ..Show::default()
    };
    let printed = show::<Agent>(&show_file, &flags(&["--model.thinking", "low"]), None).unwrap();
    assert!(
        printed.starts_with("kind = \"agent\"\nschema = 2\n"),
        "{printed}"
    );
    let back: Agent = crate::parse_spec(&printed).unwrap();
    assert_eq!(back.name.as_deref(), Some("main"));
    assert_eq!(back.model.unwrap().thinking, Some(Thinking::Low));
    // Flags alone, and as JSON.
    let alone = Show {
        output: crate::shared::Output { json: true },
        ..Show::default()
    };
    let json = show::<Agent>(&alone, &flags(&["--spec.name", "x"]), None).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["name"], "x");
    // A thing, through its owner.
    let thing = Show {
        thing: Some("main".into()),
        ..Show::default()
    };
    let owner = |name: &str| {
        Ok(Agent {
            name: Some(name.to_string()),
            ..Agent::default()
        })
    };
    let printed = show::<Agent>(&thing, &flags(&["--spec.version", "4"]), Some(&owner)).unwrap();
    let back: Agent = crate::parse_spec(&printed).unwrap();
    assert_eq!(
        (back.name.as_deref(), back.version),
        (Some("main"), Some(4))
    );
    let unowned = show::<Agent>(&thing, &FieldFlags::default(), None).unwrap_err();
    assert!(unowned.to_string().contains("names no thing"), "{unowned}");
    let both = Show {
        thing: Some("main".into()),
        spec: Some(path),
        output: crate::shared::Output { json: false },
    };
    assert!(show::<Agent>(&both, &FieldFlags::default(), Some(&owner)).is_err());
    // Showing a file never rewrites it.
    assert_eq!(
        std::fs::read_to_string(dir.path().join("agent.toml")).unwrap(),
        "schema = 2\nname = \"main\"\n"
    );
}

#[test]
fn help_lists_every_field_flag_with_its_doc_comment() {
    let help = help(&crate::schema::<Agent>());
    for line in [
        "--model.thinking <off|low|high>",
        "How much the model reasons before it answers.",
        "--files.write <text>...",
        "--files.write_dot_files\n",
        "--spec.version <integer>",
        "--variables.<name> <text>",
        "--service '{ … }'...",
        "--spec.version <integer>",
    ] {
        assert!(help.contains(line), "{line} in\n{help}");
    }
}

#[test]
fn a_top_level_field_named_spec_kind_or_schema_is_refused() {
    #[expect(dead_code, reason = "only its shape is read")]
    #[derive(JsonSchema)]
    struct Clashing {
        spec: Option<String>,
        kind: Option<String>,
        name: Option<String>,
    }
    let problems = check_spelling(&crate::schema::<Clashing>());
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(check_spelling(&crate::schema::<Agent>()).is_empty());
}

#[test]
fn a_path_is_only_where_the_shape_says() {
    let schema = crate::schema::<Agent>();
    let at = |p: &[&str]| {
        resolve(
            &schema,
            &p.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
        .ok()
        .unwrap()
    };
    assert!(at(&["env"]).path());
    assert!(!at(&["name"]).path());
}

#[test]
fn every_declared_key_is_reached_by_a_flag() {
    assert_eq!(uncovered(&crate::schema::<Agent>()), Vec::<String>::new());
    assert_eq!(uncovered(&crate::schema::<Grown>()), Vec::<String>::new());
}

/// The review's cases: shapes the first walk missed or mis-read.
#[derive(Deserialize, Serialize, JsonSchema, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Odd {
    pub on: Option<bool>,
    #[serde(default)]
    pub meta: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub grid: Vec<Vec<String>>,
    #[serde(default)]
    pub anys: Vec<serde_json::Value>,
    #[serde(rename = "a.b")]
    pub dotted: Option<Model>,
    pub inner: Option<Box<Odd>>,
}

impl crate::Specification for Odd {
    fn format() -> &'static crate::schema::Format {
        <Agent as crate::Specification>::format()
    }
}
impl crate::Kind for Odd {}

fn odd(words: &[&str]) -> anyhow::Result<toml::Table> {
    let (cwd, home) = here();
    let binding = Binding {
        schema: crate::schema::<Odd>(),
        format: <Agent as crate::Specification>::format().clone(),
    };
    let (flags, rest) = split_from(args(words), Some(&binding), None, &[], &cwd, &home)?;
    assert!(rest.is_empty(), "{rest:?}");
    Ok(flags.table().clone())
}

#[test]
fn date_like_text_is_text_where_a_string_is_wanted() {
    let table = flags(&["--spec.name", "2026-11-10", "--variables.AT", "12:30:00"])
        .table()
        .clone();
    assert_eq!(table["name"].as_str(), Some("2026-11-10"));
    assert_eq!(table["variables"]["AT"].as_str(), Some("12:30:00"));
}

#[test]
fn a_switch_takes_a_following_true_or_false_and_nothing_else() {
    assert_eq!(
        odd(&["--on", "false"]).unwrap()["on"].as_bool(),
        Some(false)
    );
    let (cwd, home) = here();
    let (flags, rest) = split_from(
        args(&["--files.write_dot_files", "main"]),
        Some(&Binding::of::<Agent>()),
        None,
        &[],
        &cwd,
        &home,
    )
    .unwrap();
    assert_eq!(
        flags.table()["files"]["write_dot_files"].as_bool(),
        Some(true)
    );
    assert_eq!(rest, args(&["main"]));
    // A dash before a word is another flag, never a value.
    assert!(refused(&["--spec.name", "-v"]).contains("needs a value"));
    assert_eq!(flags_value(&["--spec.version", "-1"]), Some(-1));
}

fn flags_value(words: &[&str]) -> Option<i64> {
    flags(words).table()["version"].as_integer()
}

#[test]
fn free_keys_with_open_values_and_lists_of_lists_take_flags() {
    let table = odd(&[
        "--meta.region",
        "eu",
        "--grid",
        "[\"a\"]",
        "--grid",
        "[\"b\", \"c\"]",
        "--anys",
        "1",
        "--anys",
        "x",
        "--\"a.b\".thinking",
        "low",
    ])
    .unwrap();
    assert_eq!(table["meta"]["region"].as_str(), Some("eu"));
    assert_eq!(table["grid"].as_array().unwrap().len(), 2);
    assert_eq!(table["anys"].as_array().unwrap().len(), 2);
    assert_eq!(table["a.b"]["thinking"].as_str(), Some("low"));
    let schema = crate::schema::<Odd>();
    assert_eq!(uncovered(&schema), Vec::<String>::new());
    // A type inside itself is listed once, as the shape walk lists it.
    let flags: Vec<String> = fields(&schema).into_iter().map(|f| f.flag).collect();
    assert!(flags.contains(&"inner".to_string()), "{flags:?}");
    assert!(!flags.iter().any(|f| f.starts_with("inner.")), "{flags:?}");
}

#[test]
fn a_tables_flag_replaces_the_files_table_and_an_empty_one_is_refused_alike() {
    let dir = TempDir::new("spec-fields");
    let path = file(
        &dir,
        "[variables]\nA = \"1\"\n[tool.code]\nenabled = true\nlimit = 3\n",
    );
    let made = read_with::<Agent>(Some(&path), &flags(&["--tool.code", "{ limit = 4 }"])).unwrap();
    assert_eq!(made.spec.tool["code"].limit, Some(4));
    assert_eq!(made.spec.tool["code"].enabled, None);
    assert_eq!(made.spec.variables["A"], "1");
    // A file can't hold an empty table, so neither can a flag.
    let said = format!(
        "{:#}",
        read_with::<Agent>(Some(&path), &flags(&["--variables={}"])).unwrap_err()
    );
    assert!(said.contains("[variables] is empty"), "{said}");
}

#[test]
fn a_nearest_flag_is_suggested_with_the_free_key_given() {
    let said = refused(&["--tool.code.enable=true"]);
    assert!(
        said.contains("the nearest is --tool.code.enabled"),
        "{said}"
    );
}

#[test]
fn a_current_file_with_flags_is_never_taken_for_an_old_one() {
    let dir = TempDir::new("spec-fields");
    let path = file(&dir, "schema = 2\nname = \"main\"\ntitle = \"x\"\n");
    let said = format!(
        "{:#}",
        read_with::<Agent>(Some(&path), &flags(&["--spec.version", "1"])).unwrap_err()
    );
    assert!(!said.contains("add schema"), "{said}");
}

#[test]
fn a_table_is_checked_against_the_shape_alone() {
    let schema = crate::schema::<Agent>();
    let good: toml::Table = toml::from_str(
        "kind = \"agent\"\nname = \"a\"\n[model]\nthinking = \"low\"\n[tool.code]\nenabled = true\n[[service]]\nname = \"w\"\nrun = \"r\"\n",
    )
    .unwrap();
    assert_eq!(check_value(&schema, &good), Vec::<String>::new());
    let bad: toml::Table =
        toml::from_str("title = \"a\"\n[model]\nthinking = 3\n[tool.code]\nlimit = \"x\"\n")
            .unwrap();
    let problems = check_value(&schema, &bad);
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("title is no field")));
}

#[test]
fn convert_rewrites_an_old_file_by_steps_alone_keeping_its_comments() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("spec-fields");
    let path = file(
        &dir,
        "# my agent\nschema = 1\ntitle = \"main\" # its name\n[model]\nthinking = \"low\"\n",
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    let exported = crate::Exported::of::<Agent>();
    let converted = crate::convert(&exported.format(), &exported.shape, &path).unwrap();
    assert_eq!(
        converted,
        crate::Converted {
            from: 1,
            changed: true
        }
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# my agent\nkind = \"agent\"\n"), "{text}");
    assert!(text.contains("name = \"main\" # its name"), "{text}");
    assert!(text.contains("schema = 2"), "{text}");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640);
    assert!(crate::parse_spec::<Agent>(&text).is_ok());
    // Current, it's left alone.
    let again = crate::convert(&exported.format(), &exported.shape, &path).unwrap();
    assert!(!again.changed);
    // A file of another kind, or one its steps can't make current, is refused.
    let other = file(&dir, "kind = \"box\"\n");
    assert!(crate::convert(&exported.format(), &exported.shape, &other).is_err());
    let unknown = file(&dir, "schema = 2\nbogus = 1\n");
    let said = crate::convert(&exported.format(), &exported.shape, &unknown).unwrap_err();
    assert!(said.to_string().contains("bogus is no field"), "{said}");
    assert_eq!(
        std::fs::read_to_string(&unknown).unwrap(),
        "schema = 2\nbogus = 1\n"
    );
}

#[test]
fn an_old_nested_flag_is_followed_through_every_later_step() {
    let format = crate::schema::Format::parse(
        "schema = 1\nkind = 'k'\ncurrent = 3\ncommand = 'lnk k spec show --spec'\n\
         [[steps]]\nfrom = 1\nsince = '0.1.0'\nuntil = '99.0.0'\nrename = { oldmodel = 'model' }\n\
         [[steps]]\nfrom = 2\nsince = '0.1.0'\nuntil = '99.0.0'\nrename = { 'model.name' = 'model.thinking' }\nremove = { 'model.unused' = 'It does nothing.' }\n",
    )
    .unwrap();
    let binding = Binding {
        schema: crate::schema::<Agent>(),
        format,
    };
    let path = |p: &str| p.split('.').map(str::to_string).collect::<Vec<_>>();
    assert_eq!(
        binding.retired(&path("oldmodel.name")).unwrap().message,
        "--oldmodel.name is gone since k schema 2. Use --model.thinking."
    );
    assert_eq!(
        binding.retired(&path("oldmodel.unused")).unwrap().message,
        "--oldmodel.unused is gone since k schema 2. It does nothing."
    );
    assert!(binding.retired(&path("model.thinking")).is_none());
    // A step can't give a field and one inside it two fates.
    assert!(
        crate::schema::Format::parse(
            "schema = 1\nkind = 'k'\ncurrent = 2\ncommand = 'lnk k spec show --spec'\n[[steps]]\nfrom = 1\nsince = '0.1.0'\nuntil = '99.0.0'\nrename = { oldmodel = 'model' }\nremove = { 'oldmodel.unused' = 'x' }\n"
        )
        .is_err()
    );
}

#[test]
fn convert_puts_kind_and_schema_first_and_keeps_line_endings() {
    let dir = TempDir::new("spec-fields");
    let exported = crate::Exported::of::<Agent>();
    let convert = |text: &str| {
        let path = file(&dir, text);
        crate::convert(&exported.format(), &exported.shape, &path).unwrap();
        std::fs::read_to_string(&path).unwrap()
    };
    // The first item a table: its comment stays at the top.
    let text = convert("#:schema ./agent.json\nschema = 2\n[model]\nthinking = \"low\"\n");
    assert!(
        text.starts_with("#:schema ./agent.json\nkind = \"agent\"\nschema = 2\n"),
        "{text}"
    );
    // A key renamed keeps its place, and kind after others moves first.
    let text = convert("title = \"a\"\nkind = \"agent\"\nschema = 1\nversion = 1\n");
    assert!(
        text.starts_with("kind = \"agent\"\nschema = 2\nname = \"a\"\nversion = 1\n"),
        "{text}"
    );
    let text = convert("schema = 2\r\nname = \"a\"\r\n");
    assert_eq!(text, "kind = \"agent\"\r\nschema = 2\r\nname = \"a\"\r\n");
}

/// A runtime flag's value before a command's program is no program's
/// word: field flags after it are still the command's.
#[test]
fn a_runtime_flags_value_never_starts_the_program() {
    let (cwd, home) = here();
    let trailing = crate::Trailing {
        words: 2,
        valued: vec!["upstream".into()],
    };
    let (flags, rest) = split_from(
        args(&[
            "run",
            "--upstream",
            "direct",
            "--files.write",
            "x",
            "sh",
            "--files.read",
            "y",
        ]),
        Some(&Binding::of::<Agent>()),
        Some(trailing),
        &[],
        &cwd,
        &home,
    )
    .unwrap();
    assert!(flags.get(&["files", "write"]).is_some());
    assert!(flags.get(&["files", "read"]).is_none());
    assert!(rest.iter().any(|a| a == "--files.read"), "{rest:?}");
}

/// A spec printed as field flags reads back as itself.
#[test]
fn a_spec_as_its_flags_reads_back() {
    let (cwd, home) = here();
    let spec: Agent = crate::parse_spec(
        "schema = 2\n[model]\nthinking = \"high\"\n[files]\nwrite = [\"/a\", \"/b\"]\n",
    )
    .unwrap();
    let words = crate::flags_of(&spec).unwrap();
    assert!(words.contains(&"--model.thinking".to_string()), "{words:?}");
    let (flags, _) = split_from(
        words.iter().map(OsString::from),
        Some(&Binding::of::<Agent>()),
        None,
        &[],
        &cwd,
        &home,
    )
    .unwrap();
    assert_eq!(over(&Agent::default(), &flags).unwrap(), spec);
}

#[test]
fn text_read_elsewhere_takes_the_flags_over_it() {
    let text = "schema = 2\nname = \"a\"\nversion = 1\n[model]\nthinking = \"low\"\n";
    let alone: Agent = super::read_text_with(text, &FieldFlags::default()).unwrap();
    assert_eq!(alone.name.as_deref(), Some("a"));
    let over: Agent = super::read_text_with(text, &flags(&["--model.thinking", "high"])).unwrap();
    assert_eq!(over.name.as_deref(), Some("a"));
    assert_eq!(
        over.model.and_then(|m| m.thinking),
        Some(Thinking::High),
        "the flag wins"
    );
    let empty: Agent = super::read_text_with("", &flags(&["--spec.name", "b"])).unwrap();
    assert_eq!(empty.name.as_deref(), Some("b"));
    let unknown =
        super::read_text_with::<Agent>("schema = 2\nnope = 1\n", &flags(&["--spec.name", "b"]));
    assert!(unknown.is_err());
}

/// What a kind's own check refuses, every read with flags and every
/// `spec show` refuses, so a printed spec is one its commands take back.
#[test]
fn every_read_applies_the_kinds_own_check() {
    let named = flags(&["--spec.name", "unchecked"]);
    let refused = |e: anyhow::Error| format!("{e:#}").contains("no agent is named unchecked");
    let text = super::read_text_with::<Agent>("", &named).unwrap_err();
    assert!(refused(text));
    let alone = super::read_text_with::<Agent>(
        "schema = 2\nname = \"unchecked\"\n",
        &FieldFlags::default(),
    )
    .unwrap_err();
    assert!(refused(alone));
    let over = super::over(&Agent::default(), &named).unwrap_err();
    assert!(refused(over));
    let shown = super::show::<Agent>(&super::Show::default(), &named, None).unwrap_err();
    assert!(refused(shown));
}

/// The test kind, by a name its sibling modules use.
type Named = Agent;

mod contract;
mod given;
mod paths;
mod sensitive;
mod short;

/// The example spec the short-flag tests list.
type Example = Agent;

#[test]
fn a_lone_dash_is_a_flags_value_as_its_spaced_form_takes_it() {
    assert_eq!(
        flags(&["--spec.name", "-"]).table()["name"].as_str(),
        Some("-")
    );
    assert_eq!(
        flags(&["--spec.name=-"]).table()["name"].as_str(),
        Some("-")
    );
}

#[test]
fn a_table_and_a_field_inside_it_are_refused_in_either_order() {
    let whole = "--model={thinking = \"high\"}";
    for words in [
        [whole, "--model.thinking=low"],
        ["--model.thinking=low", whole],
    ] {
        let said = refused(&words);
        assert!(said.contains("one inside the other"), "{words:?}: {said}");
    }
}
