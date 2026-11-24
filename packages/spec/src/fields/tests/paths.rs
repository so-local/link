use super::*;
use std::path::Path;

#[derive(Deserialize, Serialize, JsonSchema, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
struct Confined {
    /// A host-side input within this source bundle.
    #[schemars(extend("format" = crate::CONFINED_PATH_FORMAT))]
    #[schemars(example = &"input.toml")]
    source: PathBuf,
    /// More host-side inputs within the bundle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(extend("items" = {"type":"string","format":crate::CONFINED_PATH_FORMAT}))]
    more: Vec<PathBuf>,
    /// Inputs by name within the same bundle.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[schemars(extend("additionalProperties" = {"type":"string","format":crate::CONFINED_PATH_FORMAT}))]
    named: BTreeMap<String, PathBuf>,
    /// A path on the guest, kept as configuration rather than a host path.
    guest: Option<PathBuf>,
}
mod kind {
    crate::specification!(
        crate::fields::tests::paths::Confined,
        include_str!("../../../tests/fixtures/confined-schema.toml")
    );
}

fn split_at(words: &[&str], cwd: &Path) -> FieldFlags {
    let binding = Binding::of::<Confined>();
    super::super::given::split_from(args(words), Some(&binding), None, &[], cwd, cwd)
        .unwrap()
        .0
}

#[test]
fn a_nested_invocations_confined_path_uses_its_actual_source_folder() {
    let bundle = TempDir::new("confined-source");
    let nested = bundle.join("nested");
    std::fs::create_dir(&nested).unwrap();
    std::fs::write(bundle.join("input.toml"), "decoy").unwrap();
    std::fs::write(nested.join("input.toml"), "caller").unwrap();
    let spec = bundle.join("example.toml");
    let original = "schema = 1\nsource='input.toml'\nguest='guest/work'\n";
    std::fs::write(&spec, original).unwrap();
    let flags = split_at(
        &[
            "--source",
            "input.toml",
            "--more",
            "input.toml",
            "--named.extra",
            "input.toml",
        ],
        &nested,
    );
    let made = read_with::<Confined>(Some(&spec), &flags).unwrap();
    assert_eq!(made.spec.source, PathBuf::from("nested/input.toml"));
    assert_eq!(made.spec.more, [PathBuf::from("nested/input.toml")]);
    assert_eq!(made.spec.named["extra"], PathBuf::from("nested/input.toml"));
    assert_eq!(made.spec.guest, Some(PathBuf::from("guest/work")));
    assert_eq!(
        std::fs::read_to_string(bundle.join(&made.spec.source)).unwrap(),
        "caller"
    );
    let shown = show::<Confined>(
        &Show {
            spec: Some(spec.clone()),
            ..Show::default()
        },
        &flags,
        None,
    )
    .unwrap();
    assert_eq!(crate::parse_spec::<Confined>(&shown).unwrap(), made.spec);
    assert_eq!(std::fs::read_to_string(spec).unwrap(), original);
}

#[test]
fn confined_paths_outside_the_source_folder_are_refused_in_every_container() {
    let bundle = TempDir::new("confined-bundle");
    let sibling = TempDir::new("confined-sibling");
    let spec = bundle.join("example.toml");
    std::fs::write(&spec, "schema = 1\nsource='input.toml'\n").unwrap();
    for flag in ["--source", "--more", "--named.extra"] {
        let flags = split_at(&[flag, "input.toml"], &sibling);
        let error = read_with::<Confined>(Some(&spec), &flags).unwrap_err();
        assert!(
            format!("{error:#}").contains("outside the spec's folder"),
            "{error:#}"
        );
        assert!(
            show::<Confined>(
                &Show {
                    spec: Some(spec.clone()),
                    ..Show::default()
                },
                &flags,
                None
            )
            .is_err()
        );
    }
    // Without a source folder the text reader retains its input, for a
    // caller that resolves the text's paths in a different context.
    let raw = format!(
        "schema = 1\nsource={:?}",
        sibling.join("input.toml").display().to_string()
    );
    assert!(crate::read_text_with::<Confined>(&raw, &FieldFlags::default()).is_ok());
    assert!(crate::read_text_at::<Confined>(&raw, &bundle, &FieldFlags::default()).is_err());
}

#[test]
fn shared_file_reading_does_not_validate_a_field_that_flags_replace() {
    let dir = TempDir::new("shared-override");
    let file = file(&dir, "version='invalid'\nname='main'\n");
    let made = read_with::<Agent>(Some(&file), &flags(&["--spec.version", "2"])).unwrap();
    assert_eq!(made.spec.version, Some(2));
}
