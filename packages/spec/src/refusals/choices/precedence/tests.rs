use super::*;
use std::collections::BTreeMap;

fn format() -> &'static crate::schema::Format {
    static FORMAT: std::sync::LazyLock<crate::schema::Format> = std::sync::LazyLock::new(|| {
        crate::schema::Format::shipped(include_str!("../../../../tests/fixtures/spec-schema.toml"))
            .named()
    });
    &FORMAT
}
macro_rules! kind {
    ($ty:ty) => {
        impl crate::Specification for $ty {
            fn format() -> &'static crate::schema::Format {
                format()
            }
        }
        impl Kind for $ty {}
    };
}
fn context_bool(schema: &mut schemars::Schema) {
    // Additional metadata puts this actual bool outside the existing plain
    // finite-domain sampler; its contrasting value still needs actual controls.
    schema
        .get_mut("properties")
        .unwrap()
        .get_mut("enabled")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("x-context".into(), Value::Bool(true));
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = context_bool)]
struct Scalars {
    name: String,
    count: i64,
    ratio: f64,
    enabled: bool,
}
kind!(Scalars);
const INPUT: &str = "kind='example'\nschema=2\nname='alpha'\ncount=7\nratio=0.25\nenabled=false\n";
#[test]
fn accepted_scalar_contrasts_have_real_isolated_flag_controls_and_precedence() {
    let base = baseline_text::<Scalars>(INPUT, crate::flags_of::<Scalars>).unwrap();
    let report = checked_base::<Scalars>(
        &base,
        crate::flags_of::<Scalars>,
        crate::read_text_at::<Scalars>,
    )
    .unwrap();
    for field in ["name", "count", "ratio", "enabled"] {
        let witness = report
            .witnessed
            .iter()
            .find(|w| w.field == field && w.open_sample.is_some())
            .unwrap();
        assert_ne!(witness.precedence.as_ref(), Some(&witness.value));
        assert!(
            witness
                .isolated_flag
                .as_ref()
                .unwrap()
                .split_once('=')
                .is_some_and(
                    |(flag, _)| flag == format!("--{field}") || flag == format!("--spec.{field}")
                )
        );
        assert!(
            witness.rejected.is_none(),
            "no invalid-domain claim is made"
        );
    }
}
fn ignore_existing_count(
    text: &str,
    dir: &std::path::Path,
    flags: &FieldFlags,
) -> anyhow::Result<Scalars> {
    let input: toml::Value = toml::from_str(text)?;
    if input.get("count").is_some() {
        crate::read_text_at::<Scalars>(text, dir, &FieldFlags::default())
    } else {
        crate::read_text_at::<Scalars>(text, dir, flags)
    }
}
#[test]
fn a_reader_ignoring_only_the_conflicting_combination_cannot_pass_separate_controls() {
    let base = baseline_text::<Scalars>(INPUT, crate::flags_of::<Scalars>).unwrap();
    let binding = Binding::of::<Scalars>();
    let (flag, rest) = crate::split(["--count=8".into()], Some(&binding), None).unwrap();
    assert!(rest.is_empty());
    let mut input_b = base.input.clone();
    input_b
        .as_table_mut()
        .unwrap()
        .insert("count".into(), toml::Value::Integer(8));
    let file_b = crate::read_text_at::<Scalars>(
        &toml::to_string(&input_b).unwrap(),
        &base.dir,
        &FieldFlags::default(),
    )
    .unwrap();
    let expected = serde_json::to_value(file_b).unwrap();
    let mut context = base.input.clone();
    context.as_table_mut().unwrap().remove("count");
    let flag_b =
        ignore_existing_count(&toml::to_string(&context).unwrap(), &base.dir, &flag).unwrap();
    assert_eq!(
        serde_json::to_value(flag_b).unwrap(),
        expected,
        "the isolated explicit flag control really passes"
    );
    assert_ne!(
        serde_json::to_value(ignore_existing_count(INPUT, &base.dir, &flag).unwrap()).unwrap(),
        expected,
        "only the conflicting combined input is broken"
    );
    let error = checked_base::<Scalars>(&base, crate::flags_of::<Scalars>, ignore_existing_count)
        .unwrap_err();
    assert!(
        error.to_string().contains("open scalar precedence") && error.to_string().contains("count"),
        "{error:#}"
    );
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Restricted {
    keep: String,
    name: String,
}
impl crate::Specification for Restricted {
    fn format() -> &'static crate::schema::Format {
        format()
    }
}
impl Kind for Restricted {
    fn check(&self) -> anyhow::Result<()> {
        ensure!(
            self.name == "baseline",
            "only the baseline name is accepted"
        );
        Ok(())
    }
}
#[test]
fn a_generated_contrast_rejected_by_the_owning_reader_is_an_exclusion() {
    let base = baseline_text::<Restricted>(
        "kind='example'\nschema=2\nkeep='anchor'\nname='baseline'\n",
        crate::flags_of::<Restricted>,
    )
    .unwrap();
    let report = checked_base::<Restricted>(
        &base,
        crate::flags_of::<Restricted>,
        crate::read_text_at::<Restricted>,
    )
    .unwrap();
    assert!(!report.witnessed.iter().any(|w| w.field == "name"));
    assert!(
        report
            .excluded
            .iter()
            .any(|w| w.field == "name" && w.reason.contains("full file rejected"))
    );
}
#[derive(serde::Serialize)]
#[serde(transparent)]
struct Collapsed(String);
impl<'de> serde::Deserialize<'de> for Collapsed {
    fn deserialize<D: serde::Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        let input = String::deserialize(reader)?;
        Ok(Self(input.split('-').next().unwrap().into()))
    }
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct CollapsedSpec {
    #[schemars(with = "String")]
    name: Collapsed,
}
kind!(CollapsedSpec);
#[test]
fn an_accepted_raw_spelling_that_collapses_to_a_is_not_a_precedence_witness() {
    let base = baseline_text::<CollapsedSpec>(
        "kind='example'\nschema=2\nname='baseline'\n",
        crate::flags_of::<CollapsedSpec>,
    )
    .unwrap();
    let report = checked_base::<CollapsedSpec>(
        &base,
        crate::flags_of::<CollapsedSpec>,
        crate::read_text_at::<CollapsedSpec>,
    )
    .unwrap();
    assert!(report.witnessed.is_empty());
    assert!(
        report
            .excluded
            .iter()
            .any(|w| w.reason.contains("not observably different"))
    );
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = context_bool)]
struct Elided {
    keep: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    enabled: bool,
}
kind!(Elided);
#[test]
fn an_elided_contrast_is_not_an_observed_conflicting_value() {
    let base = baseline_text::<Elided>(
        "kind='example'\nschema=2\nkeep='anchor'\nenabled=true\n",
        crate::flags_of::<Elided>,
    )
    .unwrap();
    let report = checked_base::<Elided>(
        &base,
        crate::flags_of::<Elided>,
        crate::read_text_at::<Elided>,
    )
    .unwrap();
    assert!(!report.witnessed.iter().any(|w| w.field == "enabled"));
    assert!(
        report
            .excluded
            .iter()
            .any(|w| w.field == "enabled" && w.reason.contains("elided"))
    );
}
fn dropped_count(spec: &Scalars) -> anyhow::Result<Vec<String>> {
    let mut words = crate::flags_of(spec)?;
    if spec.count != 7 {
        let at = words.iter().position(|word| word == "--count").unwrap();
        words.drain(at..=at + 1);
    }
    Ok(words)
}
#[test]
fn an_untargetable_contrast_is_explicitly_excluded_not_counted() {
    let base = baseline_text::<Scalars>(INPUT, dropped_count).unwrap();
    let report =
        checked_base::<Scalars>(&base, dropped_count, crate::read_text_at::<Scalars>).unwrap();
    assert!(!report.witnessed.iter().any(|w| w.field == "count"));
    assert!(
        report
            .excluded
            .iter()
            .any(|w| w.field == "count" && w.reason.contains("full flag replay"))
    );
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Record {
    name: String,
    count: i64,
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Records {
    entries: Vec<Record>,
    map: BTreeMap<String, Record>,
}
kind!(Records);
#[test]
fn concrete_array_records_use_the_real_collection_flag_and_map_keys_stay_quoted() {
    let text = "kind='example'\nschema=2\n[[entries]]\nname='row'\ncount=7\n[map.'a/b~c']\nname='entry'\ncount=11\n";
    let base = baseline_text::<Records>(text, crate::flags_of::<Records>).unwrap();
    let report = checked_base::<Records>(
        &base,
        crate::flags_of::<Records>,
        crate::read_text_at::<Records>,
    )
    .unwrap();
    let array = report
        .witnessed
        .iter()
        .find(|w| w.pointer == "/entries/0/count")
        .unwrap();
    assert_eq!(array.field, "entries.count");
    assert!(
        array
            .isolated_flag
            .as_ref()
            .unwrap()
            .starts_with("--entries=")
    );
    let map = report
        .witnessed
        .iter()
        .find(|w| w.pointer == "/map/a~1b~0c/count")
        .unwrap();
    assert_eq!(map.field, "map.*.count");
    assert!(
        map.isolated_flag
            .as_ref()
            .unwrap()
            .starts_with("--map.\"a/b~c\".count=")
    );
    assert_eq!(array.precedence, Some(7.into()));
    assert_eq!(array.value, Value::from(8));
    assert_eq!(map.precedence, Some(11.into()));
    assert_eq!(map.value, Value::from(12));
}
