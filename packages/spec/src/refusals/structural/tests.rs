use super::*;
#[derive(serde::Serialize)]
#[serde(transparent)]
struct Loose(u64);
impl<'de> serde::Deserialize<'de> for Loose {
    fn deserialize<D: serde::Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = Loose;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an integer")
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Loose, E> {
                Ok(Loose(v as u64))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Loose, E> {
                Ok(Loose(v))
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Loose, E> {
                Ok(Loose(u64::from(v)))
            }
        }
        reader.deserialize_u64(Visitor)
    }
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Spec {
    #[schemars(with = "u64")]
    count: Loose,
}
impl crate::Specification for Spec {
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
impl Kind for Spec {}
#[test]
fn requesting_u64_does_not_prove_that_the_owning_visitor_refuses_a_boolean() {
    let found = crate::probe::discover::<Spec>(
        &std::collections::BTreeMap::new(),
        crate::probe::Expand::Everywhere,
    )
    .unwrap();
    assert_eq!(found.requests["count"].method, "deserialize_u64");
    assert_eq!(
        crate::probe::read_strict_at::<Spec>("count", &toml::Value::Integer(1)),
        LeafRead::Accepted
    );
    assert_eq!(
        crate::probe::read_strict_at::<Spec>("count", &toml::Value::Boolean(false)),
        LeafRead::Accepted
    );
    let problems = check_structural_types::<Spec>();
    assert!(
        problems
            .iter()
            .any(|p| p.contains("count") && p.contains("accepted contrasting TOML false")),
        "{problems:?}"
    );
}
#[derive(serde::Deserialize)]
struct List {
    #[expect(dead_code, reason = "only deserialized through its owning field seed")]
    values: Vec<bool>,
}
#[test]
fn the_existing_example_sequence_coercion_is_not_a_strict_type_refusal() {
    assert!(crate::probe::read_at::<List>("values", &serde_json::json!(false)).is_ok());
    assert_eq!(
        crate::probe::read_strict_at::<List>("values", &toml::Value::Array(vec![])),
        LeafRead::Accepted
    );
    assert!(matches!(
        crate::probe::read_strict_at::<List>("values", &toml::Value::Boolean(false)),
        LeafRead::Refused(_)
    ));
    assert!(matches!(
        crate::probe::read_strict_at::<List>("missing", &toml::Value::Boolean(false)),
        LeafRead::Unreached(_)
    ));
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Nested {
    label: String,
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Strict {
    on: bool,
    count: u64,
    nested: Option<Nested>,
    values: Vec<bool>,
    opaque: serde_json::Value,
}
impl crate::Specification for Strict {
    fn format() -> &'static crate::schema::Format {
        <Spec as crate::Specification>::format()
    }
}
impl Kind for Strict {}
#[test]
fn native_leaf_proofs_record_some_routes_and_leave_opaque_values_unproved() {
    let report = structural_types::<Strict>().unwrap();
    for field in ["on", "count", "nested.label", "values"] {
        assert!(
            report.witnessed.iter().any(|w| w.field == field),
            "{report:?}"
        );
    }
    assert!(
        report
            .excluded
            .iter()
            .any(|w| w.field == "opaque" && w.reason.contains("opaque")),
        "{report:?}"
    );
    assert!(
        report
            .excluded
            .iter()
            .any(|w| w.field == "nested" && w.reason.contains("control")),
        "required object must not get a synthesized complete specimen: {report:?}"
    );
    let nested = report
        .witnessed
        .iter()
        .find(|w| w.field == "nested.label")
        .unwrap();
    assert!(nested.request.some.contains("nested"));
    assert_eq!(nested.request.containers["nested"], RequestKind::Object);
    assert_eq!(
        report
            .witnessed
            .iter()
            .find(|w| w.field == "values")
            .unwrap()
            .request
            .kind,
        RequestKind::Array
    );
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Branch {
    One { label: String },
    Two { label: u64 },
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct Branches {
    branch: Branch,
}
impl crate::Specification for Branches {
    fn format() -> &'static crate::schema::Format {
        <Spec as crate::Specification>::format()
    }
}
impl Kind for Branches {}
#[test]
fn external_enum_routes_are_recorded_without_fanning_out_leaf_claims() {
    let found = crate::probe::discover::<Branches>(
        &std::collections::BTreeMap::new(),
        crate::probe::Expand::Everywhere,
    )
    .unwrap();
    assert_eq!(found.requests["branch.one.label"].enum_route, ["one"]);
    assert_eq!(found.requests["branch.two.label"].enum_route, ["two"]);
    let report = structural_types::<Branches>().unwrap();
    assert!(
        !report
            .witnessed
            .iter()
            .any(|w| !w.request.enum_route.is_empty())
    );
    assert!(
        report
            .excluded
            .iter()
            .any(|w| w.field == "branch.one.label" && w.reason.contains("branch association"))
    );
}

fn no_values(schema: &mut schemars::Schema) {
    schema
        .as_object_mut()
        .unwrap()
        .insert("enum".into(), serde_json::json!([]));
}
fn disjoint(schema: &mut schemars::Schema) {
    *schema =
        serde_json::json!({"allOf":[{"type":"integer","enum":[1]},{"type":"integer","enum":[2]}]})
            .try_into()
            .unwrap();
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct NoDeclaredControl {
    #[schemars(transform=no_values)]
    empty: u64,
    #[schemars(transform=disjoint)]
    composed: u64,
}
impl crate::Specification for NoDeclaredControl {
    fn format() -> &'static crate::schema::Format {
        <Spec as crate::Specification>::format()
    }
}
impl Kind for NoDeclaredControl {}
#[test]
fn native_reader_acceptance_cannot_invent_a_control_outside_the_declaration() {
    assert_eq!(
        crate::probe::read_strict_at::<NoDeclaredControl>("empty", &toml::Value::Integer(1)),
        LeafRead::Accepted
    );
    assert_eq!(
        crate::probe::read_strict_at::<NoDeclaredControl>("composed", &toml::Value::Integer(1)),
        LeafRead::Accepted
    );
    let report = structural_types::<NoDeclaredControl>().unwrap();
    assert!(report.witnessed.is_empty(), "{report:?}");
    for field in ["empty", "composed"] {
        assert!(
            report
                .excluded
                .iter()
                .any(|w| w.field == field && w.reason.contains("control")),
            "{report:?}"
        );
    }
}
