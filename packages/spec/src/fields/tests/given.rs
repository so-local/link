//! Reading a given spec file beside its flags.

use super::*;

/// A file names its schema, empty or not: an empty `--spec` file is
/// refused alone and under flags, as the kind's own reader refuses empty
/// text, while flags with no file are current.
#[test]
fn an_empty_spec_file_is_refused_and_flags_alone_are_current() {
    let dir = TempDir::new("empty-spec");
    let empty = dir.path().join("spec.toml");
    std::fs::write(&empty, "").unwrap();
    let named = flags(&["--spec.name", "b"]);
    for given in [&FieldFlags::default(), &named] {
        let refused = crate::fields::read_with::<Named>(Some(&empty), given).unwrap_err();
        assert!(format!("{refused:#}").contains("schema"), "{refused:#}");
    }
    assert!(crate::parse_spec::<Named>("").is_err());
    let alone = crate::fields::read_with::<Named>(None, &named).unwrap();
    assert_eq!(alone.spec.name.as_deref(), Some("b"));
}
