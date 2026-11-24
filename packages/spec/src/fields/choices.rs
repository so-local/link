//! Conservative finite scalar domains for owning-reader checks.
use super::{At, Value, closed};

pub(super) fn plain(node: &Value) -> bool {
    node.as_object().is_some_and(|object| {
        object.keys().all(|key| {
            matches!(
                key.as_str(),
                "$ref"
                    | "anyOf"
                    | "oneOf"
                    | "allOf"
                    | "type"
                    | "enum"
                    | "const"
                    | "description"
                    | "title"
                    | "default"
                    | "examples"
                    | "$comment"
            )
        })
    })
}

impl At<'_> {
    /// Only one unconstrained scalar domain, not a union of alternatives or
    /// finite candidates with additional validation still to be reviewed.
    pub(crate) fn scalar_choices(&self) -> Option<Vec<Value>> {
        if !self.plain_choices || self.nodes.len() != 1 {
            return None;
        }
        let types = self.types()?;
        if types.len() != 1 {
            return None;
        }
        let kind = *types.first()?;
        if !matches!(kind, "string" | "boolean") {
            return None;
        }
        let values: Vec<Value> = match closed(self.nodes[0]) {
            Some(values) => values.into_iter().cloned().collect(),
            None if kind == "boolean" => vec![false.into(), true.into()],
            None => return None,
        };
        (!values.is_empty()
            && values.iter().all(|value| match kind {
                "string" => value.is_string(),
                _ => value.is_boolean(),
            }))
        .then_some(values)
    }
}
