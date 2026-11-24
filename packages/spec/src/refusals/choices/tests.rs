use super::*;

#[derive(Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Fast,
    Safe,
}

#[derive(serde::Serialize)]
#[serde(transparent)]
struct Collapsed(String);
impl<'de> serde::Deserialize<'de> for Collapsed {
    fn deserialize<D: serde::Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        let _ = Mode::deserialize(reader)?;
        Ok(Self("fast".into()))
    }
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct CollapsedSpec {
    #[schemars(with = "Mode")]
    mode: Collapsed,
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct OpenSpec {
    #[schemars(with = "Mode")]
    mode: String,
}

fn format() -> &'static crate::schema::Format {
    static FORMAT: std::sync::LazyLock<crate::schema::Format> = std::sync::LazyLock::new(|| {
        crate::schema::Format::shipped(include_str!("../../../tests/fixtures/spec-schema.toml"))
            .named()
    });
    &FORMAT
}
impl crate::Specification for CollapsedSpec {
    fn format() -> &'static crate::schema::Format {
        format()
    }
}
impl Kind for CollapsedSpec {}
impl crate::Specification for OpenSpec {
    fn format() -> &'static crate::schema::Format {
        format()
    }
}
impl Kind for OpenSpec {}

#[test]
fn a_stable_baseline_cannot_hide_a_reader_collapsing_a_declared_choice() {
    assert!(crate::check_roundtrip::<CollapsedSpec>().is_empty());
    assert!(crate::check_type_refusals::<CollapsedSpec>().is_empty());
    let changed = crate::read_text_at::<CollapsedSpec>(
        "kind='example'\nschema=2\nmode='safe'\n",
        &std::env::current_dir().unwrap(),
        &FieldFlags::default(),
    )
    .unwrap();
    assert_eq!(
        changed.mode.0, "fast",
        "the mutation genuinely collapses safe"
    );
    let problems = check_scalar_choices::<CollapsedSpec>();
    assert!(
        problems
            .iter()
            .any(|p| p.contains("mode") && p.contains("preserve")),
        "{problems:?}"
    );
}

#[test]
fn a_reader_accepting_a_word_outside_the_closed_domain_is_found() {
    assert!(crate::check_roundtrip::<OpenSpec>().is_empty());
    assert!(crate::check_type_refusals::<OpenSpec>().is_empty());
    let changed = crate::read_text_at::<OpenSpec>(
        "kind='example'\nschema=2\nmode='__link_spec_invalid_choice__'\n",
        &std::env::current_dir().unwrap(),
        &FieldFlags::default(),
    )
    .unwrap();
    assert_eq!(changed.mode, "__link_spec_invalid_choice__");
    let problems = check_scalar_choices::<OpenSpec>();
    assert!(
        problems
            .iter()
            .any(|p| p.contains("mode") && p.contains("outside")),
        "{problems:?}"
    );
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
enum OpenMode {
    Mode(Mode),
    Text(String),
}
fn bounded(schema: &mut schemars::Schema) {
    // The wrapper's constraint must not disappear when At follows its arm.
    let inner = serde_json::to_value(&schema).unwrap();
    *schema = serde_json::json!({"anyOf": [inner, {"type": "null"}], "minLength": 1})
        .try_into()
        .unwrap();
}
fn off(value: &bool) -> bool {
    !value
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ChoicesSpec {
    mode: Mode,
    on: bool,
    #[serde(default, skip_serializing_if = "off")]
    quiet: bool,
    #[schemars(transform = bounded)]
    bounded: Mode,
    open: OpenMode,
}
impl crate::Specification for ChoicesSpec {
    fn format() -> &'static crate::schema::Format {
        format()
    }
}
impl Kind for ChoicesSpec {}

#[test]
fn finite_choices_and_an_accepted_open_contrast_keep_distinct_proof_scopes() {
    let report = scalar_choices::<ChoicesSpec>().unwrap();
    let found: Vec<_> = report
        .witnessed
        .iter()
        .filter(|w| w.open_sample.is_none())
        .map(|w| (w.field.as_str(), w.value.clone()))
        .collect();
    assert_eq!(found.len(), 5, "{report:?}");
    for (field, value) in [
        ("mode", serde_json::json!("fast")),
        ("mode", serde_json::json!("safe")),
        ("on", serde_json::json!(false)),
        ("on", serde_json::json!(true)),
        ("quiet", serde_json::json!(true)),
    ] {
        assert!(found.contains(&(field, value)), "{found:?}");
    }
    assert!(
        report
            .witnessed
            .iter()
            .filter(|w| w.field == "mode")
            .all(|w| w.rejected == Some(serde_json::json!("__link_spec_invalid_choice__")))
    );
    assert!(
        report
            .witnessed
            .iter()
            .filter(|w| w.field == "on")
            .all(|w| w.rejected.is_none())
    );
    assert!(
        report
            .witnessed
            .iter()
            .all(|w| w.precedence.as_ref().is_some_and(|file| file != &w.value)),
        "{report:?}"
    );
    let contrasts: Vec<_> = report
        .witnessed
        .iter()
        .filter(|w| w.open_sample.is_some())
        .collect();
    assert_eq!(contrasts.len(), 1, "{report:?}");
    let open = contrasts[0];
    assert_eq!(open.field, "open");
    assert_eq!(open.open_sample, Some(serde_json::json!("fast-contrast")));
    assert_eq!(open.value, serde_json::json!("fast-contrast"));
    assert_eq!(open.precedence, Some(serde_json::json!("fast")));
    assert!(
        open.isolated_flag
            .as_ref()
            .is_some_and(|flag| flag.starts_with("--open="))
    );
    assert!(
        open.rejected.is_none(),
        "a generated accepted sample proves no finite-domain refusal or default"
    );
    assert!(!report.witnessed.iter().any(|w| w.field == "bounded"));
    assert!(
        report
            .excluded
            .iter()
            .any(|e| e.field == "bounded" && e.reason.contains("contrasting full file rejected")),
        "{report:?}"
    );
    assert!(
        report
            .excluded
            .iter()
            .any(|e| e.field == "quiet"
                && e.reason.contains("false absent from typed serialization")),
        "{report:?}"
    );
}
fn drifted_flags(spec: &ChoicesSpec) -> anyhow::Result<Vec<String>> {
    let mut words = crate::flags_of(spec)?;
    if matches!(spec.mode, Mode::Safe) {
        let index = words.iter().position(|word| word == "--mode").unwrap();
        words[index + 1] = "fast".into();
    }
    Ok(words)
}
#[test]
fn the_flag_route_must_preserve_each_new_choice_not_only_the_baseline() {
    assert!(baseline::<ChoicesSpec>(drifted_flags).is_ok());
    let error = checked_with::<ChoicesSpec>(drifted_flags).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("flags change the typed choice \"safe\""),
        "{error:#}"
    );
}

fn ignores_flags_when_a_file_is_present(
    text: &str,
    dir: &std::path::Path,
    _: &FieldFlags,
) -> anyhow::Result<ChoicesSpec> {
    crate::read_text_at::<ChoicesSpec>(text, dir, &FieldFlags::default())
}
#[test]
fn ignoring_real_flags_only_when_a_file_is_present_is_detected() {
    assert!(crate::check_roundtrip::<ChoicesSpec>().is_empty());
    assert!(check_scalar_choices::<ChoicesSpec>().is_empty());
    let error = checked_reader_with::<ChoicesSpec>(
        crate::flags_of::<ChoicesSpec>,
        ignores_flags_when_a_file_is_present,
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("precedence") && error.to_string().contains("mode"),
        "{error:#}"
    );
}
