//! Real TOML input at the owning field seed; surrounding completion supplies no proof.
use super::*;
use crate::probe::{LeafRead, Request, RequestKind};

#[derive(Debug, serde::Serialize)]
pub struct StructuralType {
    pub field: String,
    pub declared: String,
    pub request: Request,
    pub control: serde_json::Value,
    pub control_source: String,
    pub counterexample: String,
}
#[derive(Debug, serde::Serialize)]
pub struct StructuralExclusion {
    pub field: String,
    pub request: Request,
    pub reason: String,
}
#[derive(Debug, Default, serde::Serialize)]
pub struct StructuralTypes {
    pub witnessed: Vec<StructuralType>,
    pub excluded: Vec<StructuralExclusion>,
}
/// Problems refusing contrasting TOML kinds at the actual owning field reader.
pub fn check_structural_types<T: Kind>() -> Vec<String> {
    structural_types::<T>().err().map_or_else(Vec::new, |e| {
        vec![format!("spec structural field type: {e:#}")]
    })
}
/// Independent declared controls must be accepted before a wrong-kind refusal
/// supplies a structural leaf proof. No surrounding Kind/check/default claim.
pub fn structural_types<T: Kind>() -> anyhow::Result<StructuralTypes> {
    let found = crate::probe::discover::<T>(
        &std::collections::BTreeMap::new(),
        crate::probe::Expand::Everywhere,
    )
    .map_err(anyhow::Error::msg)?;
    let schema = crate::schema::<T>();
    let mut report = StructuralTypes::default();
    for (field, request) in found.requests {
        if field.is_empty() {
            continue;
        }
        let reason = if request.opaque_ancestor || request.kind == RequestKind::Opaque {
            Some("custom deserialize_any traversal remains opaque")
        } else if !request.enum_route.is_empty() {
            Some("an enum route needs independent branch association")
        } else {
            None
        };
        if let Some(reason) = reason {
            exclude(&mut report, field, request, reason.into());
            continue;
        }
        let mut at = Some(crate::fields::At::top(&schema));
        for key in field.split('.') {
            at = at.and_then(|at| at.child(key));
        }
        let Some(at) = at else {
            exclude(
                &mut report,
                field,
                request,
                "no unambiguous declared path".into(),
            );
            continue;
        };
        let kinds = at.types();
        let kind = kinds
            .as_ref()
            .filter(|k| k.len() == 1)
            .and_then(|k| k.first())
            .copied();
        let Some(kind) = kind.filter(|kind| {
            matches!(
                (request.kind, *kind),
                (RequestKind::Boolean, "boolean")
                    | (RequestKind::Integer, "integer")
                    | (RequestKind::Number, "number")
                    | (RequestKind::String | RequestKind::Enum, "string")
                    | (RequestKind::Array, "array")
                    | (RequestKind::Object | RequestKind::Enum, "object")
            )
        }) else {
            exclude(
                &mut report,
                field,
                request,
                "declared kinds are mixed, open or unlike the actual request".into(),
            );
            continue;
        };
        let Some((control, source)) = at.leaf_control(kind) else {
            exclude(
                &mut report,
                field,
                request,
                "no independently declared leaf control".into(),
            );
            continue;
        };
        match crate::probe::read_strict_at::<T>(&field, &control) {
            LeafRead::Accepted => {}
            other => {
                exclude(
                    &mut report,
                    field,
                    request,
                    format!("declared control is not an accepted reachable leaf: {other:?}"),
                );
                continue;
            }
        }
        let wrong = if kind == "boolean" {
            toml::Value::Integer(0)
        } else {
            toml::Value::Boolean(false)
        };
        match crate::probe::read_strict_at::<T>(&field, &wrong) {
            LeafRead::Refused(_) => {}
            LeafRead::Unreached(reason) => {
                exclude(&mut report, field, request, reason);
                continue;
            }
            LeafRead::Accepted => anyhow::bail!(
                "{field}: the owning {} seed accepted contrasting TOML {wrong} for declared {kind}",
                request.method
            ),
        }
        report.witnessed.push(StructuralType {
            field,
            declared: kind.into(),
            request,
            control: serde_json::to_value(control)?,
            control_source: source.into(),
            counterexample: wrong.to_string(),
        });
    }
    Ok(report)
}
fn exclude(report: &mut StructuralTypes, field: String, request: Request, reason: String) {
    report.excluded.push(StructuralExclusion {
        field,
        request,
        reason,
    });
}

#[cfg(test)]
mod tests;
