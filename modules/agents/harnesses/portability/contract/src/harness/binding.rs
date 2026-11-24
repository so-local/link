//! Binding a harness started blank ([`Contract::blank`]) to one agent.

use super::*;

/// What binds a harness started blank ([`Contract::blank`]) to one agent:
/// one line of JSON on its control socket. Its `Debug` never shows the
/// key, nor the settings it came with.
#[derive(Duplicate, PartialEq, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Bind {
    /// Who it's bound to.
    pub agent: BindAgent,
    /// The key made for this binding alone: its endpoint's (the bearer it
    /// takes, and what it proves itself with, [`endpoint_proof`]), and
    /// the agent's on every bridge call ([`Bind::binding`]). It stands for
    /// [`Settings::endpoint_key`], whatever that says.
    #[serde(with = "link_secretive::bare")]
    #[duplicate]
    pub key: Secret,
    /// The agent's settings, as `configure` takes them, with a
    /// placeholder for every secret (the model's key, a connection's
    /// token): the harness keeps them in memory, and writes no file.
    #[duplicate]
    pub settings: Settings,
    /// Where its store bridge listens, when there is one: the socket
    /// [`Settings::store`] names for the driver its spec keeps its
    /// conversations with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<PathBuf>,
    /// Where its files bridge listens, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<PathBuf>,
}

/// The agent a [`Bind`] binds: its name, ID, the service it acts as, its
/// owner, and the teams it's on, by their IDs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindAgent {
    pub name: String,
    pub id: String,
    /// The service it acts as, its bare ID: what its bridge calls prove
    /// beside its ID.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub service: String,
    pub owner: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub teams: Vec<String>,
}

/// What a harness started blank answers a [`Bind`] with: `{}` once it
/// answers as the agent, else why not.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Bound {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The most a [`Bind`] line is: settings with their spec, many times over.
pub const BIND_MAX: u64 = 4 << 20;

impl Bind {
    /// What it proves its bridge calls with: the agent, and the key.
    pub fn binding(&self) -> crate::bridge::Binding {
        crate::bridge::Binding {
            identity: crate::bridge::Identity {
                agent: self.agent.id.clone(),
                service: self.agent.service.clone(),
                owner: self.agent.owner.clone(),
                teams: self.agent.teams.clone(),
            },
            key: self.key.duplicate(),
        }
    }
}

/// Binds the harness started blank whose control socket is `control` with
/// `bind`, and waits for its answer: an error when it refused.
pub fn bind(control: &std::path::Path, bind: &Bind) -> anyhow::Result<()> {
    use std::io::{BufRead, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(control).map_err(|err| {
        anyhow::anyhow!(
            "nothing listens at {} ({err}): no harness started blank there",
            control.display()
        )
    })?;
    stream.write_all(format!("{}\n", serde_json::to_string(bind)?).as_bytes())?;
    let mut line = String::new();
    std::io::BufReader::new(std::io::Read::take(&stream, 1 << 16)).read_line(&mut line)?;
    let bound: Bound = serde_json::from_str(&line)
        .map_err(|err| anyhow::anyhow!("reading the harness's answer to its binding: {err}"))?;
    if let Some(error) = bound.error {
        anyhow::bail!(
            "the harness refused its binding: {}",
            link_format::clean(&error)
        );
    }
    Ok(())
}
