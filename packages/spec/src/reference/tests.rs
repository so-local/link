use super::*;
use crate::fields::tests::Agent;

/// The sections of a kind's shape, each under its H2, as a page writes
/// them.
fn sections(shape: &Value, composed: &[Composed<'_>]) -> String {
    key_sections(shape, composed)
        .into_iter()
        .map(|(key, body)| format!("\n## `{key}`\n\n{body}"))
        .collect()
}

#[test]
fn each_top_level_key_has_its_section_and_each_field_its_entry() {
    let page = sections(&Exported::of::<Agent>().shape, &[]);
    assert!(page.contains("\n## `model`\n"), "{page}");
    assert!(page.contains("\n### `thinking`\n\n> **Type** one of `off`, `low`, `high` · **Flag** `--model.thinking`\n\nHow much the model reasons before it answers.\n"), "{page}");
    assert!(page.contains("\n## `variables`\n"), "{page}");
    // A list's element fields have entries, set in the list's flag.
    assert!(
        page.contains("\n### `name`\n\n> **Type** text · **Flag** in `--service`, whole elements"),
        "{page}"
    );
    assert!(page.contains("**Flag** `--service`"), "{page}");
}

#[test]
fn every_option_reads_back_through_the_kinds_reader() {
    let kind = Exported::of::<Agent>();
    let text = every_option_text(&kind);
    assert!(text.starts_with("kind = \"agent\"\nschema = 2\n"), "{text}");
    let back: Agent = crate::parse_spec(&text).unwrap();
    assert_eq!(
        back.model.unwrap().thinking,
        Some(crate::fields::tests::Thinking::Off)
    );
    assert!(!back.service.is_empty() && !back.tool.is_empty());
}

/// A shape with what a page must show whole: a tagged enum whose
/// definition gives an example, a table that gives one, a type inside
/// itself and fields deeper than H4.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[allow(dead_code, reason = "read only through its schema and serde")]
struct Shapes {
    /// When it runs.
    when: Option<When>,
    /// The machine it asks for.
    machine: Option<Machine>,
    /// The browser, with overrides per system.
    browser: Option<Browser>,
    /// Four levels down.
    a: Option<A>,
}

/// When something runs.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
#[schemars(extend("examples" = [{"kind": "cron", "at": "0 9 * * *"}]))]
#[allow(dead_code, reason = "read only through its schema and serde")]
enum When {
    /// Every few seconds.
    Every { seconds: u32 },
    /// At a cron time.
    Cron { at: String },
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("examples" = [{"cpus": 2}]))]
#[allow(dead_code, reason = "read only through its schema and serde")]
struct Machine {
    /// Its processors.
    cpus: Option<u32>,
    spot: Option<bool>,
}

/// Opening a page in your browser.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[allow(dead_code, reason = "read only through its schema and serde")]
struct Browser {
    /// Its program.
    program: Option<String>,
    linux: Option<Box<Browser>>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[allow(dead_code, reason = "read only through its schema and serde")]
struct A {
    /// B.
    b: Option<B>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[allow(dead_code, reason = "read only through its schema and serde")]
struct B {
    /// C.
    c: Option<C>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[allow(dead_code, reason = "read only through its schema and serde")]
struct C {
    /// D.
    d: Option<u32>,
}

#[test]
fn every_option_holds_every_field_and_one_variant() {
    let every = every_option(&crate::schema::<Shapes>());
    // The variant is its definition's example, never every variant's keys.
    assert_eq!(
        every["when"].to_string(),
        r#"{ at = "0 9 * * *", kind = "cron" }"#
    );
    // A table's own example doesn't stand for its fields.
    let machine = every["machine"].as_table().unwrap();
    assert!(
        machine.contains_key("cpus") && machine.contains_key("spot"),
        "{machine:?}"
    );
    // A type inside itself shows its fields once.
    assert!(every["browser"]["linux"]["program"].is_str(), "{every:?}");
    assert!(
        every["browser"]["linux"].get("linux").is_none(),
        "{every:?}"
    );
    let text = toml::to_string(&every).unwrap();
    toml::from_str::<Shapes>(&text).unwrap();
}

#[test]
fn a_deep_heading_shows_its_key_path() {
    let page = sections(&crate::schema::<Shapes>(), &[]);
    assert!(page.contains("\n#### `c`\n"), "{page}");
    assert!(page.contains("\n##### `a.b.c.d`\n"), "{page}");
}

#[test]
fn a_title_capitalizes_each_word_and_a_rustdoc_link_is_its_code() {
    assert_eq!(title("action-server"), "Action Server Specification");
    assert_eq!(title("vm"), "VM Specification");
    assert_eq!(
        plain("Like [`crate::access`], or [`x`](y)."),
        "Like `crate::access`, or [`x`](y)."
    );
}

/// A kind another holds whole is linked to its own page at the field
/// holding it, never inlined there.
#[test]
fn a_composed_kind_links_its_own_page() {
    let kind = Exported::of::<Agent>();
    let model = serde_json::to_value(schemars::schema_for!(crate::fields::tests::Model)).unwrap();
    let composed = [Composed {
        shape: &model,
        kind: "model",
        page: "model.md",
    }];
    let page = sections(&kind.shape, &composed);
    assert!(page.contains("\n## `model`\n"), "{page}");
    assert!(
        page.contains("Its fields are the [Model Specification](model.md)'s"),
        "{page}"
    );
    assert!(!page.contains("### `thinking`"), "{page}");
    // Every Option still holds the whole, which the reader takes back.
    let every = every_option_text(&kind);
    assert!(every.contains("[model]\nthinking = "), "{every}");
}

/// A field this build refuses is in the shape and on the page, marked
/// unavailable, and no example holds it; a table whose field gives an
/// example is that example whole.
#[test]
fn an_unavailable_field_is_marked_and_a_whole_table_example_holds() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "isolation": {
                "description": "Where it runs.",
                "type": "object",
                "properties": {
                    "vm": {
                        "description": "A VM's spec.",
                        "type": "string",
                        "x-link-unavailable": "this build has no VM provider"
                    },
                    "depth": {"description": "How deep.", "type": "integer"}
                }
            },
            "tool": {
                "description": "Each tool's own.",
                "type": "object",
                "additionalProperties": {"type": "object", "properties": {"a": {"type": "integer"}}},
                "examples": [{"web": {"a": 2}}]
            }
        }
    });
    let every = every_option(&schema);
    assert!(every["isolation"].get("vm").is_none(), "{every:?}");
    assert!(every["isolation"].get("depth").is_some(), "{every:?}");
    assert_eq!(every["tool"]["web"]["a"].as_integer(), Some(2), "{every:?}");
    let at = At::top(&schema);
    let vm = at.child("isolation").and_then(|i| i.child("vm")).unwrap();
    assert!(line(&vm, &["isolation".into(), "vm".into()]).contains("**Unavailable**"));
}

/// A closed choice whose declaration says what each word means lists
/// each word with its meaning under the field, in declared order, a
/// list's for its elements' words too.
#[test]
fn a_choice_lists_what_each_word_means() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "commands": {
                "description": "What it runs.",
                "type": "object",
                "properties": {
                    "toolchains": {
                        "description": "Toolchains.",
                        "type": "array",
                        "items": {"$ref": "#/$defs/toolchain"}
                    }
                }
            }
        },
        "$defs": {
            "toolchain": {
                "type": "string",
                "enum": ["shell", "git"],
                "x-link-values": {"git": "Git and ssh", "shell": "A shell"}
            }
        }
    });
    let page = sections(&schema, &[]);
    assert!(
        page.contains(
            "> **Type** a list, each one of `shell`, `git` · **Flag** `--commands.toolchains`\n\n\
             Toolchains.\n\n- `shell`: A shell\n- `git`: Git and ssh\n"
        ),
        "{page}"
    );
}
