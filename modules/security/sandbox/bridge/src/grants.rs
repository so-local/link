//! What a bridge asks IAM: whether the agent a call is proved as may do
//! what it asks on another's resource, by `link_policy::allows`, as every
//! check is. A bridge never decides that itself: it says who's asking
//! (the proved [`Identity`], holding the roles the bridge reads at that
//! call, a team's among them), the action and the resource.

use std::time::SystemTime;

use link_policy::{Access, AccountId, Action, Decision, Policy, Principal, PrincipalId, Resource};

use crate::Identity;

/// The agent `who` is proved as, acting as its service, `service:<id>`,
/// of its owner's account, holding `roles`: its own policy grants nothing
/// past its own, so what it reaches of another's is its roles' alone. An
/// error for an agent proved with no service.
pub fn principal(
    who: &Identity,
    roles: impl IntoIterator<Item = Policy>,
) -> anyhow::Result<Principal> {
    let id = service(&who.service)?;
    Ok(Principal::new(
        id,
        AccountId::of_owner(&who.owner),
        None,
        Policy::none("agent"),
    )
    .with_roles(roles))
}

/// The service `id` names, as IAM writes it: `service:<id>`, an ID of its
/// own, never an agent's.
pub fn service(id: &str) -> anyhow::Result<PrincipalId> {
    PrincipalId::new(&link_policy::PrincipalKind::Service, id)
        .map_err(|_| anyhow::anyhow!("{:?} isn't a service's ID", link_format::clean(id)))
}

/// What `allows` says of `principal` doing `action` on `name`, its
/// `[access]` `access` (none inside its account).
pub fn decide(
    principal: &Principal,
    action: Action,
    name: &str,
    access: Access,
) -> anyhow::Result<Decision> {
    let name = name.parse().map_err(anyhow::Error::msg)?;
    let resource = Resource::new(name, access)?;
    Ok(link_policy::allows(
        principal,
        action,
        &resource,
        SystemTime::now(),
    ))
}

/// Whether `principal` may do `action` on every resource of its own
/// account `pattern` reaches (`bucket:team-r7/*`, all of a team's
/// folder): false for one that isn't a pattern. A grant of one name of
/// it alone (`bucket:team-r7/`, the folder's top) is never all of it.
pub fn may_all(principal: &Principal, action: Action, pattern: &str) -> bool {
    pattern.parse().is_ok_and(|on| {
        link_policy::allows_all(principal, action, &on, SystemTime::now()).is_allowed()
    })
}

/// Whether `principal` may do `action` on `name`: false for one that
/// isn't a resource's name.
pub fn may(principal: &Principal, action: Action, name: &str, access: Access) -> bool {
    decide(principal, action, name, access).is_ok_and(|d| d.is_allowed())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_reaches_anothers_only_by_its_roles() {
        let who = Identity {
            agent: "a1b2c3d4e5f6".into(),
            service: "s1s2s3s4s5s6".into(),
            owner: "me@laptop".into(),
            teams: Vec::new(),
        };
        let message: Action = "agent.message".parse().unwrap();
        let alone = principal(&who, []).unwrap();
        // Its service's, never its own ID.
        assert_eq!(alone.id().to_string(), "service:s1s2s3s4s5s6");
        let serviceless = Identity {
            service: String::new(),
            ..who.clone()
        };
        assert!(principal(&serviceless, []).is_err());
        assert_eq!(*alone.account(), AccountId::here());
        assert!(!may(
            &alone,
            message,
            "agent:k3m4n5p6q7r8",
            Access::default()
        ));
        let team = link_policy::parse(
            "schema = 1\nname = \"team-r7\"\nversion = 1\n[[allow]]\nactions = [\"agent.message\"]\non = [\"agent:k3m4n5p6q7r8\"]",
        )
        .unwrap();
        let teammate = principal(&who, [team]).unwrap();
        assert!(may(
            &teammate,
            message,
            "agent:k3m4n5p6q7r8",
            Access::default()
        ));
        assert!(!may(
            &teammate,
            message,
            "agent:x0x0x0x0x0x0",
            Access::default()
        ));
        assert!(!may(&teammate, message, "not a name", Access::default()));
        assert!(service("a b").is_err());
    }
}
