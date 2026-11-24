//! A run's secret routes, sealed before they reach the proxy and bound to
//! the launcher's peer and the run's bridge key ([`Routes`]).

use super::*;

/// A run's secret route, sealed before it reaches the proxy. The keeper
/// binds the launcher by its socket peer and the run by its bridge key.
pub fn protect_routes(
    routes: &mut [link_sandbox_route::Route],
    binding: &crate::bridge::Binding,
) -> anyhow::Result<()> {
    protect_routes_for(routes, binding, None)
}

/// Root's binder can hand a grant to its proxy helper's UID. An ordinary
/// launcher can only bind its own UID, as the keeper checks again.
pub fn protect_routes_for(
    routes: &mut [link_sandbox_route::Route],
    binding: &crate::bridge::Binding,
    recipient_uid: Option<u32>,
) -> anyhow::Result<()> {
    let Some(socket) = socket() else {
        return Ok(());
    };
    for route in routes.iter_mut().filter(|r| r.name == "secret") {
        let given = call(
            &socket,
            &Request::ProtectRoute {
                recipient_uid,
                given: route.given.duplicate(),
                binding: binding.duplicate(),
            },
        )?;
        match given {
            Reply::Protected { given } => {
                route.given = given;
                route.bridge = link_sandbox_route::Bridge {
                    socket: socket.with_file_name("secret.sock"),
                    start: Vec::new(),
                };
            }
            Reply::Refused { error } => bail!("{error}"),
            other => return Err(other.other("protected")),
        }
    }
    Ok(())
}

/// Requests keeper route revocation when a keyring unbinds a workload.
/// [`unbind_route`] retries and logs failure; its void result cannot
/// certify that a disconnected keeper completed revocation.
pub struct RouteRevoker;

impl crate::bridge::Revoker for RouteRevoker {
    fn unbind(&self, workload: &str, key: Option<&Secret>) {
        unbind_route(workload, key)
    }
}

pub fn unbind_route(workload: &str, key: Option<&Secret>) {
    if let Some(socket) = socket() {
        let request = Request::UnbindRoute {
            workload: workload.into(),
            key: key.map(Secret::duplicate),
        };
        for attempt in 0..3 {
            let result = call(&socket, &request).and_then(|reply| match reply {
                Reply::Done => Ok(()),
                Reply::Refused { error } => bail!("{error}"),
                reply => Err(reply.other("done")),
            });
            match result {
                Ok(()) => return,
                Err(error) if attempt == 2 => {
                    link_format::daemon::warn!("revoking the secret route failed: {error:#}")
                }
                Err(_) => {}
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Protected {
    grant: String,
    binding: crate::bridge::Binding,
    given: super::super::route::Given,
}

/// Ephemeral route grants. A restart drops every binding and receiver;
/// proxies must be bound again. No decrypted route goes back to a user.
#[derive(Default)]
pub struct Routes {
    recipient: link_secretive::Recipient,
    bound: std::sync::Mutex<std::collections::BTreeMap<(u32, String, String), BoundRoute>>,
}

struct BoundRoute {
    binding: crate::bridge::Binding,
    grant: String,
}

fn route_about(uid: u32, workload: &str) -> link_secretive::About {
    link_secretive::About {
        owner: format!("{uid}/{workload}"),
        kind: "keeper-route".into(),
        version: 1,
    }
}

impl Routes {
    pub(super) fn protect(
        &self,
        peer: u32,
        uid: Option<u32>,
        given: Secret,
        binding: crate::bridge::Binding,
    ) -> anyhow::Result<Reply> {
        let uid = uid.unwrap_or(peer);
        anyhow::ensure!(
            peer == 0 || peer == uid,
            "only root can bind another user's proxy"
        );
        let given: super::super::route::Given = link_sandbox_route::given(&given)?;
        anyhow::ensure!(
            binding.key.expose().len() >= 32,
            "a bridge proof secret is at least 32 characters"
        );
        for secret in &given.secrets {
            super::super::route::check_secret_name(&secret.name)?;
            secret.rule.check()?;
            secret.check_placeholder()?;
        }
        let grant = link_files::random_token()?;
        let sealed = link_secretive::Sealed::seal_json(
            &Protected {
                grant: grant.clone(),
                binding: binding.duplicate(),
                given,
            },
            self.recipient.seal_to(),
            route_about(uid, &binding.identity.agent),
        )?;
        let id = (
            uid,
            binding.identity.agent.clone(),
            binding.identity.owner.clone(),
        );
        let mut bound = self.bound.lock().unwrap_or_else(|e| e.into_inner());
        anyhow::ensure!(
            bound.contains_key(&id)
                || bound.keys().filter(|(peer, _, _)| *peer == uid).count() < 256,
            "too many secret route bindings for this user"
        );
        bound.insert(id, BoundRoute { binding, grant });
        Ok(Reply::Protected {
            given: Secret::new(serde_json::to_string(&sealed)?),
        })
    }

    pub(super) fn unbind(&self, uid: u32, workload: &str, key: Option<&Secret>) {
        self.bound
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(peer, id, _), binding| {
                (uid != 0 && *peer != uid)
                    || id != workload
                    || key.is_some_and(|key| *key != binding.binding.key)
            });
    }

    /// Open only after the shared bridge verified this peer's proof.
    pub fn open(
        &self,
        uid: u32,
        who: &crate::bridge::Identity,
        given: &Secret,
    ) -> anyhow::Result<super::super::route::Given> {
        let sealed = link_sandbox_route::given(given)?;
        let protected: Protected = self
            .recipient
            .open_json(&sealed, &route_about(uid, &who.agent))?;
        let bindings = self.bound.lock().unwrap_or_else(|e| e.into_inner());
        let current = bindings
            .get(&(uid, who.agent.clone(), who.owner.clone()))
            .context("the route is no longer bound")?;
        anyhow::ensure!(
            crate::bridge::same_agent(&protected.binding.identity, who)
                && protected.binding.key == current.binding.key
                && protected.grant == current.grant,
            "the route belongs to another binding"
        );
        Ok(protected.given)
    }
}

impl crate::bridge::Keys for Routes {
    fn key_of(&self, _: &crate::bridge::Identity) -> Option<Secret> {
        None
    }
    fn key_of_peer(&self, who: &crate::bridge::Identity, uid: u32) -> Option<Secret> {
        self.bound
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(uid, who.agent.clone(), who.owner.clone()))
            .map(|b| b.binding.key.duplicate())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_grants_follow_peer_proof_rotation_and_revocation() {
        use crate::bridge::Keys;
        let routes = Routes::default();
        let binding = crate::bridge::Binding {
            identity: crate::bridge::Identity {
                agent: "agent-1".into(),
                service: String::new(),
                owner: "owner-1".into(),
                teams: Vec::new(),
            },
            key: Secret::new("a-proof-secret-of-at-least-32-characters".into()),
        };
        let given = || Secret::new("{\"secrets\":[]}".into());
        assert!(
            routes
                .protect(101, Some(102), given(), binding.duplicate())
                .is_err()
        );
        let Reply::Protected { given: sealed } = routes
            .protect(0, Some(102), given(), binding.duplicate())
            .unwrap()
        else {
            panic!()
        };
        assert!(routes.key_of_peer(&binding.identity, 101).is_none());
        assert!(routes.key_of_peer(&binding.identity, 102).is_some());
        assert!(routes.open(101, &binding.identity, &sealed).is_err());
        assert!(routes.open(102, &binding.identity, &sealed).is_ok());
        let replacement = crate::bridge::Binding {
            identity: binding.identity.clone(),
            key: Secret::new("another-proof-of-at-least-32-characters".into()),
        };
        routes
            .protect(0, Some(102), given(), replacement.duplicate())
            .unwrap();
        assert!(routes.open(102, &binding.identity, &sealed).is_err());
        routes.unbind(0, &binding.identity.agent, Some(&binding.key));
        assert!(routes.key_of_peer(&replacement.identity, 102).is_some());
        routes.unbind(101, &binding.identity.agent, None);
        assert!(routes.key_of_peer(&replacement.identity, 102).is_some());
        routes.unbind(0, &binding.identity.agent, Some(&replacement.key));
        assert!(routes.key_of_peer(&replacement.identity, 102).is_none());
        // A helper cannot resurrect a retained ciphertext by registering
        // dummy data under an old proof secret after it was revoked.
        routes
            .protect(102, None, given(), binding.duplicate())
            .unwrap();
        assert!(routes.open(102, &binding.identity, &sealed).is_err());
    }
}
