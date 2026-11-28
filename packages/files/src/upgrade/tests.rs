//! An upgrade's changes on two owners' files: Link's settings, whose
//! `secret.keychain` moves to a policy in schema 2, kept where secret
//! settings are, and the policy, whose `title` is `label` in its schema 2 and which takes the
//! setting. A note, another file of state, and a coded kind, which takes
//! the settings' `name` by its own code, join some. Each runs in its own
//! installation folder, asked as `lnk upgrade` asks.

use std::os::unix::fs::PermissionsExt;

use serde::{Deserialize, Serialize};

use super::*;
use crate::layout::Hidden;

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Setting {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secret: Option<Keeping>,
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Keeping {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<String>,
}

crate::specification!(Setting, include_str!("tests/setting.schema.toml"));

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    keychain: Option<bool>,
}

crate::specification!(Policy, include_str!("tests/policy.schema.toml"));

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Note {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}

crate::specification!(Note, include_str!("tests/note.schema.toml"));

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Coded {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    machines: Vec<String>,
}

crate::specification!(Coded, include_str!("tests/coded.schema.toml"), migrations);

mod migrations;

/// Settings of schema 1, as 0.0.1 wrote them, with your comments.
const SETTINGS: &str = "# My settings.\nschema = 1\nname = \"desk\"\n\n[secret]\n# Off for now.\nkeychain = true\ncolor = \"blue\"\n";

/// A policy of schema 1, its comment yours.
const POLICY: &str = "schema = 1\n# What I call it.\ntitle = \"mine\"\n";

/// A machine of its own with 0.0.1's settings and policy: its roots, and
/// `None` where this machine can't hold a trusted control file.
fn machine(name: &str) -> Option<(link_test_kit::TempDir, Roots)> {
    let dir =
        link_test_kit::TempDir::under(&Path::new(env!("CARGO_MANIFEST_DIR")).join("target"), name);
    let roots = Roots {
        user_home: dir.to_path_buf(),
        hidden: dir.join(".lnk"),
    };
    let settings = Hidden::Settings.at_default(&roots.hidden);
    if !link_test_kit::gate::settings(&settings) {
        return None;
    }
    crate::write_private(&settings, SETTINGS.as_bytes()).unwrap();
    crate::write_private(
        &Hidden::SecretSettings.at_default(&roots.hidden),
        POLICY.as_bytes(),
    )
    .unwrap();
    Some((dir, roots))
}

fn owners() -> [Saved; 2] {
    [Saved::of::<Policy>(), Saved::of::<Setting>()]
}

fn read(roots: &Roots, at: Hidden) -> String {
    std::fs::read_to_string(at.at_default(&roots.hidden)).unwrap()
}

fn journal(roots: &Roots) -> PathBuf {
    Hidden::Upgrades
        .at_default(&roots.hidden)
        .join("transfers.toml")
}

/// What `owners` will change, asked as `lnk upgrade` asks: first for the
/// transfers, then for every change on what they leave.
fn plan_of(owners: &[Saved], roots: &Roots) -> anyhow::Result<Vec<Change>> {
    let transfers: Vec<Change> = run(Stage::Plan, owners, roots, &[])?
        .into_iter()
        .filter(|change| matches!(change, Change::Transfer { .. }))
        .collect();
    run(Stage::Plan, owners, roots, &transfers)
}

/// The plan, agreed, then transfers and conversions of it.
fn upgrade_of(owners: &[Saved], roots: &Roots) -> anyhow::Result<Vec<Change>> {
    let agreed = plan_of(owners, roots)?;
    let mut changes = run(Stage::Transfer, owners, roots, &agreed)?;
    changes.extend(run(Stage::Convert, owners, roots, &agreed)?);
    Ok(changes)
}

fn plan(roots: &Roots) -> Vec<Change> {
    plan_of(&owners(), roots).unwrap()
}

fn upgrade(roots: &Roots) -> Vec<Change> {
    upgrade_of(&owners(), roots).unwrap()
}

#[test]
fn the_plan_names_each_change_and_changes_nothing() {
    let Some((_dir, roots)) = machine("upgrade-plan") else {
        return;
    };
    let planned = plan(&roots);
    let settings = Hidden::Settings.at_default(&roots.hidden);
    let policy = Hidden::SecretSettings.at_default(&roots.hidden);
    assert_eq!(
        planned,
        [
            Change::Transfer {
                source: settings.clone(),
                from: "upgrade setting".into(),
                into: policy.clone(),
                kind: "upgrade policy".into(),
                paths: vec!["secret.keychain".into()],
            },
            Change::Convert {
                file: policy,
                kind: "upgrade policy".into(),
                from: 1,
                to: 2
            },
            Change::Convert {
                file: settings,
                kind: "upgrade setting".into(),
                from: 1,
                to: 2
            },
        ]
    );
    assert_eq!(read(&roots, Hidden::Settings), SETTINGS);
    assert_eq!(read(&roots, Hidden::SecretSettings), POLICY);
    assert!(!journal(&roots).exists());
    // As the answer a program prints, read back by the protocol's version.
    let printed = serde_json::to_vec(&Answer::stating(planned.clone(), None, &PROTOCOL)).unwrap();
    assert_eq!(
        Answer::parse("lnk-test", &printed).unwrap().changes,
        planned
    );
    let newer = format!("{{\"protocol\":{},\"changes\":[]}}", PROTOCOL.current + 1);
    let refused = Answer::parse("lnk-test", newer.as_bytes()).unwrap_err();
    assert!(refused.to_string().contains("lnk upgrade"), "{refused}");
}

#[test]
fn an_upgrade_moves_the_record_and_converts_both_files_keeping_comments_and_modes() {
    let Some((_dir, roots)) = machine("upgrade-converts") else {
        return;
    };
    let policy = Hidden::SecretSettings.at_default(&roots.hidden);
    std::fs::set_permissions(&policy, std::fs::Permissions::from_mode(0o640)).unwrap();
    // The transfer writes the policy in its current schema, converting it.
    let changes = upgrade(&roots);
    assert_eq!(changes.len(), 2, "{changes:?}");
    assert_eq!(
        read(&roots, Hidden::SecretSettings),
        // The record's comment goes with it.
        "schema = 2\n# What I call it.\nlabel = \"mine\"\n# Off for now.\nkeychain = true\n"
    );
    assert_eq!(
        std::fs::metadata(&policy).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(
        read(&roots, Hidden::Settings),
        "# My settings.\nschema = 2\nname = \"desk\"\n\n[secret]\ncolor = \"blue\"\n"
    );
    assert_eq!(
        crate::saved::read::<Policy>(&policy).unwrap(),
        Policy {
            label: Some("mine".into()),
            keychain: Some(true)
        }
    );
    assert!(!journal(&roots).exists());
    // Run again, it has nothing left to do.
    assert_eq!(upgrade(&roots), []);
    assert_eq!(plan(&roots), []);
}

#[test]
fn an_interrupted_transfer_finishes_on_a_rerun_and_takes_nothing_twice() {
    for (name, stop) in [
        ("upgrade-stopped-recorded", journal::Stop::Recorded),
        ("upgrade-stopped-written", journal::Stop::Destination),
    ] {
        let Some((_dir, roots)) = machine(name) else {
            return;
        };
        interrupt(&roots, stop);
        assert!(journal(&roots).exists(), "{name}");
        // The source keeps its record until its destination has it.
        assert_eq!(read(&roots, Hidden::Settings), SETTINGS, "{name}");
        upgrade(&roots);
        let moved = read(&roots, Hidden::SecretSettings);
        assert_eq!(moved.matches("keychain").count(), 1, "{name}: {moved}");
        assert!(
            !read(&roots, Hidden::Settings).contains("keychain"),
            "{name}"
        );
        assert!(!journal(&roots).exists(), "{name}");
    }
}

/// A transfer stopped once its source was written leaves nothing to plan
/// once the rest is converted, and the rerun's stages, given no plan,
/// still finish its entry.
#[test]
fn a_rerun_with_nothing_planned_finishes_a_transfer_left_in_the_journal() {
    let Some((_dir, roots)) = machine("upgrade-stopped-source") else {
        return;
    };
    let agreed = plan(&roots);
    interrupt(&roots, journal::Stop::Source);
    run(Stage::Convert, &owners(), &roots, &agreed).unwrap();
    assert_eq!(plan(&roots), []);
    assert!(under_way(&roots).unwrap());
    run(Stage::Transfer, &owners(), &roots, &[]).unwrap();
    run(Stage::Convert, &owners(), &roots, &[]).unwrap();
    assert!(!under_way(&roots).unwrap());
    assert!(!journal(&roots).exists());
}

/// The transfer of the policy's settings, agreed as planned, stopped
/// at `stop`.
fn interrupt(roots: &Roots, stop: journal::Stop) {
    let [policy, _] = owners();
    let agreed = plan(roots);
    let transfer = &policy.format.transfers[0];
    journal::run_until(&policy, transfer, &[], roots, &agreed, stop).unwrap();
}

#[test]
fn a_destination_changed_mid_transfer_is_taken_again_and_a_refused_one_recovers_on_a_rerun() {
    let Some((_dir, roots)) = machine("upgrade-changed") else {
        return;
    };
    interrupt(&roots, journal::Stop::Recorded);
    // Changed to hold the record otherwise: refused before anything
    // changes, before the yes.
    let into = Hidden::SecretSettings.at_default(&roots.hidden);
    let colliding = "schema = 1\nkeychain = false\n";
    crate::write_private(&into, colliding.as_bytes()).unwrap();
    let refused = upgrade_of(&owners(), &roots).unwrap_err();
    assert!(
        format!("{refused:#}").contains("holds keychain"),
        "{refused:#}"
    );
    assert_eq!(read(&roots, Hidden::SecretSettings), colliding);
    assert_eq!(read(&roots, Hidden::Settings), SETTINGS);
    // Kept another way, the rerun takes the records again, from the
    // source the stopped run never touched.
    let changed = "schema = 1\ntitle = \"someone else's\"\n";
    crate::write_private(&into, changed.as_bytes()).unwrap();
    upgrade(&roots);
    assert_eq!(
        crate::saved::read::<Policy>(&into).unwrap(),
        Policy {
            label: Some("someone else's".into()),
            keychain: Some(true)
        }
    );
    assert!(!read(&roots, Hidden::Settings).contains("keychain"));
    assert!(!journal(&roots).exists());
}

#[test]
fn a_record_left_in_a_changed_source_goes_only_where_it_was_moved() {
    let Some((_dir, roots)) = machine("upgrade-source-changed") else {
        return;
    };
    interrupt(&roots, journal::Stop::Destination);
    let settings = Hidden::Settings.at_default(&roots.hidden);
    // Another value written since: refused, by the plan and by the
    // journal alike, the entry kept.
    let other = SETTINGS.replace("keychain = true", "keychain = false");
    crate::write_private(&settings, other.as_bytes()).unwrap();
    let refused = upgrade_of(&owners(), &roots).unwrap_err();
    assert!(
        format!("{refused:#}").contains("holds keychain"),
        "{refused:#}"
    );
    let [policy, _] = owners();
    let transfer = &policy.format.transfers[0];
    let refused = journal::run(&policy, transfer, &[], &roots, &[]).unwrap_err();
    assert!(
        format!("{refused:#}").contains("another one"),
        "{refused:#}"
    );
    assert!(journal(&roots).exists());
    // The same value, with another change beside it: taken out.
    let same = SETTINGS.replace("name = \"desk\"", "name = \"den\"");
    crate::write_private(&settings, same.as_bytes()).unwrap();
    upgrade(&roots);
    let left = read(&roots, Hidden::Settings);
    assert!(!left.contains("keychain") && left.contains("den"), "{left}");
    assert!(!journal(&roots).exists());
}

#[test]
fn only_the_agreed_plan_is_made() {
    let Some((_dir, roots)) = machine("upgrade-agreed") else {
        return;
    };
    // Agreed to nothing, as though both files were current then.
    let refused = run(Stage::Transfer, &owners(), &roots, &[]).unwrap_err();
    assert!(format!("{refused:#}").contains("agreed"), "{refused:#}");
    let refused = run(Stage::Convert, &owners(), &roots, &[]).unwrap_err();
    assert!(format!("{refused:#}").contains("agreed"), "{refused:#}");
    assert_eq!(read(&roots, Hidden::Settings), SETTINGS);
    assert_eq!(read(&roots, Hidden::SecretSettings), POLICY);
    assert!(!journal(&roots).exists());
}

#[test]
fn a_missing_destination_keeps_its_records_and_names_the_file_never_blocking() {
    let Some((_dir, roots)) = machine("upgrade-no-destination") else {
        return;
    };
    let note = Hidden::Envs.at_default(&roots.hidden).join("note.toml");
    crate::write_private(&note, b"schema = 1\nsaid = \"hi\"\n").unwrap();
    // The policy's program isn't installed: nothing takes its record.
    let owners = [Saved::of::<Setting>(), Saved::of::<Note>()];
    let settings = Hidden::Settings.at_default(&roots.hidden);
    let kept = |changes: &[Change]| {
        changes.iter().any(|change| {
            matches!(change, Change::Kept { file, why, .. }
                if *file == settings && why.contains("secret.keychain"))
        })
    };
    let planned = plan_of(&owners, &roots).unwrap();
    assert!(kept(&planned), "{planned:?}");
    let changes = upgrade_of(&owners, &roots).unwrap();
    assert!(kept(&changes), "{changes:?}");
    // The settings as they were; the note after them converted.
    assert_eq!(read(&roots, Hidden::Settings), SETTINGS);
    assert_eq!(
        crate::saved::read::<Note>(&note).unwrap(),
        Note {
            text: Some("hi".into())
        }
    );
}

#[test]
fn the_plan_tries_each_conversion_and_names_one_that_fails_before_the_yes() {
    let Some((_dir, roots)) = machine("upgrade-tried") else {
        return;
    };
    // Its steps reach schema 2, but its name isn't a name.
    let wrong = SETTINGS.replace("name = \"desk\"", "name = 5");
    let settings = Hidden::Settings.at_default(&roots.hidden);
    crate::write_private(&settings, wrong.as_bytes()).unwrap();
    let planned = plan_of(&owners(), &roots).unwrap();
    assert!(
        planned
            .iter()
            .any(|change| matches!(change, Change::Unread { file, .. } if *file == settings)),
        "{planned:?}"
    );
    assert_eq!(read(&roots, Hidden::Settings), wrong);
    assert_eq!(read(&roots, Hidden::SecretSettings), POLICY);
}

/// A torn file, one in no schema's shape and one a newer release wrote
/// are each kept as they are and named, and the files beside them are
/// converted: one file never stops `lnk upgrade`.
#[test]
fn one_unreadable_file_is_kept_and_named_and_the_rest_converted() {
    let Some((_dir, roots)) = machine("upgrade-unread") else {
        return;
    };
    let envs = Hidden::Envs.at_default(&roots.hidden);
    let note = envs.join("note.toml");
    crate::write_private(&note, b"schema = 1\nsaid = \"hi\"\n").unwrap();
    let owners = [Saved::of::<Note>()];
    let unread = |changes: &[Change], torn: &Path, words: &str| {
        changes.iter().any(|change| {
            matches!(change, Change::Unread { file, why, .. } if file == torn && why.contains(words))
        })
    };
    for (text, words) in [
        ("schema = 1\nsaid = \"hi", "isn't as it should be"),
        ("schema = 9\nsaid = \"hi\"\n", "needs a newer lnk"),
    ] {
        crate::write_private(&note, text.as_bytes()).unwrap();
        let planned = plan_of(&owners, &roots).unwrap();
        assert!(unread(&planned, &note, words), "{planned:?}");
        let changes = upgrade_of(&owners, &roots).unwrap();
        assert!(unread(&changes, &note, words), "{changes:?}");
        assert_eq!(std::fs::read_to_string(&note).unwrap(), text);
    }
}

#[test]
fn a_transfer_runs_its_owners_code_on_what_it_takes() {
    let Some((_dir, roots)) = machine("upgrade-coded") else {
        return;
    };
    let owners = [
        Saved::of::<Coded>(),
        Saved::of::<Policy>(),
        Saved::of::<Setting>(),
    ];
    upgrade_of(&owners, &roots).unwrap();
    let coded = Hidden::Measures
        .at_default(&roots.hidden)
        .join("coded.toml");
    assert_eq!(
        crate::saved::read::<Coded>(&coded).unwrap(),
        Coded {
            machines: vec!["desk".into()]
        }
    );
    let left = read(&roots, Hidden::Settings);
    assert!(
        !left.contains("desk") && !left.contains("keychain"),
        "{left}"
    );
    assert_eq!(upgrade_of(&owners, &roots).unwrap(), []);
}

#[test]
fn an_older_core_reads_a_newer_programs_answer_in_its_own_version() {
    let newer = link_version::Protocol::parse(&format!(
        "name = \"upgrade\"\ncurrent = {}\noldest = {}\nupgrade = \"lnk upgrade\"\n",
        PROTOCOL.current + 1,
        PROTOCOL.oldest
    ))
    .unwrap();
    let asked = newer.accept("the lnk upgrading", PROTOCOL.current).ok();
    let answered = Answer::stating(Vec::new(), asked, &newer);
    let printed = serde_json::to_vec(&answered).unwrap();
    let read = Answer::parse("lnk-test", &printed).unwrap();
    assert_eq!((read.protocol, read.changes), (PROTOCOL.current, vec![]));
    // A person asking by hand reads the newest.
    assert_eq!(
        Answer::stating(Vec::new(), None, &newer).protocol,
        newer.current
    );
}

#[test]
fn a_stage_that_writes_is_refused_to_anyone_but_lnk_upgrade() {
    let Some((_dir, roots)) = machine("upgrade-asker") else {
        return;
    };
    for stage in [Stage::Transfer, Stage::Convert] {
        let refused = respond(stage, &owners(), &roots, None, b"").unwrap_err();
        assert!(refused.to_string().contains("lnk upgrade"), "{refused}");
    }
    let older = respond(Stage::Plan, &owners(), &roots, Some(0), b"").unwrap_err();
    assert!(older.to_string().contains("lnk upgrade"), "{older}");
    // By hand, the plan is answered, as though no transfer ran.
    let answer = respond(Stage::Plan, &owners(), &roots, None, b"").unwrap();
    assert_eq!(answer.protocol, PROTOCOL.current);
    assert_eq!(read(&roots, Hidden::Settings), SETTINGS);
    // The first plan a core asks gives nothing, and a stage that writes
    // is never asked so.
    let stated = Some(PROTOCOL.current);
    let first = respond(Stage::Plan, &owners(), &roots, stated, b"").unwrap();
    assert_eq!(first.protocol, PROTOCOL.current);
    let given = respond(Stage::Transfer, &owners(), &roots, stated, b"").unwrap_err();
    assert!(given.to_string().contains("lnk upgrade"), "{given}");
    assert_eq!(read(&roots, Hidden::Settings), SETTINGS);
}

/// A newer core asks a rolled-back program, stating its newer version:
/// the program answers in its own, and a request written in a version it
/// doesn't read is refused by that version, never parsed into its own
/// shape. The core writes each later request in the version answered.
#[test]
fn a_request_in_a_newer_version_is_refused_before_its_shape_is_read() {
    let Some((_dir, roots)) = machine("upgrade-newer-request") else {
        return;
    };
    let newer = PROTOCOL.current + 1;
    let first = respond(Stage::Plan, &owners(), &roots, Some(newer), b"").unwrap();
    assert_eq!(first.protocol, PROTOCOL.current);
    // A request that would parse in this release's shape, stated newer.
    let request = format!("{{\"protocol\":{newer},\"changes\":[]}}");
    for stage in Stage::ALL {
        let refused =
            respond(stage, &owners(), &roots, Some(newer), request.as_bytes()).unwrap_err();
        assert!(
            refused.to_string().contains("newer than"),
            "{}: {refused}",
            stage.word()
        );
    }
    assert_eq!(read(&roots, Hidden::Settings), SETTINGS);
    // In the version it answered, the request is read.
    let agreed = serde_json::to_vec(&Request::stating(plan(&roots), first.protocol)).unwrap();
    let made = respond(Stage::Transfer, &owners(), &roots, Some(newer), &agreed).unwrap();
    assert_eq!(made.changes.len(), 1, "{:?}", made.changes);
}

#[test]
fn an_unreadable_folder_fails_the_upgrade() {
    let Some((_dir, roots)) = machine("upgrade-unreadable") else {
        return;
    };
    // Root reads any folder: nothing to prove there.
    if link_test_kit::users::root()
        && !link_test_kit::gate::lacks("an ordinary user", "root reads any folder", None)
    {
        return;
    }
    let grants = Hidden::StoppedServices.at_default(&roots.hidden);
    crate::write_private(&grants.join("mini.toml"), b"schema = 1\n").unwrap();
    let before = std::fs::metadata(&grants).unwrap().permissions();
    std::fs::set_permissions(&grants, std::fs::Permissions::from_mode(0o000)).unwrap();
    let place = Place {
        location: "lnk.stopped-services".into(),
        path: Some("*.toml".into()),
        encoding: Encoding::Toml,
    };
    let refused = found(&place, &roots);
    std::fs::set_permissions(&grants, before).unwrap();
    let refused = refused.unwrap_err();
    assert!(format!("{refused:#}").contains("reading"), "{refused:#}");
}

#[test]
fn a_transfer_takes_only_from_another_file_in_control() {
    let policy = include_str!("tests/policy.schema.toml");
    let own = policy.replace(
        "source = { location = \"lnk.settings\"",
        "source = { location = \"lnk.secret-settings\"",
    );
    let refused = Format::parse(&own).unwrap_err().to_string();
    assert!(refused.contains("its own file"), "{refused}");
    let data = policy.replace(
        "source = { location = \"lnk.settings\"",
        "source = { location = \"lnk.model-context\"",
    );
    let refused = Format::parse(&data).unwrap_err().to_string();
    assert!(refused.contains("outside control"), "{refused}");
}

#[test]
fn a_record_the_destination_holds_otherwise_is_refused_before_anything_changes() {
    let Some((_dir, roots)) = machine("upgrade-collides") else {
        return;
    };
    let held = "schema = 1\nkeychain = false\n";
    crate::write_private(
        &Hidden::SecretSettings.at_default(&roots.hidden),
        held.as_bytes(),
    )
    .unwrap();
    let refused = plan_of(&owners(), &roots).unwrap_err();
    assert!(
        format!("{refused:#}").contains("holds keychain already"),
        "{refused:#}"
    );
    assert_eq!(read(&roots, Hidden::SecretSettings), held);
}

/// A file naming no schema is no file Link wrote: refused, as the
/// source of a transfer and as a file to convert, and left as it is.
#[test]
fn a_file_naming_no_schema_is_refused_never_taken_for_the_current() {
    let Some((_dir, roots)) = machine("upgrade-unnamed") else {
        return;
    };
    let bare = SETTINGS.replace("schema = 1\n", "");
    let settings = Hidden::Settings.at_default(&roots.hidden);
    crate::write_private(&settings, bare.as_bytes()).unwrap();
    let refused = plan_of(&owners(), &roots).unwrap_err();
    assert!(
        format!("{refused:#}").contains("names no schema"),
        "{refused:#}"
    );
    assert_eq!(read(&roots, Hidden::Settings), bare);
    let policy = Hidden::SecretSettings.at_default(&roots.hidden);
    crate::write_private(&policy, b"title = \"mine\"\n").unwrap();
    let refused = plan_of(&[Saved::of::<Policy>()], &roots).unwrap_err();
    assert!(
        format!("{refused:#}").contains("names no schema"),
        "{refused:#}"
    );
}

#[test]
fn a_file_newer_than_this_release_is_refused_naming_the_upgrade() {
    let Some((_dir, roots)) = machine("upgrade-newer") else {
        return;
    };
    crate::write_private(
        &Hidden::SecretSettings.at_default(&roots.hidden),
        b"schema = 3\nlabel = \"later\"\n",
    )
    .unwrap();
    let refused = plan_of(&[Saved::of::<Policy>()], &roots).unwrap_err();
    assert!(
        format!("{refused:#}").contains("lnk upgrade"),
        "{refused:#}"
    );
}

#[test]
fn only_history_keeps_its_steps_forever() {
    let ledger = Format::parse(include_str!("tests/ledger.schema.toml")).unwrap();
    assert_eq!(ledger.holds, Holds::History);
    assert_eq!(ledger.steps[0].until, None);
    let line: serde_json::Value = serde_json::from_slice(
        &ledger
            .read_json::<serde_json::Value>(br#"{"said":"hello","schema":1}"#)
            .unwrap()
            .text
            .into_bytes(),
    )
    .unwrap();
    assert_eq!(line["text"], "hello");
    let state = include_str!("tests/ledger.schema.toml").replace("holds = \"history\"\n", "");
    let refused = Format::parse(&state).unwrap_err().to_string();
    assert!(refused.contains("names no until"), "{refused}");
    let listed = format!(
        "{}\n[[files]]\nlocation = \"lnk.settings\"\nencoding = \"lines\"\n",
        include_str!("tests/ledger.schema.toml")
    );
    let refused = Format::parse(&listed).unwrap_err().to_string();
    assert!(refused.contains("never rewrites"), "{refused}");
}

/// State inside something else lists no files and takes no records, and
/// its steps are retired as state's are.
#[test]
fn state_inside_something_else_lists_no_files_and_retires_its_steps() {
    let ledger = include_str!("tests/ledger.schema.toml");
    let inside = ledger.replace("holds = \"history\"", "holds = \"inside\"");
    let refused = Format::parse(&inside).unwrap_err().to_string();
    assert!(refused.contains("names no until"), "{refused}");
    let retired = inside.replace("since = \"0.0.2\"", "since = \"0.0.2\"\nuntil = \"0.0.3\"");
    let format = Format::parse(&retired).unwrap();
    assert_eq!(format.holds, Holds::Inside);
    let line = format
        .read_json::<serde_json::Value>(br#"{"said":"hello","schema":1}"#)
        .unwrap();
    assert_eq!(line.spec["text"], "hello");
    let listed =
        format!("{retired}\n[[files]]\nlocation = \"lnk.settings\"\nencoding = \"toml\"\n");
    let refused = Format::parse(&listed).unwrap_err().to_string();
    assert!(refused.contains("can't find"), "{refused}");
}

#[test]
fn a_star_stands_for_part_of_one_name_and_is_put_back_where_it_goes() {
    let Some((_dir, roots)) = machine("upgrade-stars") else {
        return;
    };
    let grants = Hidden::StoppedServices.at_default(&roots.hidden);
    for name in ["mini.toml", "claude.toml", "notes.txt", ".hidden.toml"] {
        crate::write_private(&grants.join(name), b"schema = 1\n").unwrap();
    }
    let place = Place {
        location: "lnk.stopped-services".into(),
        path: Some("*.toml".into()),
        encoding: Encoding::Toml,
    };
    let found = found(&place, &roots).unwrap();
    assert_eq!(
        found,
        [
            (grants.join("claude.toml"), vec!["claude".to_string()]),
            (grants.join("mini.toml"), vec!["mini".to_string()]),
        ]
    );
    let into = Place {
        location: "lnk.migration-journal".into(),
        path: Some("*/grants.toml".into()),
        encoding: Encoding::Toml,
    };
    assert_eq!(
        at(&into, &roots, &found[1].1).unwrap(),
        Hidden::MigrationJournal
            .at_default(&roots.hidden)
            .join("mini/grants.toml")
    );
}

/// A note as a type no `specification!` declares, another crate's, its
/// format shipped beside the code that reads it.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plain {
    text: String,
}

static SHIPPED: std::sync::LazyLock<Format> =
    std::sync::LazyLock::new(|| Format::shipped(include_str!("tests/note.schema.toml")));

#[test]
fn a_shipped_format_converts_as_the_type_its_owner_reads() {
    let Some((_dir, roots)) = machine("upgrade-shipped") else {
        return;
    };
    let note = Hidden::Envs.at_default(&roots.hidden).join("note.toml");
    crate::write_private(&note, b"schema = 1\nsaid = \"hi\"\n").unwrap();
    let owners = [Saved::shipped::<Plain>(&SHIPPED)];
    let changes = upgrade_of(&owners, &roots).unwrap();
    assert!(
        changes
            .iter()
            .any(|change| matches!(change, Change::Convert { file, to: 2, .. } if *file == note)),
        "{changes:?}"
    );
    let text = std::fs::read_to_string(&note).unwrap();
    assert!(text.contains("schema = 2"), "{text}");
    assert_eq!(
        SHIPPED.read_saved::<Plain>(&text).unwrap().spec,
        Plain { text: "hi".into() }
    );
}
