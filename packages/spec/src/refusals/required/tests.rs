use super::*;
fn require_label(schema: &mut schemars::Schema) {
    schema
        .as_object_mut()
        .unwrap()
        .insert("required".into(), serde_json::json!(["label"]));
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = require_label)]
struct LostRequired {
    #[serde(default)]
    #[schemars(example = &"baseline")]
    label: String,
}
impl crate::Specification for LostRequired {
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
impl Kind for LostRequired {}
#[test]
fn a_reader_default_cannot_waive_the_independent_required_declaration() {
    assert_eq!(
        crate::schema::<LostRequired>()["required"],
        serde_json::json!(["label"])
    );
    assert!(crate::check_roundtrip::<LostRequired>().is_empty());
    assert!(crate::check_type_refusals::<LostRequired>().is_empty());
    let changed = crate::read_text_at::<LostRequired>(
        "kind='example'\nschema=2\n",
        &std::env::current_dir().unwrap(),
        &FieldFlags::default(),
    )
    .unwrap();
    assert_eq!(
        changed.label, "",
        "the mutated reader genuinely accepts omission"
    );
    let problems = check_required_refusals::<LostRequired>();
    assert!(
        problems
            .iter()
            .any(|p| p.contains("label") && p.contains("accepted omission")),
        "{problems:?}"
    );
}
