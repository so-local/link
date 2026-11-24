//! The checks `actions.toml` names, run after the declared ones pass.

use crate::cli::SecretDeleteArgs;

/// `lnk secret delete` deletes one thing: a secret by its name or ID, or
/// on a box given a copy, a service's own secret by its service
/// and use, there alone.
pub fn one_secret_to_delete(args: &SecretDeleteArgs) -> Result<(), String> {
    match (&args.secret, &args.service, &args.own) {
        (Some(_), None, None) => Ok(()),
        (None, Some(_), Some(_)) if args.here => Ok(()),
        (None, Some(_), Some(_)) => {
            Err("--service deletes a service's own secret here alone: give --here too".into())
        }
        (None, Some(_), None) => {
            Err("--service needs --own: which of the service's own secrets".into())
        }
        (None, None, Some(_)) => Err("--own needs --service: whose own secret".into()),
        (Some(_), _, _) => {
            Err("give a secret's name or ID, or --service and --own, not both".into())
        }
        (None, None, None) => Err("give a secret's name or ID".into()),
    }
}
