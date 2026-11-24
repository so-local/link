use crate::{QuestionSet, ResponseCandidate, identity::*};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainDescriptor {
    pub id: DomainId,
    pub parent: Option<DomainId>,
    pub label: String,
    pub sources: Vec<SourceId>,
    pub authorization: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceCapabilities {
    pub confirmation: bool,
    pub text: bool,
    pub choice: bool,
    pub sets: bool,
    pub free_response: bool,
    pub max_review_bytes: usize,
    pub inline: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextAnchor {
    pub container: String,
    pub element: Option<String>,
    pub content_revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delivery {
    pub id: DeliveryId,
    pub request: RequestId,
    pub route: String,
    pub surface: String,
    pub message: Option<String>,
    pub anchor: Option<ContextAnchor>,
    pub delivered: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryEnvelope {
    pub question: QuestionSet,
    pub delivery: Delivery,
    /// Owner surfaces only. This value is never returned to a model/source.
    pub response_code: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InteractionPayload {
    Question(Box<QuestionSet>),
    Response(ResponseCandidate),
    Input(crate::control::UserInput),
    Control(crate::control::WorkControl),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub version: u32,
    pub payload: InteractionPayload,
}

impl<'de> Deserialize<'de> for Envelope {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            version: u32,
            payload: serde_json::Value,
        }
        // The version is read before the payload's shape.
        let wire = Wire::deserialize(deserializer)?;
        crate::PROTOCOL
            .read("the interaction's sender", wire.version)
            .map_err(serde::de::Error::custom)?;
        let payload =
            InteractionPayload::deserialize(wire.payload).map_err(serde::de::Error::custom)?;
        Ok(Self {
            version: wire.version,
            payload,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<crate::control::Receipt>,
    pub id: u64,
    pub request: Option<RequestId>,
    pub target: Option<WorkRef>,
    pub kind: String,
    pub at: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An envelope's version is read before its payload: an older or a
    /// newer one is refused by name, whatever the payload holds.
    #[test]
    fn an_envelope_is_refused_by_its_version_before_its_payload() {
        for (version, said) in [
            (
                crate::PROTOCOL.oldest - 1,
                "the oldest this release accepts",
            ),
            (crate::PROTOCOL.current + 1, "the newest this release reads"),
        ] {
            let wire = serde_json::json!({"version": version, "payload": {"kind": "unknown"}});
            let refused = serde_json::from_value::<Envelope>(wire).unwrap_err();
            assert!(refused.to_string().contains(said), "{refused}");
        }
    }
}
