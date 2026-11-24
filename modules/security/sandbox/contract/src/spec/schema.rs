//! Schema and documentation from the policy type.
use super::*;

/// A section of a spec, as the docs and `lnk sandbox permission list` show
/// it: its name (none for the keys at the top), what it opens, and its
/// keys, each with what it takes. Made from [`Spec`]'s own comments.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub name: Option<String>,
    pub about: String,
    pub fields: Vec<(String, String)>,
}

impl Spec {
    /// The JSON Schema of a spec, made from this type: what an editor
    /// checks a spec's file with (`lnk spec show sandbox --json`).
    pub fn schema() -> serde_json::Value {
        <Self as link_spec::Specification>::format().json_schema(
            serde_json::to_value(schemars::schema_for!(Spec)).expect("a schema is JSON"),
        )
    }

    /// Its sections, the keys at the top first, each section's keys but
    /// its OS tables, by name.
    pub fn sections() -> Vec<Section> {
        let schema = Spec::schema();
        let definitions = &schema["$defs"];
        let words = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
        let about = |v: &serde_json::Value| words(v["description"].as_str().unwrap_or_default());
        // A key's first paragraph, for a table's cell: the rest is for an
        // editor reading the schema.
        let summary = |v: &serde_json::Value| {
            let text = v["description"].as_str().unwrap_or_default();
            words(text.split("\n\n").next().unwrap_or_default())
        };
        // A property's type by name, behind an `Option`'s `anyOf` or an
        // `allOf`.
        let named = |v: &serde_json::Value| -> Option<String> {
            let refs = std::iter::once(v)
                .chain(v["anyOf"].as_array().into_iter().flatten())
                .chain(v["allOf"].as_array().into_iter().flatten());
            refs.filter_map(|r| r["$ref"].as_str())
                .find_map(|r| r.strip_prefix("#/$defs/").map(String::from))
        };
        let mut top = Section {
            name: None,
            about: String::new(),
            fields: Vec::new(),
        };
        let mut sections = Vec::new();
        let properties = schema["properties"].as_object().into_iter().flatten();
        for (name, property) in properties {
            let Some(section) =
                named(property).filter(|n| definitions[n]["properties"].is_object())
            else {
                top.fields.push((name.clone(), summary(property)));
                continue;
            };
            let definition = &definitions[&section];
            let fields = definition["properties"].as_object().into_iter().flatten();
            sections.push(Section {
                name: Some(name.clone()),
                about: about(definition),
                fields: fields
                    .filter(|(key, _)| !matches!(key.as_str(), "macos" | "linux"))
                    .map(|(key, v)| (key.clone(), summary(v)))
                    .collect(),
            });
        }
        std::iter::once(top).chain(sections).collect()
    }
}
