//! Optional team commands of a cloud that hosts agents: `agent-team`'s
//! request and reply, generated from the contract's declarations
//! (`types.toml`).

pub use crate::declared::{
    HostedTeam, Invitation, Movement, MovingMember, TeamAction as Action, TeamActionApply,
    TeamActionBackup, TeamActionBeginIn, TeamActionBeginOut, TeamActionChat, TeamActionDelete,
    TeamActionFolder, TeamActionGet, TeamActionInvites, TeamActionJoin, TeamActionLeave,
    TeamActionList, TeamActionPut, TeamActionRestore, TeamActionStart, TeamActionStop, TeamFile,
    TeamReply as Reply,
};

impl MovingMember {
    /// Whether the destination supplies this member's models.
    pub fn manages_models(&self) -> bool {
        self.managed_models == Some(true)
    }
}
