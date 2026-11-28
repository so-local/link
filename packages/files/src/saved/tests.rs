use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Kept {
    #[serde(default)]
    place: Option<String>,
    #[serde(default)]
    names: std::collections::BTreeMap<String, String>,
}

crate::specification!(
    Kept,
    r#"
schema = 1
kind = "kept"
current = 2
command = "lnk location"

[[steps]]
from = 1
since = "0.0.1"
until = "0.0.2"
rename = { where = "place" }
remove = { old = "nothing reads it" }
"#
);

/// A file of the schema before is advanced through its step, never
/// refused; one with no schema is refused as none Link wrote; one newer
/// is refused,
/// naming the upgrade; and a write names the current schema.
#[test]
fn a_saved_file_advances_through_its_steps_and_refuses_a_newer_one() {
    let dir = link_test_kit::TempDir::new("saved");
    let path = dir.join("kept.toml");
    assert_eq!(read::<Kept>(&path).unwrap(), Kept::default());
    crate::write_private(&path, b"schema = 1\nwhere = \"Docs\"\nold = 1\n").unwrap();
    assert_eq!(read::<Kept>(&path).unwrap().place.as_deref(), Some("Docs"));
    crate::write_private(&path, b"place = \"Docs\"\n").unwrap();
    let refused = format!("{:#}", read::<Kept>(&path).unwrap_err());
    assert!(refused.contains("names no schema"), "{refused}");
    crate::write_private(&path, b"schema = 3\nplace = \"Docs\"\n").unwrap();
    let refused = format!("{:#}", read::<Kept>(&path).unwrap_err());
    assert!(refused.contains("lnk upgrade"), "{refused}");
    let kept = Kept {
        place: Some("Docs".into()),
        names: [("a".into(), "b".into())].into(),
    };
    write(&path, &kept).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("schema = 2\n"), "{text}");
    assert_eq!(read::<Kept>(&path).unwrap(), kept);
    // An empty table a writer leaves is read as a missing one.
    assert_eq!(
        parse::<Kept>("schema = 2\n[names]\n").unwrap(),
        Kept::default()
    );
}

#[test]
fn a_saved_json_file_advances_through_the_same_steps() {
    let kept: Kept = parse_json(br#"{"schema":1,"where":"Docs","old":true}"#).unwrap();
    assert_eq!(kept.place.as_deref(), Some("Docs"));
    let refused = parse_json::<Kept>(br#"{"place":"Docs"}"#).unwrap_err();
    assert!(refused.to_string().contains("names no schema"), "{refused}");
    let kept: Kept = parse_json(br#"{"schema":2,"place":"Docs"}"#).unwrap();
    assert_eq!(kept.place.as_deref(), Some("Docs"));
    assert!(parse_json::<Kept>(br#"{"schema":3}"#).is_err());
    assert!(parse_json::<Kept>(br#"{"schema":1,"where":"a","place":"b"}"#).is_err());
    let bytes = print_json(&kept).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["schema"], 2);
    assert_eq!(parse_json::<Kept>(&bytes).unwrap(), kept);
}

/// Each line of a ledger names its own schema, so one written across an
/// upgrade holds lines of either, each advanced as it's read.
#[test]
fn every_line_of_a_ledger_names_its_schema() {
    let kept = Kept {
        place: Some("Docs".into()),
        names: Default::default(),
    };
    let written = line(&kept).unwrap();
    assert!(written.ends_with(b"\n") && !written[..written.len() - 1].contains(&b'\n'));
    let ledger = [
        br#"{"schema":1,"where":"Docs"}"#.as_slice(),
        b"\n",
        &written,
    ]
    .concat();
    let read: Vec<Kept> = ledger
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .map(|l| parse_line(l).unwrap())
        .collect();
    assert_eq!(read, [kept.clone(), kept]);
    assert!(parse_line::<Kept>(br#"{"schema":3,"place":"Docs"}"#).is_err());
}

#[test]
fn a_shipped_format_reads_and_writes_another_crates_type() {
    let format = crate::schema::Format::shipped(
        "schema = 1\nkind = \"names\"\ncurrent = 1\ncommand = \"lnk location\"\n",
    );
    let names: std::collections::BTreeMap<String, u32> = [("a".into(), 1)].into();
    let bytes = format.print_json(&names).unwrap();
    assert_eq!(bytes, br#"{"a":1,"schema":1}"#);
    // Empty, and a type's own field order kept with its schema last.
    let none: std::collections::BTreeMap<String, u32> = Default::default();
    assert_eq!(format.print_json(&none).unwrap(), br#"{"schema":1}"#);
    #[derive(Serialize)]
    struct Ordered {
        t: u32,
        a: u32,
    }
    assert_eq!(
        format.print_json(&Ordered { t: 2, a: 1 }).unwrap(),
        br#"{"t":2,"a":1,"schema":1}"#
    );
    assert!(format.print_json(&[1, 2]).is_err());
    assert_eq!(
        format
            .read_json::<std::collections::BTreeMap<String, u32>>(&bytes)
            .unwrap()
            .spec,
        names
    );
}

/// A write and an append take the file's writer lock, the one an upgrade
/// converting it holds: a writer holding it writes through it, and
/// another waits until it's let go.
#[test]
fn a_write_and_an_append_take_the_files_writer_lock() {
    let dir = link_test_kit::TempDir::new("saved-locked");
    let path = dir.join("kept.toml");
    let kept = |place: &str| Kept {
        place: Some(place.into()),
        ..Kept::default()
    };
    let held = lock(&path).unwrap();
    held.write(&kept("held")).unwrap();
    std::thread::scope(|scope| {
        let other = scope.spawn(|| write(&path, &kept("other")).unwrap());
        assert_eq!(read::<Kept>(&path).unwrap(), kept("held"));
        drop(held);
        other.join().unwrap();
    });
    assert_eq!(read::<Kept>(&path).unwrap(), kept("other"));
    assert!(dir.join("kept.toml.lock").is_file());
    let ledger = dir.join("kept.jsonl");
    append(&ledger, &kept("one")).unwrap();
    append(&ledger, &kept("two")).unwrap();
    let text = std::fs::read_to_string(&ledger).unwrap();
    let places: Vec<Kept> = text
        .lines()
        .map(|l| parse_line(l.as_bytes()).unwrap())
        .collect();
    assert_eq!(places, [kept("one"), kept("two")]);
    assert!(dir.join("kept.jsonl.lock").is_file());
}
