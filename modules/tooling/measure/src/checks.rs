//! The hooks `actions.toml` names: rules across an action's arguments.

/// `lnk measure`: `--pid` is one of `--agent`'s processes, and `--last`
/// is the watcher's last look as JSON alone.
pub fn list_flags(args: &crate::cli::MeasureListArgs) -> Result<(), String> {
    if args.pid.is_some() && args.agent.is_none() {
        return Err("--pid needs --agent: it's one of that agent's processes".into());
    }
    if args.last && !args.json {
        return Err("--last needs --json".into());
    }
    Ok(())
}

/// `lnk measure export set`: exactly one of `--url`, `--off` and
/// `--job` says what it sets, and a header file is sent with looks to
/// `--url`.
pub fn export_flags(args: &crate::cli::ExportSetArgs) -> Result<(), String> {
    let said = [args.url.is_some(), args.off, args.job.is_some()];
    if said.iter().filter(|given| **given).count() != 1 {
        return Err(
            "give one of --url, --off and --job (lnk measure get says where looks go now)".into(),
        );
    }
    match (&args.header_file, &args.url) {
        (Some(_), None) => Err("--header-file needs --url".into()),
        _ => Ok(()),
    }
}

/// `lnk measure set`: at least one setting to write.
pub fn set_flags(args: &crate::cli::MeasureSetArgs) -> Result<(), String> {
    let given = args.every.is_some()
        || args.keep_fine.is_some()
        || args.keep_minutes.is_some()
        || args.processes
        || args.no_processes
        || args.folders_every.is_some()
        || args.folders_max.is_some();
    match given {
        true => Ok(()),
        false => Err("give a setting to set (lnk measure get shows them)".into()),
    }
}
