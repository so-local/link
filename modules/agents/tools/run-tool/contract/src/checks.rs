//! The check hooks the tool's declarations name, on the arguments this
//! crate generates.

use crate::cli::RunToolServeArgs;

pub(crate) fn background(args: &RunToolServeArgs) -> Result<(), String> {
    crate::background(
        args.state.is_some(),
        &[
            args.way_in.is_some(),
            args.r#as.is_some(),
            args.background.is_some(),
            args.background_timeout.is_some(),
            args.background_output.is_some(),
            args.background_keep.is_some(),
            args.background_per_day.is_some(),
            args.hops.is_some(),
        ],
        args.keyring.is_some(),
    )
}
