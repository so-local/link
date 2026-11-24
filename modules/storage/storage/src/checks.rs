//! The check hooks `actions.toml` names: rules across a command's
//! arguments, run after the declared checks pass.

use crate::cli::StorageConnectArgs;

/// `connect --owner` says whose `--owner-bucket` is, so it comes with one.
pub fn owner_with_bucket(args: &StorageConnectArgs) -> Result<(), String> {
    match (&args.owner, &args.owner_bucket) {
        (Some(_), None) => {
            Err("--owner names whose --owner-bucket is: give --owner-bucket too".into())
        }
        _ => Ok(()),
    }
}
