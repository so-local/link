//! Match a host-issued read binding against the final offered operation.

use super::*;
use link_harness_wire::evidence::{ReadAuthority, ReadRegistration};

impl Toolbox<'_> {
    pub fn register_reads(&mut self, sources: &[ReadRegistration]) {
        self.read_sources = sources.to_vec();
    }

    pub(super) fn read_authority(&self, call: &Call) -> Option<ReadAuthority> {
        let (_, index, operation) = self
            .tools
            .iter()
            .find(|(tool, _, _)| tool.name == call.name)?;
        if operation != "read" {
            return None;
        }
        let server = &self.servers[*index].0;
        Some(authority(
            &self.read_sources,
            server,
            operation,
            &self.agent,
        ))
    }
}

fn authority(
    sources: &[ReadRegistration],
    server: &Server,
    operation: &str,
    agent: &Agent,
) -> ReadAuthority {
    let mut matching = sources.iter().filter(|source| {
        source.server == server.name
            && source.operation == operation
            && source.endpoint == server.url
    });
    let candidate = matching.next();
    match candidate {
        Some(source)
            if matching.next().is_none()
                && !agent.owner().is_empty()
                && source.source.owner == agent.owner()
                && source.source.agent == agent.id() =>
        {
            ReadAuthority::Managed {
                source: source.source.clone(),
            }
        }
        _ => ReadAuthority::ServiceAsserted {
            server: server.name.clone(),
            operation: operation.into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_harness_wire::evidence::{ManagedReadSource, ReadProvider};

    #[test]
    fn final_endpoint_operation_identity_and_unambiguous_binding_are_required() {
        let agent = Agent::with_identity("main", "abcdef123456", "owner").unwrap();
        let server = Server {
            name: "file".into(),
            url: "http://final".into(),
            kept: true,
        };
        let source = ReadRegistration {
            server: "file".into(),
            operation: "read".into(),
            endpoint: server.url.clone(),
            source: ManagedReadSource {
                provider: ReadProvider::File,
                owner: agent.owner().into(),
                agent: agent.id().into(),
                definition: "native".into(),
                argv_sha256: "digest".into(),
            },
        };
        let mut alternate = source.clone();
        alternate.endpoint = "http://alternate".into();
        assert!(matches!(
            authority(&[source.clone(), alternate], &server, "read", &agent),
            ReadAuthority::Managed { .. }
        ));
        for sources in [vec![], vec![source.clone(), source.clone()]] {
            assert!(matches!(
                authority(&sources, &server, "read", &agent),
                ReadAuthority::ServiceAsserted { .. }
            ));
        }
        assert!(matches!(
            authority(&[source], &server, "write", &agent),
            ReadAuthority::ServiceAsserted { .. }
        ));
    }
}
