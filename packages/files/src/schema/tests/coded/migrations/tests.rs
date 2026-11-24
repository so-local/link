//! The steps hold the code they name to its kind, its schemas and its
//! owner's registration.

use crate::schema::{Format, Migration};

const DISKS: &[Migration] = &[Migration {
    name: "disks_1_to_2",
    run: Ok,
}];

const TWICE: &[Migration] = &[DISKS[0], DISKS[0]];

fn declared(step: &str) -> String {
    format!(
        "schema = 1\nkind = 'disks'\ncurrent = 2\ncommand = 'lnk disks spec show'\n[[steps]]\nfrom = 1\n\
         since = '0.1.0'\nuntil = '0.2.0'\n{step}\n"
    )
}

#[test]
fn a_step_names_registered_code_for_its_kind_and_schemas() {
    let named = declared("migration = 'disks_1_to_2'");
    assert!(Format::parse_with(&named, DISKS).is_ok());
    for (text, code, says) in [
        (named.clone(), &[][..], "doesn't register"),
        (
            declared("migration = 'disks_2_to_3'"),
            DISKS,
            "name it \"disks_1_to_2\"",
        ),
        (declared("migration = 'disk_1_to_2'"), DISKS, "name it"),
        (
            declared("migration = 'disks_1_to_2'\nrename = { a = 'b' }"),
            DISKS,
            "give the edits their own step",
        ),
        (
            declared("rename = { a = 'b' }"),
            DISKS,
            "which no schema step names",
        ),
        (named.clone(), TWICE, "doesn't register once"),
    ] {
        let refused = Format::parse_with(&text, code).unwrap_err().to_string();
        assert!(refused.contains(says), "{text}: {refused}");
    }
}

#[test]
fn code_that_writes_the_envelope_is_refused() {
    const ENVELOPE: &[Migration] = &[Migration {
        name: "disks_1_to_2",
        run: |mut body| {
            body.insert("schema".into(), 9.into());
            Ok(body)
        },
    }];
    let format = Format::parse_with(&declared("migration = 'disks_1_to_2'"), ENVELOPE).unwrap();
    let refused = format
        .read::<toml::Table>("schema = 1\nname = 'web'\n")
        .unwrap_err()
        .to_string();
    assert!(refused.contains("which the steps own"), "{refused}");
}

#[test]
fn a_code_step_answers_the_field_flags_it_retires() {
    let named = declared(
        "migration = 'disks_1_to_2'\nflags = { disk = 'say its size and unit apart: --disk.size 20 --disk.unit G' }",
    );
    let format = Format::parse_with(&named, DISKS).unwrap();
    let said = "say its size and unit apart: --disk.size 20 --disk.unit G";
    assert_eq!(
        format.fate(1, "disk"),
        crate::schema::Fate::Refused(said.into())
    );
    assert!(format.retired().iter().any(|r| r.path == "disk"
        && r.since == 2
        && r.fate == crate::schema::Fate::Refused(said.into())));
    // A file is read by the code alone: the flags never edit it.
    let read = format
        .read::<toml::Table>("schema = 1\ndisk = 'kept'\n")
        .unwrap();
    assert_eq!(read.spec["disk"].as_str(), Some("kept"));
    for (step, says) in [
        (
            "rename = { a = 'b' }\nflags = { a = 'why' }",
            "without running code",
        ),
        (
            "migration = 'disks_1_to_2'\nflags = { a = ' ' }",
            "needs its explanation",
        ),
        (
            "migration = 'disks_1_to_2'\nflags = { 'a..b' = 'why' }",
            "invalid schema step path",
        ),
    ] {
        let refused = Format::parse_with(&declared(step), DISKS)
            .unwrap_err()
            .to_string();
        assert!(refused.contains(says), "{step}: {refused}");
    }
}

/// A TOML datetime the code doesn't touch is still one after the step,
/// though the code reads the file through JSON, which has none.
#[test]
fn a_code_step_keeps_a_datetime_it_leaves_alone() {
    let format = Format::parse_with(&declared("migration = 'disks_1_to_2'"), DISKS).unwrap();
    let read = format
        .read::<toml::Table>(
            "schema = 1\nname = 'web'\nmade = 1979-05-27T07:32:00Z\n[at]\nwhen = 2026-01-02\n",
        )
        .unwrap();
    let after: toml::Table = toml::from_str(&read.text).unwrap();
    assert!(after["made"].is_datetime(), "{}", read.text);
    assert!(after["at"]["when"].is_datetime(), "{}", read.text);
    assert!(
        read.text.contains("made = 1979-05-27T07:32:00Z"),
        "{}",
        read.text
    );
}

/// A field the steps take away is answered by a step, and a flag a code
/// step answers is a field of the schema it steps from.
#[test]
fn every_field_a_step_takes_is_answered() {
    use crate::schema::{field_paths, tests::coverage::section_schema};
    let text = |flags: &str| {
        declared(&format!(
            "migration = 'disks_1_to_2'\n[steps.flags]\n{flags}"
        ))
    };
    let from = field_paths(&section_schema(true));
    let current = field_paths(&section_schema(false));
    let answered = Format::parse_with(
        &text("'internet.tcp' = 'say it'\n'internet.linux.tcp' = 'say it'"),
        DISKS,
    )
    .unwrap();
    assert!(answered.unanswered(&from, &current).is_empty());
    let unanswered = Format::parse_with(&text("'internet.tcp' = 'say it'"), DISKS).unwrap();
    assert_eq!(
        unanswered.unanswered(&from, &current),
        [
            "internet.linux.tcp of schema 1 is no field of schema 2 (internet.linux.tcp), and no step answers it"
        ]
    );
    let stray = Format::parse_with(
        &text(
            "'internet.tcp' = 'say it'\n'internet.linux.tcp' = 'say it'\n'internet.udp' = 'say it'",
        ),
        DISKS,
    )
    .unwrap();
    assert_eq!(
        stray.unanswered(&from, &current),
        ["internet.udp, a flag of step 1, is no field of schema 1"]
    );
}
