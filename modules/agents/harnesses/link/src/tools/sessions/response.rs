//! A JSON-RPC response must answer this request, not merely look like
//! some response. The same validation applies to JSON and SSE.

use super::*;

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::tools) struct Refused {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&link_format::clean(&self.message))
    }
}

impl std::error::Error for Refused {}

pub(super) fn checked(answer: Value, wanted: &Value) -> anyhow::Result<Value> {
    let object = answer
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("a tool response isn't an object"))?;
    anyhow::ensure!(
        object.get("jsonrpc").and_then(Value::as_str) == Some("2.0"),
        "a tool response isn't JSON-RPC 2.0"
    );
    anyhow::ensure!(
        object.get("id") == Some(wanted),
        "a tool response has the wrong request ID"
    );
    match (object.get("result"), object.get("error")) {
        (Some(result), None) => Ok(result.clone()),
        (None, Some(error)) => Err(serde_json::from_value::<Refused>(error.clone())?.into()),
        _ => anyhow::bail!("a tool response must have exactly one result or error"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_response_matches_its_id_and_has_exactly_one_result_or_error() {
        let id = json!(7);
        assert_eq!(
            checked(json!({"jsonrpc":"2.0","id":7,"result":{}}), &id).unwrap(),
            json!({})
        );
        for bad in [
            json!({"jsonrpc":"2.0","id":8,"result":{}}),
            json!({"jsonrpc":"2.0","id":7}),
            json!({"jsonrpc":"2.0","id":7,"result":{},"error":{"code":1,"message":"no"}}),
            json!({"jsonrpc":"2.0","id":7,"error":{"message":"no code"}}),
            json!({"id":7,"result":{}}),
        ] {
            assert!(checked(bad, &id).is_err());
        }
        let refused = checked(json!({"jsonrpc":"2.0","id":7,"error":{"code":-32000,"message":"no","data":{"why":"policy"}}}), &id).unwrap_err();
        let refused = refused.downcast_ref::<Refused>().unwrap();
        assert_eq!(refused.code, -32000);
        assert_eq!(refused.data, Some(json!({"why":"policy"})));
    }
}
