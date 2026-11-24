use super::*;
use serde::ser::SerializeMap;

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct LeakedInvocation {
    /// The reusable name.
    name: String,
}

impl serde::Serialize for LeakedInvocation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut table = serializer.serialize_map(Some(2))?;
        table.serialize_entry("name", &self.name)?;
        // Mutation: a runtime argument escaped into the printed spec,
        // without changing its correct schema or independent reader.
        table.serialize_entry("json", &true)?;
        table.end()
    }
}

impl crate::Specification for LeakedInvocation {
    fn format() -> &'static crate::schema::Format {
        static FORMAT: std::sync::LazyLock<crate::schema::Format> =
            std::sync::LazyLock::new(|| {
                crate::schema::Format::shipped(include_str!(
                    "../../tests/fixtures/spec-schema.toml"
                ))
                .named()
            });
        &FORMAT
    }
}
impl Kind for LeakedInvocation {}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct StableValue {
    #[schemars(example = &"from_file")]
    name: String,
}
impl crate::Specification for StableValue {
    fn format() -> &'static crate::schema::Format {
        <LeakedInvocation as crate::Specification>::format()
    }
}
impl Kind for StableValue {}

fn drifted_flags(_: &StableValue) -> anyhow::Result<Vec<String>> {
    Ok(vec!["--spec.name".into(), "from_flags".into()])
}

#[test]
fn a_stable_flag_replay_cannot_hide_a_changed_typed_value() {
    let dir = std::env::current_dir().unwrap();
    let binding = Binding::of::<StableValue>();
    let text = crate::reference::every_option_text(&Exported::of::<StableValue>());
    let file = crate::read_text_at::<StableValue>(&text, &dir, &FieldFlags::default()).unwrap();
    let (flags, _) = crate::split(
        drifted_flags(&file).unwrap().into_iter().map(Into::into),
        Some(&binding),
        None,
    )
    .unwrap();
    let replay = crate::read_text_at::<StableValue>("", &dir, &flags).unwrap();
    assert_ne!(
        file.name, replay.name,
        "the mutation changes the typed value"
    );
    let (fixed, _) = crate::split(
        drifted_flags(&replay).unwrap().into_iter().map(Into::into),
        Some(&binding),
        None,
    )
    .unwrap();
    assert_eq!(
        flags.table(),
        fixed.table(),
        "the previous fixedpoint check alone accepts this mutation"
    );
    let error = checked_with::<StableValue>(drifted_flags).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("field flags change the typed configuration"),
        "{error:#}"
    );
    assert!(
        check_roundtrip::<StableValue>().is_empty(),
        "the genuine emitter preserves this sample"
    );
}

#[test]
fn a_runtime_argument_leaked_by_the_serializer_fails_at_the_shared_boundary() {
    assert!(crate::check_shape::<LeakedInvocation>().is_empty());
    let problems = check_roundtrip::<LeakedInvocation>();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("reading the printed spec")
            && problems[0].contains("unknown field `json`"),
        "{problems:?}"
    );
}
