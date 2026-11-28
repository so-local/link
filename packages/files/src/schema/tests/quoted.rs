//! A step names a key holding a dot by quoting it, `locations."lnk.migration-journal"`,
//! in a TOML document and a JSON object alike.

use super::*;

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Map {
    locations: BTreeMap<String, String>,
}

fn format() -> Format {
    Format::parse(
        r#"
schema = 1
kind = "map example"
current = 2
[[steps]]
from = 1
since = '0.0.1'
until = '0.0.2'
rename = { 'locations."lnk.migration-journal"' = 'locations."lnk.moves"' }
"#,
    )
    .unwrap()
}

fn current() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("lnk.moves".into(), "journal".into()),
        ("lnk.stopped-services".into(), "kept-off".into()),
    ])
}

#[test]
fn a_quoted_key_is_renamed_in_place_with_its_comment() {
    let read = format()
        .read_saved::<Map>(include_str!("../../../tests/fixtures/locations-1.toml"))
        .unwrap();
    assert_eq!(read.spec.locations, current());
    assert!(
        read.text.contains(
            "# Where this machine keeps the journal.\n\"lnk.moves\" = \"journal\"\n\"lnk.stopped-services\""
        ),
        "{}",
        read.text
    );
}

#[test]
fn a_quoted_key_is_renamed_in_json() {
    let read = format()
        .read_json::<Map>(include_bytes!("../../../tests/fixtures/locations-1.json"))
        .unwrap();
    assert_eq!(read.spec.locations, current());
}

#[test]
fn both_names_present_are_refused() {
    let text =
        "schema = 1\n[locations]\n\"lnk.migration-journal\" = \"a\"\n\"lnk.moves\" = \"b\"\n";
    let refused = format().read_saved::<Map>(text).unwrap_err().to_string();
    assert!(refused.contains("both"), "{refused}");
}

/// A key that can be bare is never quoted, and a broken quote is no path,
/// so each field has one spelling in a step.
#[test]
fn an_unneeded_or_broken_quote_is_refused() {
    for path in [
        r#"'locations."lnk"'"#,
        r#"'locations."lnk.migration-journal'"#,
    ] {
        let declared = format!(
            "schema = 1\nkind = \"k\"\ncurrent = 2\n[[steps]]\nfrom = 1\nsince = '0.0.1'\n\
             until = '0.0.2'\nremove = {{ {path} = \"gone\" }}\n"
        );
        assert!(Format::parse(&declared).is_err(), "{path}");
    }
}
