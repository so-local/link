use super::*;
use crate::history::History;

/// A release whose core renamed `memory-tool` to `notes-tool` and
/// removed `pager-tool` and `webhook`, and whose `notes-tool` renamed its
/// call `search` to `find` and removed `links`.
fn release() -> References {
    let core = History::parse(
        "[[renamed]]\nmodule = 'memory-tool'\nnow = 'notes-tool'\nsince = '1.1.0'\nuntil = '2.0.0'\n\
         [[removed]]\nmodule = 'pager-tool'\nwhy = 'Use the notes tool.'\nsince = '1.1.0'\nuntil = '2.0.0'\n\
         [[removed]]\nmodule = 'webhook'\nwhy = 'Use lnk webhook-connection.'\nsince = '1.1.0'\nuntil = '2.0.0'\n",
    )
    .unwrap();
    let notes = History::parse(
        "[[gone]]\nwas = 'notes-tool search'\nnow = 'lnk notes-tool find'\nsince = '1.1.0'\nuntil = '2.0.0'\n\
         [[gone]]\nwas = 'notes-tool links'\nwhy = 'Read the page instead.'\nsince = '1.1.0'\nuntil = '2.0.0'\n",
    )
    .unwrap();
    // As the core answers it, over the wire.
    let wire = core.release_wire([("notes-tool", &notes)]);
    let wire: Wire = serde_json::from_str(&serde_json::to_string(&wire).unwrap()).unwrap();
    References::from(wire)
}

#[test]
fn a_renamed_name_is_its_current_one_and_a_removed_one_is_refused_by_name() {
    let release = release();
    assert_eq!(release.tool("memory").unwrap(), "notes");
    assert_eq!(release.tool("web").unwrap(), "web");
    assert_eq!(release.module("memory-tool").unwrap(), "notes-tool");
    assert_eq!(
        release.command("memory-tool search").unwrap(),
        "notes-tool find"
    );
    assert_eq!(release.command("box show").unwrap(), "box show");
    for (kind, name, said) in [
        (
            Reference::Tool,
            "pager",
            "pager-tool module is removed since 1.1.0",
        ),
        (Reference::Module, "webhook", "Use lnk webhook-connection."),
        (
            Reference::Command,
            "webhook start",
            "webhook module is removed",
        ),
        (
            Reference::Command,
            "memory-tool links",
            "notes-tool links is gone since 1.1.0",
        ),
    ] {
        let error = release.follow(kind, name).unwrap_err();
        assert!(format!("{error:#}").contains(said), "{name}: {error:#}");
    }
}

/// A replayed call is renamed where its tool or command was, and keeps
/// its name where they're removed or aren't Link's: never refused.
#[test]
fn a_recorded_call_follows_its_tool_and_command_and_a_removed_one_keeps_its_name() {
    let release = release();
    for (recorded, replayed) in [
        ("memory__search", "notes__find"),
        ("memory__remember", "notes__remember"),
        ("notes__search", "notes__find"),
        ("memory__links", "memory__links"),
        ("pager__page", "pager__page"),
        ("gh__list_issues", "gh__list_issues"),
        ("plain", "plain"),
    ] {
        assert_eq!(release.call(recorded), replayed, "{recorded}");
    }
    // With no history, every name is as written.
    assert_eq!(
        References::default().call("memory__search"),
        "memory__search"
    );
}

fn shape() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "tools": { "$ref": "#/$defs/tools" },
            "tool": {
                "type": "object",
                "propertyNames": { "type": "string", "x-link-ref": "tool" },
                "additionalProperties": { "type": "object", "properties": {
                    "after": { "type": "string", "x-link-ref": "command" }
                }}
            },
            "hosts": { "type": "array", "items": { "type": "string" } }
        },
        "$defs": { "tools": {
            "type": "object",
            "properties": {
                "names": { "type": "array", "items": { "type": "string", "x-link-ref": "tool" } },
                "linux": { "$ref": "#/$defs/tools" }
            }
        }}
    })
}

#[test]
fn a_shape_says_where_its_names_are() {
    let shown: Vec<_> = sites(&shape())
        .iter()
        .map(|site| (site.shown(), site.kind, site.keys))
        .collect();
    assert!(
        shown.contains(&("tools.names".into(), Reference::Tool, false)),
        "{shown:?}"
    );
    assert!(
        shown.contains(&("tools.linux.names".into(), Reference::Tool, false)),
        "{shown:?}"
    );
    assert!(
        shown.contains(&("tool".into(), Reference::Tool, true)),
        "{shown:?}"
    );
    assert!(
        shown.contains(&("tool.*.after".into(), Reference::Command, false)),
        "{shown:?}"
    );
    assert!(
        !shown.iter().any(|(path, ..)| path.starts_with("hosts")),
        "{shown:?}"
    );
}

/// A definition naming itself, as each sandbox section names its own for
/// each platform's overlay, is walked once more for the overlay and never
/// a third time: its places are its own and each overlay's, not one per
/// nesting the walk could reach.
#[test]
fn a_shape_naming_itself_has_its_overlays_places_and_no_more() {
    let shape = serde_json::json!({
        "type": "object",
        "properties": { "tools": { "$ref": "#/$defs/tools" } },
        "$defs": { "tools": {
            "type": "object",
            "properties": {
                "names": { "type": "array", "items": { "type": "string", "x-link-ref": "tool" } },
                "macos": { "$ref": "#/$defs/tools" },
                "linux": { "$ref": "#/$defs/tools" }
            }
        }}
    });
    let mut shown: Vec<_> = sites(&shape).iter().map(Site::shown).collect();
    shown.sort();
    assert_eq!(
        shown,
        ["tools.linux.names", "tools.macos.names", "tools.names"]
    );
}

#[test]
fn a_document_reads_its_names_as_they_are_today() {
    let format =
        link_files::schema::Format::parse("schema = 1\nkind = 'example'\ncurrent = 1\n").unwrap();
    let read = format
        .read_following::<toml::Table>(
            "[tools]\nnames = ['memory', 'gh']\n[tool.memory]\nafter = 'memory-tool search'\n",
            &sites(&shape()),
            &release(),
        )
        .unwrap();
    assert_eq!(
        read.spec["tools"]["names"],
        toml::Value::from(vec!["notes", "gh"])
    );
    assert_eq!(
        read.spec["tool"]["notes"]["after"].as_str(),
        Some("notes-tool find")
    );
    let refused = format.read_following::<toml::Table>(
        "[tools]\nnames = ['pager']\n",
        &sites(&shape()),
        &release(),
    );
    let error = refused.unwrap_err();
    assert!(format!("{error:#}").contains("tools.names"), "{error:#}");
    assert!(
        format!("{error:#}").contains("pager-tool module is removed"),
        "{error:#}"
    );
}
