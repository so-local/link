//! The typed call every surface makes into an action's implementation,
//! and the outcome it gets back: one shape for a command line, an HTTP
//! request or a private process call.

use serde::{Deserialize, Serialize};

use crate::ActionId;

/// One call of an action: which, its runtime arguments, the spec it
/// reads and who asks. The caller is the principal IAM checked, never one
/// the arguments name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Call<A, S = ()> {
    pub action: ActionId,
    pub args: A,
    /// The spec it reads, when its action binds one.
    #[serde(default)]
    pub spec: S,
    pub caller: String,
    /// The resource the call acts on, as its arguments name it; none for
    /// an action on its project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
}

impl<A, S> Call<A, S> {
    /// The same call carrying `args` and `spec` instead, as a typed
    /// reader makes it.
    pub fn with<B, T>(self, args: B, spec: T) -> Call<B, T> {
        Call {
            action: self.action,
            args,
            spec,
            caller: self.caller,
            resource: self.resource,
        }
    }
}

/// How a call ended. Each keeps its meaning on every surface, including
/// a failure after output began.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "lowercase", deny_unknown_fields)]
pub enum Outcome<O> {
    /// Done, with its output.
    Completed { output: O },
    /// Its caller, asked, said no.
    Declined { reason: String },
    /// Its input or state rules it out.
    Refused { reason: String },
    /// It went wrong.
    Failed { reason: String },
    /// It was stopped before it ended: cancelled or out of time.
    Interrupted { reason: String },
}

impl<O> Outcome<O> {
    pub fn map<P>(self, f: impl FnOnce(O) -> P) -> Outcome<P> {
        match self {
            Self::Completed { output } => Outcome::Completed { output: f(output) },
            Self::Declined { reason } => Outcome::Declined { reason },
            Self::Refused { reason } => Outcome::Refused { reason },
            Self::Failed { reason } => Outcome::Failed { reason },
            Self::Interrupted { reason } => Outcome::Interrupted { reason },
        }
    }
}

/// The most a call or its outcome may be, in bytes, between a host and a
/// namespace program.
pub const MAX_BYTES: u64 = 16 << 20;

/// What a namespace program does with the one call on its standard
/// input: `dispatch` runs it, and the outcome goes to its standard
/// output. Both are private pipes its host holds.
pub fn answer_one(
    dispatch: impl FnOnce(Call<serde_json::Value, serde_json::Value>) -> Outcome<serde_json::Value>,
) -> anyhow::Result<()> {
    use std::io::{Read, Write};
    let mut input = Vec::new();
    std::io::stdin()
        .take(MAX_BYTES + 1)
        .read_to_end(&mut input)?;
    anyhow::ensure!(
        input.len() as u64 <= MAX_BYTES,
        "a call over {MAX_BYTES} bytes"
    );
    let call: Call<serde_json::Value, serde_json::Value> = serde_json::from_slice(&input)?;
    let outcome = dispatch(call);
    let mut stdout = link_format::streams::protocol();
    serde_json::to_writer(&mut stdout, &outcome)?;
    stdout.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outcome_is_tagged_by_how_it_ended() {
        let done: Outcome<u8> = Outcome::Completed { output: 4 };
        let json = serde_json::to_string(&done).unwrap();
        assert_eq!(json, r#"{"outcome":"completed","output":4}"#);
        assert_eq!(serde_json::from_str::<Outcome<u8>>(&json).unwrap(), done);
        let refused: Outcome<u8> =
            serde_json::from_str(r#"{"outcome":"refused","reason":"no"}"#).unwrap();
        assert_eq!(
            refused.map(|n| n + 1),
            Outcome::Refused {
                reason: "no".into()
            }
        );
        assert!(serde_json::from_str::<Outcome<u8>>(r#"{"outcome":"done"}"#).is_err());
    }
}
