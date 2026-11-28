use super::*;

#[derive(Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    name: Option<String>,
    disk: Option<Disk>,
}
#[derive(Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Disk {
    size: u32,
}

fn format() -> Format {
    Format::parse(
        r#"
schema = 1
kind = "example"
current = 3
command = "lnk example spec --spec"
[[steps]]
from = 1
since = '0.0.1'
until = '0.0.2'
rename = { "machine.disk" = "disk.size" }
[[steps]]
from = 2
since = '0.0.1'
until = '0.0.2'
remove = { "unused" = "This field no longer has an effect." }
refuse = { "internet.any" = "Choose internet hosts explicitly." }
"#,
    )
    .unwrap()
}

#[test]
fn reads_current_and_all_published_schemas() {
    let format = format();
    for (text, old) in [
        ("schema = 1\n[machine]\ndisk = 20\n", true),
        ("schema = 2\nunused = true\n[disk]\nsize = 20\n", true),
        ("schema = 3\n[disk]\nsize = 20\n", false),
    ] {
        let read = format.read::<Spec>(text).unwrap();
        assert_eq!(read.spec.disk.unwrap().size, 20);
        assert_eq!(read.changed, old);
        assert!(read.text.contains("schema = 3"));
    }
}

#[test]
fn files_prove_the_published_chain_and_explicit_refusal() {
    let before = include_str!("../../tests/fixtures/schema-1.toml");
    let after = include_str!("../../tests/fixtures/schema-3.toml");
    let read = format().read::<Spec>(before).unwrap();
    assert_eq!(read.spec, format().read::<Spec>(after).unwrap().spec);
    assert!(read.text.contains("# Keep this explanation"));
    let refused = include_str!("../../tests/fixtures/schema-2-refused.toml");
    assert!(
        format()
            .read::<Spec>(refused)
            .unwrap_err()
            .to_string()
            .contains("Choose internet hosts explicitly")
    );
}

#[test]
fn inline_tables_are_upgraded_and_never_overwrite_existing_values() {
    let read = format()
        .read::<Spec>("schema = 1\nmachine = { disk = 20 }\n")
        .unwrap();
    assert_eq!(read.spec.disk.unwrap().size, 20);
    assert!(
        format()
            .read::<Spec>("schema = 1\nmachine = { disk = 20 }\ndisk = { size = 30 }\n")
            .is_err()
    );
}

#[test]
fn refuses_unsafe_ambiguous_future_and_malformed_documents() {
    for text in [
        "schema = 0",
        "schema = -1",
        "schema = 4",
        "schema = '1'",
        "schema = 1\n[machine]\ndisk = 20\n[disk]\nsize = 30",
        "schema = 2\n[internet]\nany = true",
        "schema = 3\ntypo = true",
    ] {
        assert!(format().read::<Spec>(text).is_err(), "{text}");
    }
    assert!(
        format()
            .read::<Spec>("schema = 4")
            .unwrap_err()
            .to_string()
            .contains("lnk upgrade")
    );
    assert!(
        format()
            .read::<Spec>("[machine]\ndisk = 20")
            .unwrap_err()
            .to_string()
            .contains("names no schema")
    );
}

#[test]
fn editing_preserves_comments_order_and_untouched_values() {
    let text = "# My spec\nschema = 1 # format\nname = 'web' # keep\n[machine]\n# disk comment\ndisk = 20 # capacity\n";
    let read = format().read::<Spec>(text).unwrap();
    assert!(read.text.contains("# My spec"));
    assert!(read.text.contains("# format"));
    assert!(read.text.contains("# disk comment"));
    assert!(read.text.contains("name = 'web' # keep"));
    assert!(read.text.contains("20 # capacity"));
    assert!(read.text.find("name").unwrap() < read.text.find("[disk]").unwrap());
    assert!(!read.text.contains("[machine]"));
    assert_eq!(format().read::<Spec>(&read.text).unwrap().spec, read.spec);
}

#[test]
fn moving_the_last_key_keeps_its_table_header_comments() {
    for text in [
        "schema = 1\n# Storage choices\n[machine] # machine settings\ndisk = 20\n",
        "schema = 1\n# Storage choices\nmachine = { disk = 20 } # machine settings\n",
    ] {
        let read = format().read::<Spec>(text).unwrap();
        for comment in ["# Storage choices", "# machine settings"] {
            assert!(read.text.contains(comment), "{comment}: {}", read.text);
        }
        assert_eq!(read.spec.disk.unwrap().size, 20);
    }
    let table_move = Format::parse(
        "schema = 1\nkind = 'example'\ncommand = 'lnk example spec --spec'\ncurrent = 2\n[[steps]]\nfrom = 1\nsince = '0.0.1'\nuntil = '0.0.2'\nrename = { 'machine.disk' = 'disk' }\n",
    )
    .unwrap();
    let text = "schema = 1\n# Storage choices\n[machine] # machine settings\n[machine.disk] # disk settings\nsize = 20\n";
    let read = table_move.read::<Spec>(text).unwrap();
    for comment in ["# Storage choices", "# machine settings", "# disk settings"] {
        assert!(read.text.contains(comment), "{comment}: {}", read.text);
    }
    assert_eq!(read.spec.disk.unwrap().size, 20);
}

#[test]
fn declarations_refuse_gaps_duplicates_and_unexplained_removals() {
    for steps in [
        "",
        "[[steps]]\nfrom = 1\nsince = '0.0.1'\nuntil = '0.0.2'",
        "[[steps]]\nfrom = 1\nsince = '0.0.1'\nuntil = '0.0.2'\nremove = { x = '' }\n[[steps]]\nfrom = 2\nsince = '0.0.1'\nuntil = '0.0.2'",
    ] {
        assert!(
            Format::parse(&format!(
                "schema = 1\nkind = 'example'\ncommand = 'lnk example spec --spec'\ncurrent = 3\n{steps}"
            ))
            .is_err()
        );
    }
}

#[test]
fn a_step_that_rewrites_a_value_by_code_is_refused() {
    let text = "schema = 1\nkind = 'named'\ncurrent = 2\ncommand = 'lnk named spec --spec'\n[[steps]]\nfrom = 1\nsince = '0.0.1'\nuntil = '0.0.2'\nrewrite = { model = 'prefixed' }\n";
    let refused = Format::parse(text).unwrap_err().to_string();
    assert!(refused.contains("rewrite"), "{refused}");
}

#[test]
fn a_step_names_the_releases_that_bring_and_delete_it() {
    let step = |since: &str, until: &str| {
        Format::parse(&format!(
            "schema = 1\nkind = 'k'\ncurrent = 2\ncommand = 'lnk k spec show --spec'\n[[steps]]\nfrom = 1\nsince = '{since}'\nuntil = '{until}'\n"
        ))
    };
    assert!(step("0.0.1", "0.0.2").is_ok());
    assert!(step("0.0.2", "0.0.2").is_err());
    assert!(
        Format::parse(
            "schema = 1\nkind = 'k'\ncurrent = 2\ncommand = 'lnk k spec show --spec'\n[[steps]]\nfrom = 1\n"
        )
        .is_err()
    );
}

fn kinded() -> Format {
    format().named()
}

/// A spec's error names the line of the person's file: its schema and
/// kind lines count, though the spec's type never reads them.
#[test]
fn an_error_names_the_line_of_the_file() {
    let error = |text: &str| format!("{:#}", kinded().read::<Spec>(text).unwrap_err());
    let typo = error("schema = 3\nkind = 'example'\n# a comment\n[disk]\nsize = 'big'\n");
    assert!(typo.contains("line 5"), "{typo}");
    let unknown = error("name = 'a'\nschema = 3 # format\ntypo = true\n");
    assert!(unknown.contains("line 3"), "{unknown}");
}

#[test]
fn a_named_kinds_files_say_their_kind_and_refuse_another() {
    let read = kinded()
        .read::<Spec>("kind = \"example\"\nschema = 3\n[disk]\nsize = 20\n")
        .unwrap();
    assert_eq!(read.spec.disk.unwrap().size, 20);
    // Left out, the kind is this one.
    assert!(
        kinded()
            .read::<Spec>("schema = 3\n[disk]\nsize = 20\n")
            .is_ok()
    );
    // A spec names its schema, and is refused without one, as JSON too.
    let unnamed = kinded().read::<Spec>("[disk]\nsize = 20\n").unwrap_err();
    assert!(unnamed.to_string().contains("add schema = 3"), "{unnamed}");
    assert!(
        kinded()
            .read_json::<Spec>(br#"{"kind":"example"}"#)
            .is_err()
    );
    // A saved file naming none is refused too, as none Link wrote.
    let unnamed = format().read::<Spec>("[disk]\nsize = 20\n").unwrap_err();
    assert!(unnamed.to_string().contains("names no schema"), "{unnamed}");
    assert!(
        format()
            .read_json::<Spec>(br#"{"disk":{"size":20}}"#)
            .is_err()
    );
    let other = kinded()
        .read::<Spec>("kind = \"box\"\nschema = 3\n[disk]\nsize = 20\n")
        .unwrap_err();
    assert!(
        other
            .to_string()
            .contains("a \"box\" spec, not a example spec"),
        "{other}"
    );
    let printed = kinded()
        .print(&Spec {
            name: Some("a".into()),
            disk: None,
        })
        .unwrap();
    assert!(
        printed.starts_with("kind = \"example\"\nschema = 3\n"),
        "{printed}"
    );
    assert!(kinded().read::<Spec>(&printed).is_ok());
    let json = kinded()
        .print_json(&Spec {
            name: None,
            disk: None,
        })
        .unwrap();
    assert_eq!(
        json,
        br#"{"name":null,"disk":null,"kind":"example","schema":3}"#
    );
    assert!(kinded().read_json::<Spec>(&json).is_ok());
    assert!(
        kinded()
            .read_json::<Spec>(br#"{"kind":"box","schema":3}"#)
            .is_err()
    );
    // A saved file's format has no kind: its key is any other.
    assert!(format().read::<Spec>("kind = \"box\"\n").is_err());
}

#[test]
fn a_step_deleted_at_its_until_leaves_its_schema_refused_as_unknown() {
    let format = Format::parse(
        "schema = 1\nkind = 'k'\ncurrent = 3\noldest = 2\ncommand = 'lnk k spec show --spec'\n[[steps]]\nfrom = 2\nsince = '0.0.1'\nuntil = '0.0.2'\nrename = { title = 'name' }\n",
    )
    .unwrap();
    assert!(format.read::<Spec>("schema = 2\ntitle = 'a'\n").is_ok());
    let old = format.read::<Spec>("schema = 1\n").unwrap_err();
    assert!(old.to_string().contains("no longer read"), "{old}");
    // Every schema from the oldest is still stepped from.
    assert!(
        Format::parse("schema = 1\nkind = 'k'\ncurrent = 3\noldest = 1\ncommand = 'lnk k spec show --spec'\n[[steps]]\nfrom = 2\nsince = '0.0.1'\nuntil = '0.0.2'\n")
            .is_err()
    );
}

#[test]
fn a_key_renamed_in_its_table_keeps_its_place() {
    let format = Format::parse(
        "schema = 1\nkind = 'k'\ncurrent = 2\ncommand = 'lnk k spec show --spec'\n[[steps]]\nfrom = 1\nsince = '0.0.1'\nuntil = '0.0.2'\nrename = { title = 'name' }\n",
    )
    .unwrap();
    let (doc, _) = format
        .document("schema = 1\ntitle = 'a'\nother = 1\n")
        .unwrap();
    let text = doc.to_string();
    assert!(
        text.find("name").unwrap() < text.find("other").unwrap(),
        "{text}"
    );
}

mod coded;
mod coverage;
mod quoted;

#[test]
fn a_declaration_names_its_own_format_first() {
    let body = "kind = 'k'\ncurrent = 1\ncommand = 'lnk k spec show'\n";
    assert!(Format::parse(&format!("schema = 1\n{body}")).is_ok());
    assert!(Format::parse(body).is_err());
    let newer = Format::parse(&format!("schema = 2\n{body}"))
        .unwrap_err()
        .to_string();
    assert!(newer.contains("format 2"), "{newer}");
    let step =
        "current = 2\ncommand = 'lnk k spec show'\nfrom = 1\nsince = '0.0.1'\nuntil = '0.0.2'\n";
    let step = |table: &str| {
        let (head, tail) = step.split_at(step.find("from").unwrap());
        format!("schema = 1\nkind = 'k'\n{head}[[{table}]]\n{tail}")
    };
    assert!(Format::parse(&step("steps")).is_ok());
    assert!(Format::parse(&step("step")).is_err());
}
