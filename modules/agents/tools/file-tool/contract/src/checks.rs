//! The check hooks the tool's declarations name, on the arguments this
//! crate generates.

use crate::cli::FileToolServeArgs;

pub(crate) fn team_bridge(args: &FileToolServeArgs) -> Result<(), String> {
    crate::team_bridge(
        !args.team_bridge.is_empty(),
        args.socket.is_some(),
        args.r#as.is_some(),
        args.account.is_some(),
        args.keyring.is_some(),
    )
}
