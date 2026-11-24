use super::*;
fn one() -> serde_json::Value {
    serde_json::json!({"kind":"one", "label":"baseline"})
}
fn two() -> serde_json::Value {
    serde_json::json!({"kind":"two", "count":7, "note":"kept"})
}
fn incomplete() -> serde_json::Value {
    serde_json::json!({"kind":"two"})
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[schemars(example=one(),example=two(),example=incomplete())]
enum Mode {
    One {
        label: String,
    },
    Two {
        count: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Strict {
    mode: Mode,
}
fn format() -> &'static crate::schema::Format {
    static FORMAT: std::sync::LazyLock<crate::schema::Format> = std::sync::LazyLock::new(|| {
        crate::schema::Format::shipped(include_str!("../../../tests/fixtures/spec-schema.toml"))
            .named()
    });
    &FORMAT
}
impl crate::Specification for Strict {
    fn format() -> &'static crate::schema::Format {
        format()
    }
}
impl Kind for Strict {}
#[test]
fn a_maintained_second_arm_must_qualify_as_a_complete_file_and_flag_baseline() {
    let report = example_readers::<Strict>().unwrap();
    assert_eq!(report.witnessed.len(), 1, "{report:?}");
    let sample = &report.witnessed[0];
    assert_eq!(sample.pointer, "/mode");
    assert_eq!(sample.typed["mode"]["kind"], "two");
    assert_eq!(sample.input["mode"]["count"], 7);
    assert_eq!(sample.flags["mode"]["count"], 7);
    assert!(
        sample
            .types
            .witnessed
            .iter()
            .any(|w| w.pointer == "/mode/count" && w.counterexample == "false")
    );
    let omitted = sample
        .omissions
        .witnessed
        .iter()
        .find(|w| w.pointer == "/mode/note")
        .expect("the maintained alternate arm's optional field was omitted");
    assert_eq!(omitted.result, OmissionState::SerializedAbsent);
    assert_eq!(omitted.declared_default, None);
    assert!(
        report
            .excluded
            .iter()
            .any(|e| e.value == incomplete() && e.reason.contains("count")),
        "{report:?}"
    );
}
#[derive(serde::Serialize)]
#[serde(transparent)]
struct LooseCount(u64);
impl<'de> serde::Deserialize<'de> for LooseCount {
    fn deserialize<D: serde::Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Input {
            Count(u64),
            Boolean(bool),
        }
        Ok(Self(match Input::deserialize(reader)? {
            Input::Count(value) => value,
            Input::Boolean(value) => u64::from(value),
        }))
    }
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[schemars(example=one(),example=two())]
enum LooseMode {
    One {
        label: String,
    },
    Two {
        #[schemars(with = "u64")]
        count: LooseCount,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Loose {
    mode: LooseMode,
}
impl crate::Specification for Loose {
    fn format() -> &'static crate::schema::Format {
        format()
    }
}
impl Kind for Loose {}
#[test]
fn a_permissive_reader_in_a_maintained_alternate_arm_is_found() {
    assert!(crate::check_roundtrip::<Loose>().is_empty());
    assert!(crate::check_type_refusals::<Loose>().is_empty());
    let changed = crate::read_text_at::<Loose>(
        "kind='example'\nschema=2\n[mode]\nkind='two'\ncount=false\n",
        &std::env::current_dir().unwrap(),
        &FieldFlags::default(),
    )
    .unwrap();
    assert!(matches!(
        changed.mode,
        LooseMode::Two {
            count: LooseCount(0),
            ..
        }
    ));
    let problems = check_example_readers::<Loose>();
    assert!(
        problems
            .iter()
            .any(|p| p.contains("mode.count") && p.contains("accepted") && p.contains("boolean")),
        "{problems:?}"
    );
}

fn drifted_alternate_flags(spec: &Strict) -> anyhow::Result<Vec<String>> {
    let mut words = crate::flags_of(spec)?;
    if matches!(spec.mode, Mode::Two { .. }) {
        let index = words
            .iter()
            .position(|word| word == "--mode.count")
            .unwrap();
        words[index + 1] = "99".into();
    }
    Ok(words)
}
#[test]
fn accepted_files_with_broken_flag_replay_are_failures_not_context_exclusions() {
    assert!(baseline::<Strict>(drifted_alternate_flags).is_ok());
    let error = checked_with::<Strict>(drifted_alternate_flags).unwrap_err();
    assert!(
        error.to_string().contains("file and flag baselines differ"),
        "{error:#}"
    );
}
