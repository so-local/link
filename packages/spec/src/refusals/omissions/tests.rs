use super::*;
use std::collections::BTreeMap;

macro_rules! kind {
    ($ty:ty) => {
        impl crate::Specification for $ty {
            fn format() -> &'static crate::schema::Format {
                static FORMAT: std::sync::LazyLock<crate::schema::Format> =
                    std::sync::LazyLock::new(|| {
                        crate::schema::Format::shipped(include_str!(
                            "../../../tests/fixtures/spec-schema.toml"
                        ))
                        .named()
                    });
                &FORMAT
            }
        }
        impl Kind for $ty {}
    };
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Record {
    keep: String,
    optional: Option<bool>,
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct SimpleSpec {
    keep: String,
    required: String,
    optional: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    records: Vec<Record>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    map: BTreeMap<String, Record>,
    nullable: Option<Record>,
}
kind!(SimpleSpec);
const SIMPLE: &str =
    "kind='example'\nschema=2\nkeep='anchor'\nrequired='required'\noptional='chosen'\n";
fn run<T: Kind>(text: &str) -> anyhow::Result<Omissions> {
    checked_base::<T>(
        &baseline_text::<T>(text, crate::flags_of::<T>)?,
        crate::flags_of::<T>,
    )
}
#[test]
fn optional_none_is_observed_and_requiredness_is_independent() {
    let report = run::<SimpleSpec>(SIMPLE).unwrap();
    let witness = report
        .witnessed
        .iter()
        .find(|w| w.field == "optional")
        .unwrap();
    assert!(matches!(&witness.result, OmissionState::Retained { value } if value.is_null()));
    assert_eq!(witness.pointer, "/optional");
    assert!(witness.declared_default.is_none());
    assert!(!report.witnessed.iter().any(|w| w.field == "required"));
    assert!(
        report
            .excluded
            .iter()
            .any(|w| w.field == "required" && w.reason.contains("independently requires"))
    );
}
#[test]
fn optional_properties_in_concrete_map_array_and_nullable_records_are_kept() {
    let text = format!(
        "{SIMPLE}\n[[records]]\nkeep='row'\noptional=true\n[map.'a/b~c']\nkeep='entry'\noptional=true\n[nullable]\nkeep='present'\noptional=true\n"
    );
    let report = run::<SimpleSpec>(&text).unwrap();
    for (field, pointer) in [
        ("records.optional", "/records/0/optional"),
        ("map.*.optional", "/map/a~1b~0c/optional"),
        ("nullable.optional", "/nullable/optional"),
    ] {
        assert!(
            report.witnessed.iter().any(|w| w.field == field
                && w.pointer == pointer
                && matches!(&w.result, OmissionState::Retained { value } if value.is_null())),
            "{field}: {report:?}"
        );
    }
    assert!(!report.witnessed.iter().any(|w| w.pointer == "/records/0"));
    assert!(!report.witnessed.iter().any(|w| w.pointer == "/map/a~1b~0c"));
}
#[test]
fn an_allof_required_only_sibling_is_not_flattened_into_optional() {
    let mut base = baseline_text::<SimpleSpec>(SIMPLE, crate::flags_of::<SimpleSpec>).unwrap();
    base.schema.as_object_mut().unwrap().insert(
        "allOf".into(),
        serde_json::json!([{"required":["optional"]}]),
    );
    let report = checked_base::<SimpleSpec>(&base, crate::flags_of::<SimpleSpec>).unwrap();
    assert!(!report.witnessed.iter().any(|w| w.field == "optional"));
    assert!(
        report
            .excluded
            .iter()
            .any(|w| w.field == "optional" && w.reason.contains("unambiguous"))
    );
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Tagged {
    One {
        keep: String,
        optional: Option<bool>,
    },
    Two {
        keep: String,
        other: Option<bool>,
    },
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct TaggedSpec {
    keep: String,
    record: Tagged,
}
kind!(TaggedSpec);
#[test]
fn only_the_actual_tag_selected_record_receives_an_omission_witness() {
    let report = run::<TaggedSpec>(
        "kind='example'\nschema=2\nkeep='root'\n[record]\nkind='One'\nkeep='row'\noptional=true\n",
    )
    .unwrap();
    assert!(
        report
            .witnessed
            .iter()
            .any(|w| w.field == "record.optional" && w.pointer == "/record/optional")
    );
    assert!(!report.witnessed.iter().any(|w| w.field == "record.other"));
}
fn optional_value(schema: &mut schemars::Schema) {
    if let Some(required) = schema.get_mut("required").and_then(Value::as_array_mut) {
        required.retain(|value| value.as_str() != Some("value"));
    }
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = optional_value)]
struct ActuallyRequired {
    keep: String,
    value: String,
}
kind!(ActuallyRequired);
#[test]
fn a_reader_rejecting_a_structurally_optional_omission_is_excluded() {
    let report =
        run::<ActuallyRequired>("kind='example'\nschema=2\nkeep='anchor'\nvalue='present'\n")
            .unwrap();
    assert!(!report.witnessed.iter().any(|w| w.field == "value"));
    assert!(
        report
            .excluded
            .iter()
            .any(|w| w.field == "value" && w.reason.contains("owning reader rejects"))
    );
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Solo {
    optional: Option<bool>,
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct EmptyParent {
    keep: String,
    record: Solo,
}
kind!(EmptyParent);
#[test]
fn a_newly_empty_parent_is_excluded_without_pruning_it() {
    let report =
        run::<EmptyParent>("kind='example'\nschema=2\nkeep='anchor'\n[record]\noptional=true\n")
            .unwrap();
    assert!(
        !report
            .witnessed
            .iter()
            .any(|w| w.field == "record.optional")
    );
    assert!(
        report
            .excluded
            .iter()
            .any(|w| w.field == "record.optional" && w.reason.contains("empty parent"))
    );
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = hidden_default)]
struct Hidden {
    keep: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    optional: Option<String>,
}
fn hidden_default(schema: &mut schemars::Schema) {
    schema
        .get_mut("properties")
        .unwrap()
        .get_mut("optional")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("default".into(), Value::Null);
}
kind!(Hidden);
#[test]
fn accepted_serialized_absence_is_distinct_from_retained_null() {
    let report =
        run::<Hidden>("kind='example'\nschema=2\nkeep='anchor'\noptional='present'\n").unwrap();
    let absent = report
        .witnessed
        .iter()
        .find(|w| w.field == "optional")
        .unwrap();
    assert_eq!(absent.result, OmissionState::SerializedAbsent);
    assert_eq!(
        serde_json::to_value(&absent.result).unwrap(),
        serde_json::json!({"state":"serialized-absent"})
    );
    // A declared default is not asserted when the actual value is hidden.
    assert!(
        crate::schema::<Hidden>()["properties"]["optional"]
            .get("default")
            .is_some()
    );
    assert!(absent.declared_default.is_none());
    let retained = run::<SimpleSpec>(SIMPLE).unwrap();
    let retained = retained
        .witnessed
        .iter()
        .find(|w| w.field == "optional")
        .unwrap();
    assert_eq!(
        serde_json::to_value(&retained.result).unwrap(),
        serde_json::json!({"state":"retained","value":null})
    );
}

fn wrong_default(schema: &mut schemars::Schema) {
    schema
        .get_mut("properties")
        .unwrap()
        .get_mut("enabled")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("default".into(), Value::Bool(true));
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = wrong_default)]
struct DefaultMismatch {
    keep: String,
    #[serde(default)]
    enabled: bool,
}
kind!(DefaultMismatch);
#[test]
fn an_independently_declared_default_cannot_confirm_a_different_reader_default() {
    let error = run::<DefaultMismatch>("kind='example'\nschema=2\nkeep='anchor'\nenabled=true\n")
        .unwrap_err();
    assert!(
        error.to_string().contains("independently declared default"),
        "{error:#}"
    );
}
fn drifted(spec: &SimpleSpec) -> anyhow::Result<Vec<String>> {
    let mut flags = crate::flags_of(spec)?;
    if spec.optional.is_none() {
        flags.extend(["--optional".into(), "chosen".into()]);
    }
    Ok(flags)
}
#[test]
fn flags_that_restore_the_removed_value_fail_despite_a_good_positive_baseline() {
    let base = baseline_text::<SimpleSpec>(SIMPLE, drifted).unwrap();
    let error = checked_base::<SimpleSpec>(&base, drifted).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("omitted file and emitted field flags change"),
        "{error:#}"
    );
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct InvalidEmptyFlags {
    keep: String,
    #[serde(default)]
    map: BTreeMap<String, String>,
}
kind!(InvalidEmptyFlags);
#[test]
fn a_real_invalid_empty_map_flag_is_still_rejected_before_omission_probes() {
    let error = run::<InvalidEmptyFlags>("kind='example'\nschema=2\nkeep='anchor'\n").unwrap_err();
    assert!(
        error.to_string().contains("reading unmutated field flags"),
        "{error:#}"
    );
    assert!(format!("{error:#}").contains("[map] is empty"), "{error:#}");
}

fn hidden_drifted(spec: &Hidden) -> anyhow::Result<Vec<String>> {
    let mut flags = crate::flags_of(spec)?;
    if spec.optional.is_none() {
        flags.extend(["--optional".into(), "present".into()]);
    }
    Ok(flags)
}
#[test]
fn serialized_absence_cannot_waive_flags_that_restore_the_removed_field() {
    let text = "kind='example'\nschema=2\nkeep='anchor'\noptional='present'\n";
    let base = baseline_text::<Hidden>(text, hidden_drifted).unwrap();
    let error = checked_base::<Hidden>(&base, hidden_drifted).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("omitted file and emitted field flags change"),
        "{error:#}"
    );
}
