//! A bound memory engine's authentication at a Unix embedding endpoint.
//! The receiver resolves the agent against the machine's live binding;
//! no owner is asserted by the engine.

use serde::{Deserialize, Serialize};

use crate::bridge::{Binding, proof, proof_matches};
use link_secretive::Secret;

const LABEL: &str = "memory embedding binding";
const SCHEME: &str = "Link-Binding ";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authentication {
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<Team>,
    #[serde(with = "link_secretive::bare")]
    pub proof: Secret,
}

/// The immutable team and the name resolved by its files bridge on each call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Team {
    pub id: String,
    pub named: crate::TeamRef,
}

impl Team {
    fn challenge(&self) -> anyhow::Result<String> {
        crate::bridge::check_agent(&self.id)?;
        crate::TeamRef::parse(&self.named.to_string())?;
        Ok(format!("{LABEL}:{}:{}", self.id, self.named))
    }
}

impl Authentication {
    pub fn new(binding: &Binding) -> anyhow::Result<Self> {
        crate::bridge::check_agent(&binding.identity.agent)?;
        crate::identity::check_owner(&binding.identity.owner)?;
        Ok(Self {
            agent: binding.identity.agent.clone(),
            team: None,
            proof: proof(&binding.key, LABEL, &binding.identity).into(),
        })
    }

    pub fn for_team(binding: &Binding, team: Team) -> anyhow::Result<Self> {
        let mut authentication = Self::new(binding)?;
        authentication.proof = proof(&binding.key, &team.challenge()?, &binding.identity).into();
        authentication.team = Some(team);
        Ok(authentication)
    }

    /// A credential for this binding and purpose alone, on a Unix socket.
    pub fn header(&self) -> Secret {
        self.proof.expose_map(|proof| {
            let base = format!("{SCHEME}{}:{proof}", self.agent);
            self.team.as_ref().map_or(base.clone(), |team| {
                format!("{base}:{}:{}", team.id, team.named)
            })
        })
    }

    pub fn from_header(header: &str) -> Option<Self> {
        let mut parts = header.strip_prefix(SCHEME)?.split(':');
        let (agent, proof) = (parts.next()?, parts.next()?);
        crate::bridge::check_agent(agent).ok()?;
        if proof.len() != 64 || !proof.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let team = match parts.next() {
            None => None,
            Some(id) => {
                let team = Team {
                    id: id.into(),
                    named: crate::TeamRef::parse(parts.next()?).ok()?,
                };
                team.challenge().ok()?;
                Some(team)
            }
        };
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            agent: agent.into(),
            team,
            proof: proof.to_string().into(),
        })
    }

    pub fn matches(&self, binding: &Binding) -> bool {
        let challenge = match &self.team {
            Some(team) => match team.challenge() {
                Ok(challenge) => challenge,
                Err(_) => return false,
            },
            None => LABEL.into(),
        };
        self.agent == binding.identity.agent
            && proof_matches(
                &binding.key,
                &challenge,
                &binding.identity,
                self.proof.expose(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::Identity;

    #[test]
    fn authentication_is_for_one_binding_and_purpose_and_redacts_its_proof() {
        let mut binding = Binding {
            identity: Identity {
                agent: "agent-a".into(),
                service: String::new(),
                owner: "acct-a".into(),
                teams: Vec::new(),
            },
            key: "the-binding-secret-0123456789abcdef".to_string().into(),
        };
        let authentication = Authentication::new(&binding).unwrap();
        let read = Authentication::from_header(authentication.header().expose()).unwrap();
        assert!(read.matches(&binding));
        let team = Team {
            id: "t4m9q2x7k1ab".into(),
            named: crate::TeamRef::parse("research").unwrap(),
        };
        let scoped = Authentication::for_team(&binding, team).unwrap();
        let mut scoped = Authentication::from_header(scoped.header().expose()).unwrap();
        assert!(scoped.matches(&binding));
        scoped.team.as_mut().unwrap().id = "z9y8x7w6v5u4".into();
        assert!(!scoped.matches(&binding));
        scoped.team.as_mut().unwrap().id = "t4m9q2x7k1ab".into();
        scoped.team.as_mut().unwrap().named = crate::TeamRef::parse("other").unwrap();
        assert!(!scoped.matches(&binding));
        scoped.team = None;
        assert!(!scoped.matches(&binding));
        let appended = format!("{}:t4m9q2x7k1ab:research", authentication.header().expose());
        assert!(
            !Authentication::from_header(&appended)
                .unwrap()
                .matches(&binding)
        );
        assert!(!format!("{authentication:?}").contains(authentication.proof.expose()));
        assert!(!format!("{:?}", authentication.header()).contains(authentication.proof.expose()));
        assert!(!proof_matches(
            &binding.key,
            "another purpose",
            &binding.identity,
            authentication.proof.expose()
        ));
        binding.identity.owner = "acct-b".into();
        assert!(!read.matches(&binding));
        binding.identity.owner = "acct-a".into();
        binding.key = "the-next-binding-secret-0123456789abcdef"
            .to_string()
            .into();
        assert!(!read.matches(&binding));
        binding.identity.agent = "agent\r\nx-link-owner: acct-b".into();
        assert!(Authentication::new(&binding).is_err());
        for malformed in [
            "",
            "Bearer token",
            "Link-Binding ../agent:bad",
            "Link-Binding agent:bad",
        ] {
            assert!(Authentication::from_header(malformed).is_none());
        }
    }
}
