//! A kind whose step from schema 1 is its owner's code, registered in its
//! `migrations/` folder: its files of every schema reach the current type
//! through the steps in order.

use super::*;

#[derive(Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Disks {
    pub(super) name: String,
    pub(super) disk: Disk,
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Disk {
    pub(super) size: u32,
    pub(super) units: String,
}

crate::specification!(Disks, include_str!("coded/schema.toml"), migrations);

mod migrations;

fn web(size: u32, units: &str) -> Disks {
    Disks {
        name: "web".into(),
        disk: Disk {
            size,
            units: units.into(),
        },
    }
}

#[test]
fn a_fixture_of_the_first_schema_reaches_the_current_type() {
    let fixture = include_str!("../../../tests/fixtures/disks-1.toml");
    let read = Disks::format().read::<Disks>(fixture).unwrap();
    assert_eq!(read.spec, web(20, "G"));
    assert_eq!(read.from, 1);
    assert!(read.changed);
    // What the code left alone keeps its comments and place.
    assert!(read.text.starts_with("# Disks saved in the first schema.\nschema = 3\n# The machine's name.\nname = \"web\"\n"), "{}", read.text);
    assert_eq!(
        crate::saved::parse::<Disks>(&read.text).unwrap(),
        web(20, "G")
    );
}

#[test]
fn every_later_schema_skips_the_code_it_has_passed() {
    for text in [
        "schema = 2\nname = \"web\"\n[disk]\nsize = 20\nunit = \"G\"\n",
        "schema = 3\nname = \"web\"\n[disk]\nsize = 20\nunits = \"G\"\n",
        "name = \"web\"\n[disk]\nsize = 20\nunits = \"G\"\n",
    ] {
        assert_eq!(
            crate::saved::parse::<Disks>(text).unwrap(),
            web(20, "G"),
            "{text}"
        );
    }
}

#[test]
fn a_saved_json_file_and_a_ledger_line_take_the_same_code() {
    let fixture = include_bytes!("../../../tests/fixtures/disks-1.json");
    let fixture = fixture.trim_ascii_end();
    assert_eq!(
        crate::saved::parse_json::<Disks>(fixture).unwrap(),
        web(512, "M")
    );
    assert_eq!(
        crate::saved::parse_line::<Disks>(fixture).unwrap(),
        web(512, "M")
    );
}

#[test]
fn what_the_code_refuses_is_refused_naming_the_schema() {
    let refused =
        crate::saved::parse::<Disks>("schema = 1\nname = \"web\"\ndisk = \"lots\"\n").unwrap_err();
    let said = format!("{refused:#}");
    assert!(
        said.contains("disks schema 1") && said.contains("lots"),
        "{said}"
    );
}

#[test]
fn a_format_exported_without_its_code_refuses_rather_than_skips_it() {
    let mut exported = Disks::format().clone();
    exported.migrations = &[];
    let refused = exported
        .read::<Disks>(include_str!("../../../tests/fixtures/disks-1.toml"))
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("disks schema 1 reaches schema 2 by its owner's code")
            && refused.contains("lnk disks spec show"),
        "{refused}"
    );
    // A file past the code needs none.
    assert!(
        exported
            .read::<Disks>("schema = 2\nname = \"web\"\n[disk]\nsize = 1\nunit = \"G\"\n")
            .is_ok()
    );
}
