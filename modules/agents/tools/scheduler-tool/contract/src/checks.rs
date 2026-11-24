//! The check hooks the tool's declarations name, on the arguments this
//! crate generates.

use crate::cli::SchedulerToolRunArgs;

pub(crate) fn agents(args: &SchedulerToolRunArgs) -> Result<(), String> {
    crate::agents(args.agents.is_some(), args.flags.time_zone.is_some())
}
