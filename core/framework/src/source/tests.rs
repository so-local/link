use super::*;

const OURS: &str = "0123456789abcdef0123456789abcdef01234567";
const THEIRS: &str = "fedcba9876543210fedcba9876543210fedcba98";

/// The variable a build script hands its sources in is the one
/// `source!` reads.
#[test]
fn the_build_script_and_the_program_name_one_variable() {
    assert_eq!(link_component::SOURCE_VARIABLE, "LNK_BUILT_FROM");
}

/// A module is refused only when both its sources and this program's are
/// known and differ; this program's unknown, nothing is asked.
#[test]
fn only_two_known_and_different_sources_are_refused() {
    let path = Path::new("/opt/link/lnk-sandbox");
    let asked = std::cell::Cell::new(false);
    let unasked = |_: &Path| {
        asked.set(true);
        Some(THEIRS.to_string())
    };
    assert_eq!(compare(None, "sandbox", path, unasked), Ok(()));
    assert!(!asked.get());
    let same = compare(Some(OURS), "sandbox", path, |_| Some(OURS.into()));
    assert_eq!(same, Ok(()));
    let unknown = compare(Some(OURS), "sandbox", path, |_| None);
    assert_eq!(unknown, Ok(()));
    let skew = compare(Some(OURS), "sandbox", path, |_| Some(THEIRS.into())).unwrap_err();
    assert_eq!(
        skew,
        Skew {
            module: "sandbox".into(),
            path: path.to_path_buf(),
            ours: OURS.into(),
            theirs: THEIRS.into(),
        }
    );
}

/// What it says names the module, where it is, both commits and the
/// commands that build them together.
#[test]
fn a_skew_names_both_sources_and_the_fix() {
    let skew = Skew {
        module: "sandbox".into(),
        path: PathBuf::from("/opt/link/lnk-sandbox"),
        ours: OURS.into(),
        theirs: THEIRS.into(),
    };
    let said = skew.to_string();
    for part in [
        "lnk-sandbox",
        "/opt/link/lnk-sandbox",
        &THEIRS[..12],
        &OURS[..12],
        "cargo build --release --workspace --bins",
        "lnk up --source",
    ] {
        assert!(said.contains(part), "{said}");
    }
}

/// A descriptor's sources are read for that field alone: a newer one
/// with more fields still says them, an older one without says none.
#[test]
fn a_descriptor_says_its_sources() {
    let newer = format!(r#"{{"protocol":1,"source":"{OURS}","more":true}}"#);
    assert_eq!(sources_in(newer.as_bytes()), Some(OURS.into()));
    assert_eq!(sources_in(br#"{"protocol":1}"#), None);
    assert_eq!(sources_in(br#"{"source":""}"#), None);
    assert_eq!(sources_in(b"not a descriptor"), None);
}

/// A script isn't run to ask: no build made it, so it has no sources.
#[cfg(unix)]
#[test]
fn a_script_is_never_run_to_ask() {
    let dir = link_test_kit::TempDir::new("source-script");
    let ran = dir.join("ran");
    let script = dir.join("lnk-fake");
    link_test_kit::write_program(
        &script,
        format!(
            "#!/bin/sh\ntouch '{}'\necho '{{\"source\":\"{THEIRS}\"}}'\n",
            ran.display()
        ),
    )
    .unwrap();
    assert_eq!(of(&script), None);
    assert!(!ran.exists());
}

/// An answerer compares the asker's sources with its own, nothing run,
/// and refuses by its own name only when both are known and differ.
#[test]
fn an_answerer_refuses_an_asker_built_apart() {
    let path = Path::new("/opt/link/lnk-agent");
    assert_eq!(answered(None, path, Some(OURS)), Ok(()));
    assert_eq!(answered(Some(""), path, Some(OURS)), Ok(()));
    assert_eq!(answered(Some(THEIRS), path, None), Ok(()));
    assert_eq!(answered(Some(OURS), path, Some(OURS)), Ok(()));
    assert_eq!(
        answered(Some(THEIRS), path, Some(OURS)),
        Err(Skew {
            module: "agent".into(),
            path: path.to_path_buf(),
            ours: THEIRS.into(),
            theirs: OURS.into(),
        })
    );
}
