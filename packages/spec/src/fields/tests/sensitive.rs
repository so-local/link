use super::*;
use std::path::Path;

/// A spec whose `relay.token` and each of `tokens` hold a secret: a flag
/// naming one, or a table or list holding one, is refused whatever its
/// value, while its other fields' flags are taken.
fn binding() -> Binding {
    Binding {
        schema: serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "name": { "type": "string" },
                "relay": { "$ref": "#/$defs/relay" },
                "tokens": { "type": "array", "items": { "type": "string", "x-link-sensitive": true } }
            },
            "$defs": {
                "relay": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": { "token": { "type": "string", "x-link-sensitive": true } }
                }
            }
        }),
        format: crate::schema::Format::parse(
            "schema = 1\nkind = \"secretive\"\ncurrent = 1\ncommand = \"lnk x spec show\"\n",
        )
        .unwrap(),
    }
}

fn split(words: &[&str]) -> anyhow::Result<FieldFlags> {
    let root = Path::new("/");
    super::super::given::split_from(args(words), Some(&binding()), None, &[], root, root)
        .map(|(f, _)| f)
}

#[test]
fn a_sensitive_field_takes_no_flag() {
    for words in [
        &["--relay.token", "hunter2"][..],
        &["--relay.token=hunter2"],
        &["--relay", "{ token = \"hunter2\" }"],
        &["--tokens", "hunter2"],
        &["--tokens", "[\"hunter2\"]"],
    ] {
        let refused = split(words).unwrap_err().to_string();
        assert!(refused.contains("is a secret's"), "{words:?}: {refused}");
        assert!(
            !refused.contains("hunter2"),
            "the value is never repeated: {refused}"
        );
    }
    assert!(!split(&["--spec.name", "web"]).unwrap().is_empty());
}
