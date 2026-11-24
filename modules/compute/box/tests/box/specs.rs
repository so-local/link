//! The box spec cases, `tests/specs/box/*.toml`: each a spec as a
//! user writes it (`[spec]`), how it's run (`[run]`: the account's cloud,
//! `kind`, the flags, whether the adapter has `sizes` and what it fails
//! with), and what
//! must come of it (`[expect]`): the `size` the box module picks from the
//! fake adapter's sizes (`sizes.json`) and a `note` it warns of, the
//! `error` it gives, or what
//! reading it says when it's `invalid`. Every spec a case makes is
//! printed and read back as itself. A new case is a new file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use link_box::Offered;
use link_box::spec::BoxSpec;
use link_cloud_contract::MachineSize;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    #[serde(default)]
    spec: Option<toml::Table>,
    #[serde(default)]
    run: Run,
    expect: Expect,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Run {
    #[serde(default = "aws")]
    kind: String,
    #[serde(default)]
    flags: Vec<String>,
    /// The `--zone` of the slots, which is the spec's region.
    #[serde(default)]
    zone: Option<String>,
    /// Whether the adapter has `sizes`.
    #[serde(default = "yes")]
    sizes: bool,
    /// What `sizes` fails with, if it does.
    #[serde(default)]
    sizes_error: Option<String>,
}

impl Default for Run {
    fn default() -> Self {
        Run {
            kind: aws(),
            flags: Vec::new(),
            zone: None,
            sizes: true,
            sizes_error: None,
        }
    }
}

fn aws() -> String {
    "aws".into()
}

fn yes() -> bool {
    true
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Expect {
    size: Option<String>,
    /// With `size`: a warning said before the question.
    note: Option<String>,
    error: Option<String>,
    invalid: Option<String>,
}

/// What the fake adapter of each cloud lists.
#[derive(Deserialize)]
struct Cloud {
    place: String,
    default: String,
    sizes: Vec<MachineSize>,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// What came of a case: the size picked, the error as rendered, or why
/// it's invalid.
#[derive(Debug, PartialEq)]
enum Outcome {
    Size(String, Vec<String>),
    Error(String),
    Invalid(String),
}

fn run(case: &Case, clouds: &BTreeMap<String, Cloud>) -> Outcome {
    let kind = case.run.kind.as_str();
    let dir = link_test_kit::temp::TempDir::new("box-spec-case");
    // A case with no spec is made from its flags alone, as a command
    // given no file makes it.
    let file = case.spec.as_ref().map(|spec| {
        let file = dir.path().join("box.toml");
        std::fs::write(&file, toml::to_string(spec).unwrap()).unwrap();
        file
    });
    let args = std::iter::once("lnk".to_string())
        .chain(case.run.flags.iter().cloned())
        .map(Into::into);
    let binding = link_spec::Binding::of::<BoxSpec>();
    let spec = link_spec::split(args, Some(&binding), None).and_then(|(flags, _)| {
        let zone = case.run.zone.as_deref().map(|zone| (kind, zone));
        BoxSpec::made(file.as_deref(), &flags, zone).map(|made| made.spec)
    });
    let spec = match spec {
        Ok(spec) => spec,
        Err(err) => return Outcome::Invalid(format!("{err:#}")),
    };
    // The spec the box is made from, printed (as `lnk box spec` keeps
    // and prints it), reads back as itself.
    let printed = link_spec::print(&spec).unwrap();
    let again = link_spec::parse::<BoxSpec>(&printed)
        .and_then(|again| again.check(&printed, false).map(|()| again));
    match again {
        Ok(again) if again == spec => {}
        Ok(other) => return Outcome::Invalid(format!("printed, it reads back as {other:?}")),
        Err(err) => return Outcome::Invalid(format!("printed, it's refused: {err:#}\n{printed}")),
    }
    let cloud = &clouds[kind];
    let want = spec.want(kind);
    // Asked only when the box module would ask.
    let listed =
        link_box::asks_sizes(&want).then(|| match (case.run.sizes, &case.run.sizes_error) {
            (false, _) => Offered::NoCommand,
            (true, Some(why)) => Offered::Failed(why.clone()),
            (true, None) => Offered::Sizes(cloud.sizes.clone(), None),
        });
    let chosen = want.check_buildable().and_then(|()| {
        link_box::choose_from(
            kind,
            listed,
            &cloud.place,
            Some(cloud.default.clone()),
            &want,
        )
    });
    match chosen {
        Ok(chosen) => Outcome::Size(chosen.size, chosen.notes),
        Err(err) => Outcome::Error(rendered(err)),
    }
}

/// `err` as the person reads it: the box module's report, its why
/// under its headline, rendered plain.
fn rendered(err: anyhow::Error) -> String {
    use link_format::render;
    render::report(&link_box::said::report(err), None, &render::Ctx::plain())
}

/// Every box spec the docs show is one Link reads: each `toml` block of
/// the boxes pages and the Box Spec's, which the website shows too.
#[test]
fn every_box_spec_in_the_docs_is_read() {
    let root = root();
    let pages: Vec<PathBuf> =
        std::fs::read_dir(root.join("modules/compute/box/docs/services/boxes"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "md"))
            .chain([
                root.join("modules/compute/box/docs/config/specs/box.md"),
                root.join("modules/compute/box/docs/config/specs/guides/box.md"),
            ])
            .collect();
    let mut read = 0;
    for page in pages {
        let text = std::fs::read_to_string(&page).unwrap();
        let mut block: Option<String> = None;
        for line in text.lines() {
            match (&mut block, line.trim_start()) {
                (None, "```toml") => block = Some(String::new()),
                // A setup stage's sandbox spec: the sandbox's test reads it.
                (Some(spec), "```") if is_sandbox_spec(spec) => block = None,
                (Some(spec), "```") => {
                    if let Err(err) = BoxSpec::parse(spec) {
                        panic!("{}: {err:#}\n{spec}", page.display());
                    }
                    read += 1;
                    block = None;
                }
                (Some(spec), _) => {
                    spec.push_str(line);
                    spec.push('\n');
                }
                (None, _) => {}
            }
        }
    }
    assert!(read >= 2, "the docs' box specs");
}

#[test]
fn every_box_spec_case_comes_out_as_it_says() {
    let dir = root().join("tests/specs/box");
    let clouds: BTreeMap<String, Cloud> =
        serde_json::from_str(&std::fs::read_to_string(dir.join("sizes.json")).unwrap()).unwrap();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    assert!(files.len() > 20, "the cases in {}", dir.display());
    let mut failed = Vec::new();
    for file in &files {
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        let case: Case = match toml::from_str(&std::fs::read_to_string(file).unwrap()) {
            Ok(case) => case,
            Err(err) => {
                failed.push(format!("{name}: not a case: {err}"));
                continue;
            }
        };
        let outcome = run(&case, &clouds);
        let ok = match (&case.expect, &outcome) {
            (
                Expect {
                    size: Some(want),
                    note,
                    error: None,
                    invalid: None,
                },
                Outcome::Size(got, notes),
            ) => {
                want == got
                    && note
                        .as_ref()
                        .is_none_or(|note| notes.iter().any(|n| n.contains(note.as_str())))
            }
            (
                Expect {
                    size: None,
                    note: None,
                    error: Some(want),
                    invalid: None,
                },
                Outcome::Error(got),
            ) => got.contains(want.as_str()),
            (
                Expect {
                    size: None,
                    note: None,
                    error: None,
                    invalid: Some(want),
                },
                Outcome::Invalid(got),
            ) => got.contains(want.as_str()),
            _ => false,
        };
        if !ok {
            failed.push(format!(
                "{name}: expected {:?}, got {outcome:?}",
                case.expect
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Whether `text` is a sandbox spec: a table with one of its sections.
fn is_sandbox_spec(text: &str) -> bool {
    const SECTIONS: &[&str] = &[
        "files",
        "internet",
        "lan",
        "localhost",
        "tools",
        "browser",
        "commands",
    ];
    toml::from_str::<toml::Table>(text).is_ok_and(|t| SECTIONS.iter().any(|s| t.contains_key(*s)))
}
