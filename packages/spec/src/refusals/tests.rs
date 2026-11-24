use super::*;

#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(transparent)]
struct LooseText(String);
impl<'de> serde::Deserialize<'de> for LooseText {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Input {
            Text(String),
            Boolean(bool),
        }
        Ok(Self(match Input::deserialize(deserializer)? {
            Input::Text(text) => text,
            Input::Boolean(value) => value.to_string(),
        }))
    }
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct LooseSpec {
    #[schemars(example = &"baseline")]
    label: LooseText,
}
impl crate::Specification for LooseSpec {
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
impl Kind for LooseSpec {}

#[test]
fn a_permissive_reader_cannot_hide_behind_a_stable_every_option_roundtrip() {
    assert!(crate::check_roundtrip::<LooseSpec>().is_empty());
    let dir = std::env::current_dir().unwrap();
    let changed = crate::read_text_at::<LooseSpec>(
        "kind='example'\nschema=2\nlabel=false\n",
        &dir,
        &FieldFlags::default(),
    )
    .unwrap();
    assert_eq!(
        changed.label.0, "false",
        "the mutation genuinely loosens the owning reader"
    );
    let problems = check_type_refusals::<LooseSpec>();
    assert!(
        problems
            .iter()
            .any(|p| p.contains("label") && p.contains("accepted") && p.contains("boolean")),
        "{problems:?}"
    );
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Entry {
    label: String,
}
fn entries() -> Vec<Entry> {
    vec![
        Entry {
            label: "one".into(),
        },
        Entry {
            label: "two".into(),
        },
    ]
}
fn mapped() -> std::collections::BTreeMap<String, Entry> {
    [(
        "a/b~c".into(),
        Entry {
            label: "mapped".into(),
        },
    )]
    .into()
}
fn optional() -> Option<Entry> {
    Some(Entry {
        label: "optional".into(),
    })
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
enum Mixed {
    Text(String),
    Number(u64),
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct StrictSpec {
    label: String,
    enabled: bool,
    #[serde(default)]
    count: u64,
    weight: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[schemars(example = &"")]
    unretained: String,
    #[schemars(example = entries())]
    rows: Vec<Entry>,
    #[schemars(example = mapped())]
    mapped: std::collections::BTreeMap<String, Entry>,
    #[schemars(example = optional())]
    optional: Option<Entry>,
    #[schemars(example = &vec!["tag"])]
    tags: Vec<String>,
    #[schemars(example = &serde_json::json!("anything"))]
    open: serde_json::Value,
    #[schemars(example = &"active")]
    mixed: Mixed,
}
impl crate::Specification for StrictSpec {
    fn format() -> &'static crate::schema::Format {
        <LooseSpec as crate::Specification>::format()
    }
}
impl Kind for StrictSpec {}

#[test]
fn only_retained_primitive_fields_gain_exact_concrete_witnesses() {
    let report = type_refusals::<StrictSpec>().unwrap();
    let found: Vec<_> = report
        .witnessed
        .iter()
        .map(|w| {
            (
                w.field.as_str(),
                w.pointer.as_str(),
                w.declared.as_str(),
                w.rejected.as_str(),
            )
        })
        .collect();
    for expected in [
        ("label", "/label", "string", "boolean"),
        ("enabled", "/enabled", "boolean", "integer"),
        ("count", "/count", "integer", "boolean"),
        ("weight", "/weight", "number", "boolean"),
        ("rows.label", "/rows/0/label", "string", "boolean"),
        ("rows.label", "/rows/1/label", "string", "boolean"),
        (
            "mapped.*.label",
            "/mapped/a~1b~0c/label",
            "string",
            "boolean",
        ),
        ("optional.label", "/optional/label", "string", "boolean"),
    ] {
        assert!(found.contains(&expected), "missing {expected:?}: {found:?}");
    }
    assert!(report.witnessed.iter().all(|witness| witness.counterexample
        == if witness.declared == "boolean" {
            "0"
        } else {
            "false"
        }));
    assert_eq!(
        found.len(),
        8,
        "no omitted/mixed/open/list-item type proof: {found:?}"
    );
    for field in ["unretained", "open", "mixed", "tags"] {
        assert!(
            report.excluded.iter().any(|skip| skip.field == field),
            "{field}: {report:?}"
        );
    }
    assert!(check_type_refusals::<StrictSpec>().is_empty());
}

fn no_count(spec: &StrictSpec) -> anyhow::Result<Vec<String>> {
    let mut words = crate::flags_of(spec)?;
    let index = words.iter().position(|word| word == "--count").unwrap();
    words.drain(index..index + 2);
    Ok(words)
}
#[test]
fn a_default_value_that_roundtrips_without_a_flag_is_not_a_retained_flag_witness() {
    let report = checked_with::<StrictSpec>(no_count).unwrap();
    assert!(!report.witnessed.iter().any(|w| w.field == "count"));
    assert!(
        report
            .excluded
            .iter()
            .any(|skip| skip.field == "count" && skip.reason.contains("flag table"))
    );
}
fn changed_count(spec: &StrictSpec) -> anyhow::Result<Vec<String>> {
    let mut words = crate::flags_of(spec)?;
    let index = words.iter().position(|word| word == "--count").unwrap();
    words[index + 1] = "99".into();
    Ok(words)
}
#[test]
fn a_changed_file_flag_baseline_is_refused_before_any_negative_proof() {
    let error = checked_with::<StrictSpec>(changed_count).unwrap_err();
    assert!(
        error.to_string().contains("file and flag baselines differ"),
        "{error:#}"
    );
}

#[derive(serde::Serialize)]
#[serde(transparent)]
struct LooseObject(Entry);
impl<'de> serde::Deserialize<'de> for LooseObject {
    fn deserialize<D: serde::Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Input {
            Object(Entry),
            Boolean(bool),
        }
        Ok(Self(match Input::deserialize(reader)? {
            Input::Object(entry) => entry,
            Input::Boolean(value) => Entry {
                label: value.to_string(),
            },
        }))
    }
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct LooseContainerSpec {
    #[schemars(with = "Entry", example = &serde_json::json!({"label":"example"}))]
    node: LooseObject,
}
impl crate::Specification for LooseContainerSpec {
    fn format() -> &'static crate::schema::Format {
        <LooseSpec as crate::Specification>::format()
    }
}
impl Kind for LooseContainerSpec {}

#[test]
fn a_permissive_container_reader_is_not_proved_by_retained_leaf_checks() {
    assert!(crate::check_shape::<LooseContainerSpec>().is_empty());
    assert!(crate::check_roundtrip::<LooseContainerSpec>().is_empty());
    assert!(check_type_refusals::<LooseContainerSpec>().is_empty());
    let changed = crate::read_text_at::<LooseContainerSpec>(
        "kind='example'\nschema=2\nnode=false\n",
        &std::env::current_dir().unwrap(),
        &FieldFlags::default(),
    )
    .unwrap();
    assert_eq!(changed.node.0.label, "false");
    let problems = check_container_type_refusals::<LooseContainerSpec>();
    assert!(
        problems
            .iter()
            .any(|p| p.contains("node") && p.contains("accepted") && p.contains("boolean")),
        "{problems:?}"
    );
}

#[test]
fn only_actual_retained_array_and_object_fields_gain_container_witnesses() {
    let report = container_type_refusals::<StrictSpec>().unwrap();
    let found: Vec<_> = report
        .witnessed
        .iter()
        .map(|w| (w.field.as_str(), w.pointer.as_str(), w.declared.as_str()))
        .collect();
    assert_eq!(found.len(), 5, "{report:?}");
    for expected in [
        ("rows", "/rows", "array"),
        ("mapped", "/mapped", "object"),
        ("mapped.*", "/mapped/a~1b~0c", "object"),
        ("optional", "/optional", "object"),
        ("tags", "/tags", "array"),
    ] {
        assert!(found.contains(&expected), "{found:?}");
    }
    assert!(report.witnessed.iter().all(|w| w.counterexample == "false"));
    assert!(
        !report
            .witnessed
            .iter()
            .any(|w| w.pointer == "/rows/0" || w.pointer == "/rows/1")
    );
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct DefaultContainerSpec {
    #[serde(default = "entries")]
    #[schemars(example = entries())]
    rows: Vec<Entry>,
}
impl crate::Specification for DefaultContainerSpec {
    fn format() -> &'static crate::schema::Format {
        <LooseSpec as crate::Specification>::format()
    }
}
impl Kind for DefaultContainerSpec {}
fn no_rows(_: &DefaultContainerSpec) -> anyhow::Result<Vec<String>> {
    Ok(Vec::new())
}
#[test]
fn a_container_needs_no_independent_flag_when_the_actual_file_reader_refuses_it() {
    assert!(baseline::<DefaultContainerSpec>(no_rows).is_ok());
    let report = checked_in::<DefaultContainerSpec>(no_rows, active::Scope::Container).unwrap();
    assert_eq!(report.witnessed.len(), 1, "{report:?}");
    assert_eq!(report.witnessed[0].pointer, "/rows");
    assert_eq!(report.witnessed[0].declared, "array");
}

#[test]
fn required_scalar_omissions_are_concrete_and_defaulted_fields_are_excluded() {
    let report = required_refusals::<StrictSpec>().unwrap();
    assert_eq!(report.witnessed.len(), 7, "{report:?}");
    for path in [
        "/label",
        "/enabled",
        "/weight",
        "/rows/0/label",
        "/rows/1/label",
        "/mapped/a~1b~0c/label",
        "/optional/label",
    ] {
        assert!(
            report.witnessed.iter().any(|w| w.pointer == path),
            "{report:?}"
        );
    }
    assert!(!report.witnessed.iter().any(|w| w.field == "count"));
    assert!(
        report
            .excluded
            .iter()
            .any(|w| w.field == "count" && w.reason.contains("no independent"))
    );
}
