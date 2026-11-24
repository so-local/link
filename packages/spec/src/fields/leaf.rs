//! Explicit declared leaf controls and unconstrained intrinsic kinds only.
use super::{At, Value, closed};
pub(super) fn plain(node: &Value) -> bool {
    node.as_object().is_some_and(|object| {
        object.iter().all(|(key, value)| match key.as_str() {
            "$ref"
            | "type"
            | "anyOf"
            | "oneOf"
            | "allOf"
            | "enum"
            | "const"
            | "description"
            | "title"
            | "default"
            | "examples"
            | "$comment"
            | "properties"
            | "items"
            | "additionalProperties" => true,
            "minimum" => value.as_u64() == Some(0),
            "format" => matches!(
                value.as_str(),
                Some("uint8" | "uint16" | "uint32" | "uint64" | "uint128")
            ),
            "required" => value.as_array().is_some_and(Vec::is_empty),
            _ => false,
        })
    })
}
impl At<'_> {
    /// A declaration supplies each explicit control. Additional unknown bounds
    /// or conditions prohibit synthesizing even an intrinsic empty/one value.
    pub(crate) fn leaf_control(&self, kind: &str) -> Option<(toml::Value, &'static str)> {
        let matching = |value: &Value| {
            toml::Value::try_from(value).ok().filter(|v| match kind {
                "string" => v.is_str(),
                "boolean" => v.is_bool(),
                "integer" => v.is_integer(),
                "number" => v.is_integer() || v.is_float(),
                "array" => v.is_array(),
                "object" => v.is_table(),
                _ => false,
            })
        };
        if let Some(value) = self.example_value().and_then(matching) {
            return Some((value, "declared example"));
        }
        if let Some(value) = self.default_value().and_then(matching) {
            return Some((value, "declared default given explicitly"));
        }
        if !self.leaf_plain || self.nodes.len() != 1 {
            return None;
        }
        // An empty/unrepresentable closed domain must not fall back to a
        // generic value, and several composed domains need owner examples.
        if let Some(values) = closed(self.nodes[0]) {
            return values
                .into_iter()
                .find_map(matching)
                .map(|value| (value, "plain declared finite control"));
        }
        let value = self.first_value().or_else(|| match kind {
            "array" => Some(toml::Value::Array(Vec::new())),
            "object" => Some(toml::Value::Table(toml::Table::new())),
            _ => None,
        })?;
        Some((value, "plain declared kind control"))
    }
}
