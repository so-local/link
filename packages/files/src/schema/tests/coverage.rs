//! A format's steps answer every field they take away.

use super::*;
use serde_json::json;

/// A section holding its own shape for each OS, as a sandbox section
/// does: its keys, and its OS's, but never an OS's OS.
pub(super) fn section_schema(with_tcp: bool) -> serde_json::Value {
    let mut internet = json!({"type": "object", "properties": {
        "hosts": {"type": "array"},
        "linux": {"$ref": "#/$defs/internet"},
    }});
    if with_tcp {
        internet["properties"]["tcp"] = json!({"type": "array"});
    }
    json!({"type": "object",
        "properties": {"internet": {"$ref": "#/$defs/internet"}},
        "$defs": {"internet": internet}})
}

#[test]
fn a_shape_within_itself_is_walked_once() {
    let paths = field_paths(&section_schema(true));
    let expected: BTreeSet<String> = [
        "internet",
        "internet.hosts",
        "internet.tcp",
        "internet.linux",
        "internet.linux.hosts",
        "internet.linux.tcp",
    ]
    .map(String::from)
    .into();
    assert_eq!(paths, expected);
}
