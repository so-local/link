//! How this module names what it keeps to `lnk storage`, which backs up
//! an owner's folder by the owner's ID ([`link_storage_contract::Owner`]):
//! an agent's files folder by `agent:<its ID>`, and a hosted team's
//! folder by its group's ID, `group:<its ID>`. Only this module knows
//! what these kinds mean.

use link_storage_contract::{Owner, OwnerKind};

/// The kind an agent is, as `lnk storage` names its owner.
pub const AGENT: &str = "agent";

/// The kind a team's group is, as `lnk storage` names its folder's owner:
/// the team's one ID ([`link_agent_team::KIND`]).
pub const GROUP: &str = link_agent_team::KIND;

/// Agent `id`'s owner ID, `agent:<id>`.
pub fn agent(id: &str) -> anyhow::Result<Owner> {
    of(AGENT, id)
}

/// The owner ID of the team whose group's ID is `id`, `group:<id>`.
pub fn group(id: &str) -> anyhow::Result<Owner> {
    of(GROUP, id)
}

fn of(kind: &str, id: &str) -> anyhow::Result<Owner> {
    link_machine::machine::check_id(id)?;
    Ok(Owner::new(&OwnerKind::new(kind)?, id)?)
}

/// `--owner <kind>:<id>` naming agent `id` to `lnk storage`'s hidden
/// commands, which take the ID.
pub fn agent_flag(id: &str) -> anyhow::Result<[String; 2]> {
    Ok(["--owner".to_string(), agent(id)?.to_string()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_and_a_teams_group_are_owners_by_their_ids() {
        assert_eq!(
            agent("k3v9q2x7m1ab").unwrap().as_str(),
            "agent:k3v9q2x7m1ab"
        );
        assert_eq!(
            group("t4m9q2x7k1ab").unwrap().as_str(),
            "group:t4m9q2x7k1ab"
        );
        assert!(agent("not an id").is_err());
    }
}
