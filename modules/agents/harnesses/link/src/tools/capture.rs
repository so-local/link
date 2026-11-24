//! Bounded captured replies, separate from their model-facing text.

use super::*;
use link_harness_wire::evidence::{
    Captured, Content, ModelView, Outcome, RemoteError, Reply, ReplyBody,
};
use std::collections::BTreeMap;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Limits {
    reply_bytes: usize,
    request_bytes: usize,
    whole_every: u32,
}

static LIMITS: std::sync::LazyLock<Limits> = std::sync::LazyLock::new(|| {
    let limits: Limits =
        toml::from_str(include_str!("capture.toml")).expect("reply capture limits");
    assert!(limits.reply_bytes > 0 && (limits.reply_bytes as u64) < crate::store::EVENT_MAX);
    assert!(limits.request_bytes > 0 && (limits.request_bytes as u64) < crate::store::EVENT_MAX);
    assert!(limits.whole_every > 0);
    limits
});

/// How many model requests in a row may each record only what changed
/// since the one before: the next is recorded whole.
pub(crate) fn whole_every() -> u32 {
    LIMITS.whole_every
}

pub(crate) fn request_projection(
    projection: link_harness_wire::evidence::request::RequestProjection,
) -> Captured<link_harness_wire::evidence::request::RequestProjection> {
    let bytes = serde_json::to_vec(&projection).expect("request projection JSON");
    if bytes.len() > LIMITS.request_bytes {
        omitted(&bytes, "model request projection byte limit")
    } else {
        Captured::Retained { value: projection }
    }
}

fn omitted<T>(bytes: &[u8], reason: &str) -> Captured<T> {
    Captured::Omitted {
        bytes: bytes.len() as u64,
        sha256: link_digest::sha256(bytes),
        reason: reason.into(),
    }
}

fn value(value: &Value, left: &mut usize) -> Captured<Value> {
    let bytes = serde_json::to_vec(value).expect("JSON value serializes");
    if bytes.len() > *left {
        return omitted(&bytes, "reply capture byte limit");
    }
    *left -= bytes.len();
    Captured::Retained {
        value: value.clone(),
    }
}

pub(crate) fn dispatch(request: Dispatch<'_>) -> link_harness_wire::evidence::McpDispatch {
    let mut left = LIMITS.reply_bytes;
    link_harness_wire::evidence::McpDispatch {
        server: request.server.into(),
        operation: request.operation.into(),
        arguments: value(request.arguments, &mut left),
    }
}

pub(crate) fn captured(
    answer: &Answered,
    text: &str,
    parts: &[crate::event::Part],
    media: &BTreeMap<usize, Result<crate::event::Part, String>>,
) -> Outcome {
    let mut left = LIMITS.reply_bytes;
    match &answer.observation {
        Observation::Refused => Outcome::Refused {
            reason: answer.text.clone(),
        },
        Observation::Unknown => Outcome::Unknown {
            reason: answer.text.clone(),
        },
        Observation::RemoteError {
            code,
            message,
            data,
        } => {
            let captured_message = if message.len() > left {
                omitted(message.as_bytes(), "reply capture byte limit")
            } else {
                left -= message.len();
                Captured::Retained {
                    value: message.clone(),
                }
            };
            Outcome::RemoteError {
                error: RemoteError {
                    code: *code,
                    message: captured_message,
                    data: data.as_ref().map(|data| value(data, &mut left)),
                },
            }
        }
        Observation::Reply(result) => {
            let fields = result
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(name, _)| name.as_str() != "content")
                .map(|(name, field)| {
                    let captured = if name == "_meta" {
                        omitted(
                            &serde_json::to_vec(field).expect("JSON metadata"),
                            "unrestricted reply metadata is not retained",
                        )
                    } else {
                        value(field, &mut left)
                    };
                    (name.clone(), captured)
                })
                .collect();
            let mut omissions = BTreeMap::<&str, usize>::new();
            let content = result["content"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(index, block)| {
                    let binary =
                        block["type"] == "image" || block["resource"].get("blob").is_some();
                    let supported_text =
                        block["type"] == "text" || block["resource"]["text"].is_string();
                    let part = media.get(&index).and_then(|p| p.as_ref().ok()).cloned();
                    let omission = media
                        .get(&index)
                        .and_then(|p| p.as_ref().err())
                        .cloned()
                        .or_else(|| answer.media_omissions.get(&index).cloned())
                        .or_else(|| {
                            (!binary && !supported_text).then(|| "unsupported content kind".into())
                        });
                    if omission.is_some() {
                        let kind = if media.get(&index).is_some_and(Result::is_err) {
                            "media storage omissions"
                        } else if answer.media_omissions.contains_key(&index) {
                            "media decoding or limit omissions"
                        } else {
                            "unsupported content omissions"
                        };
                        *omissions.entry(kind).or_default() += 1;
                    }
                    let captured = if binary || !supported_text {
                        omitted(
                            &serde_json::to_vec(block).expect("JSON block"),
                            if binary {
                                "binary content uses stored media references"
                            } else {
                                "unsupported content kind"
                            },
                        )
                    } else {
                        value(block, &mut left)
                    };
                    Content {
                        index: index as u32,
                        value: captured,
                        media: part,
                        omission,
                    }
                })
                .collect();
            let captured_body = ReplyBody { fields, content };
            let body = if serde_json::to_vec(&captured_body)
                .expect("captured reply JSON")
                .len()
                > LIMITS.reply_bytes
            {
                omitted(
                    &serde_json::to_vec(result).expect("original reply JSON"),
                    "aggregate reply capture byte limit",
                )
            } else {
                Captured::Retained {
                    value: captured_body,
                }
            };
            Outcome::Completed {
                reply: Reply {
                    is_error: answer.failed,
                    body,
                    model: ModelView {
                        text_sha256: link_digest::sha256(text.as_bytes()),
                        text_bytes: text.len() as u64,
                        original_text_bytes: answer.original_text_bytes as u64,
                        cut_text_bytes: answer.cut_text_bytes as u64,
                        media: parts.to_vec(),
                        omissions: omissions
                            .into_iter()
                            .map(|(kind, count)| format!("{count} {kind}"))
                            .collect(),
                    },
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_does_not_hide_structured_content_and_clipping_is_separate() {
        let answer = answered(
            &json!({"content":[{"type":"text","text":"abcdefgh"}],"structuredContent":{"exit":7}}),
            3,
        );
        let Outcome::Completed { reply } = captured(&answer, &answer.text, &[], &BTreeMap::new())
        else {
            panic!("no completed reply")
        };
        assert_eq!(
            reply
                .field("structuredContent")
                .unwrap()
                .retained()
                .unwrap()["exit"],
            7
        );
        assert_eq!(reply.model.original_text_bytes, 8);
        assert_eq!(reply.model.cut_text_bytes, 5);
        reply.validate().unwrap();
    }

    #[test]
    fn omitted_replies_and_metadata_never_look_retained() {
        let result = json!({"content":[],"structuredContent":{"large":"x".repeat(LIMITS.reply_bytes + 1)},"_meta":{"credential":"never persisted"}});
        let answer = answered(&result, 2);
        let Outcome::Completed { reply } = captured(&answer, &answer.text, &[], &BTreeMap::new())
        else {
            panic!("no completed reply")
        };
        assert!(
            reply
                .field("structuredContent")
                .unwrap()
                .retained()
                .is_none()
        );
        assert!(reply.field("_meta").unwrap().retained().is_none());
        assert!(
            !serde_json::to_string(&reply)
                .unwrap()
                .contains("never persisted")
        );
    }

    #[test]
    fn aggregate_content_metadata_is_bounded_without_dropping_the_model_view() {
        let result =
            json!({"content":(0..20_000).map(|_| json!({"type":"unknown"})).collect::<Vec<_>>()});
        let answer = answered(&result, 100);
        let Outcome::Completed { reply } = captured(&answer, &answer.text, &[], &BTreeMap::new())
        else {
            panic!("missing reply")
        };
        assert!(matches!(reply.body, Captured::Omitted { .. }));
        assert_eq!(
            reply.model.text_sha256,
            link_digest::sha256(answer.text.as_bytes())
        );
        assert_eq!(
            reply.model.omissions,
            ["20000 unsupported content omissions"]
        );
        assert!(serde_json::to_vec(&reply).unwrap().len() < LIMITS.reply_bytes);
        reply.validate().unwrap();
    }

    #[test]
    fn oversized_projection_is_an_omission_instead_of_rejecting_a_model_attempt() {
        use link_harness_wire::evidence::request::*;
        let projection = RequestProjection::Whole(Projected {
            messages: (0..10_000)
                .map(|index| RequestMessage {
                    role: if index % 2 == 0 { "user" } else { "assistant" }.into(),
                    text: link_files::reading::ContentDigest::of(b"body"),
                    answers: vec![],
                    calls: vec![],
                    media: vec![],
                })
                .collect(),
            sources: vec![],
        });
        let captured = request_projection(projection);
        assert!(matches!(captured, Captured::Omitted { .. }));
        assert!(serde_json::to_vec(&captured).unwrap().len() < LIMITS.request_bytes);
        captured.validate().unwrap();
    }
}
