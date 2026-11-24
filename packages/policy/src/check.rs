//! `allows`, the one check every service makes: a principal, an action
//! and a resource in, a yes or a no out, with why. Pure: what's checked
//! is what it's given, the time included.

use std::fmt;
use std::time::SystemTime;

use crate::name::{AccountId, Accounts, Action, PrincipalId, ResourceName, Resources};
use crate::policy::{Access, Policy};

/// Who's asking, as the service that proved it holds them: its id, its
/// account, its end, its own policy and its roles'.
#[derive(Clone, Debug)]
pub struct Principal {
    id: PrincipalId,
    account: AccountId,
    expires: Option<SystemTime>,
    policy: Policy,
    roles: Vec<Policy>,
}

impl Principal {
    /// A principal, ending at `expires` if it ends: a session's service.
    pub fn new(
        id: PrincipalId,
        account: AccountId,
        expires: Option<SystemTime>,
        policy: Policy,
    ) -> Principal {
        Principal {
            id,
            account,
            expires,
            policy,
            roles: Vec::new(),
        }
    }

    /// It, holding `roles` too: what each grants, it may do, as if its
    /// own policy granted it.
    pub fn with_roles(mut self, roles: impl IntoIterator<Item = Policy>) -> Principal {
        self.roles.extend(roles);
        self
    }

    /// The roles it holds, each a Policy Spec.
    pub fn roles(&self) -> &[Policy] {
        &self.roles
    }

    pub fn id(&self) -> &PrincipalId {
        &self.id
    }

    pub fn account(&self) -> &AccountId {
        &self.account
    }

    pub fn expires(&self) -> Option<SystemTime> {
        self.expires
    }

    pub fn policy(&self) -> &Policy {
        &self.policy
    }
}

/// What's asked about: its name and its `[access]`.
#[derive(Clone, Debug)]
pub struct Resource {
    name: ResourceName,
    access: Access,
}

impl Resource {
    /// A resource, refused when its `[access]` names a verb its kind
    /// hasn't.
    pub fn new(name: ResourceName, access: Access) -> anyhow::Result<Resource> {
        access.check(name.kind())?;
        Ok(Resource { name, access })
    }

    pub fn name(&self) -> &ResourceName {
        &self.name
    }
}

/// The answer, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Allowed by grant `grant`, from 1, of the principal's own policy,
    /// or of the role named `role`; and, for a resource outside its
    /// account, by the resource's `[access]` too.
    Allowed {
        grant: usize,
        role: Option<String>,
        by_access: bool,
    },
    Refused(Refused),
}

/// Why a principal was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Its end has passed.
    Expired,
    /// The action isn't one of the resource's kind.
    NotOfKind,
    /// No grant of its policy allows it.
    NotGranted,
    /// Its policy allows it, but the resource is another account's and
    /// its `[access]` doesn't let it in.
    NotLetIn,
}

impl Decision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Decision::Allowed { .. })
    }
}

impl fmt::Display for Decision {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Decision::Allowed {
                grant,
                role,
                by_access,
            } => {
                write!(f, "allowed by grant {grant}")?;
                if let Some(role) = role {
                    write!(f, " of the role {role}")?;
                }
                if *by_access {
                    f.write_str(" and the resource's [access]")?;
                }
                Ok(())
            }
            Decision::Refused(Refused::Expired) => f.write_str("refused: the principal has expired"),
            Decision::Refused(Refused::NotOfKind) => {
                f.write_str("refused: the action isn't one of the resource's kind")
            }
            Decision::Refused(Refused::NotGranted) => {
                f.write_str("refused: no grant of the principal's policy allows it")
            }
            Decision::Refused(Refused::NotLetIn) => f.write_str(
                "refused: the resource is another account's, and its [access] doesn't let the principal in",
            ),
        }
    }
}

/// Whether `principal` may do `action` on `resource` at `now`: its
/// policy or one of its roles allows it, and the resource is in its
/// account or the resource's `[access]` lets it in. Nothing else is
/// allowed.
pub fn allows(
    principal: &Principal,
    action: Action,
    resource: &Resource,
    now: SystemTime,
) -> Decision {
    if principal.expires.is_some_and(|end| now >= end) {
        return Decision::Refused(Refused::Expired);
    }
    if action.kind() != resource.name.kind() {
        return Decision::Refused(Refused::NotOfKind);
    }
    let granted = |policy: &Policy| policy.grant_for(action, &resource.name, &principal.account);
    let found = granted(&principal.policy)
        .map(|grant| (grant, None))
        .or_else(|| {
            principal
                .roles
                .iter()
                .find_map(|role| granted(role).map(|grant| (grant, Some(role.name.clone()))))
        });
    let Some((grant, role)) = found else {
        return Decision::Refused(Refused::NotGranted);
    };
    let own = resource
        .name
        .account()
        .is_none_or(|account| *account == principal.account);
    if own {
        Decision::Allowed {
            grant,
            role,
            by_access: false,
        }
    } else if resource
        .access
        .lets(action, &principal.id, &principal.account)
    {
        Decision::Allowed {
            grant,
            role,
            by_access: true,
        }
    } else {
        Decision::Refused(Refused::NotLetIn)
    }
}

/// Whether `principal` may do `action` on every resource of its own
/// account `on` reaches (`bucket:team-r7/*`, all of a team's folder) at
/// `now`: a grant of its policy or a role reaches all of that pattern.
/// One that reaches a name of it alone (`bucket:team-r7/`, the folder's
/// top) never does. Another account's resources are never asked so.
pub fn allows_all(
    principal: &Principal,
    action: Action,
    on: &Resources,
    now: SystemTime,
) -> Decision {
    all(principal, action, on, None, now)
}

/// Whether the principal may reach the whole pattern, including another
/// account's explicitly named resource whose current access covers it all.
/// A resource keeper supplies that access; a caller's token cannot supply it.
pub fn allows_all_with_access(
    principal: &Principal,
    action: Action,
    on: &Resources,
    access: &Access,
    now: SystemTime,
) -> Decision {
    all(principal, action, on, Some(access), now)
}

fn all(
    principal: &Principal,
    action: Action,
    on: &Resources,
    access: Option<&Access>,
    now: SystemTime,
) -> Decision {
    if principal.expires.is_some_and(|end| now >= end) {
        return Decision::Refused(Refused::Expired);
    }
    if on.kind().is_some_and(|kind| kind != action.kind()) {
        return Decision::Refused(Refused::NotOfKind);
    }
    let outside = match on {
        Resources::Of {
            accounts: Accounts::Any,
            ..
        } => return Decision::Refused(Refused::NotLetIn),
        Resources::Of {
            accounts: Accounts::One(account),
            ..
        } => *account != principal.account,
        _ => false,
    };
    if outside
        && !access.is_some_and(|access| {
            access.check(action.kind()).is_ok()
                && access.lets(action, &principal.id, &principal.account)
        })
    {
        return Decision::Refused(Refused::NotLetIn);
    }
    let granted = |policy: &Policy| policy.grant_for_all(action, on, &principal.account);
    let found = granted(&principal.policy)
        .map(|grant| (grant, None))
        .or_else(|| {
            principal
                .roles
                .iter()
                .find_map(|role| granted(role).map(|grant| (grant, Some(role.name.clone()))))
        });
    match found {
        Some((grant, role)) => Decision::Allowed {
            grant,
            role,
            by_access: outside,
        },
        None => Decision::Refused(Refused::NotGranted),
    }
}
