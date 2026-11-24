//! The hooks `actions.toml` names: rules across an action's arguments.

/// `lnk archive export` writes from a source: the things `--select`
/// selects, every thing (`--all`) or a folder (`--from`). Its
/// declaration refuses two at once.
pub fn export_one_source(args: &crate::cli::ArchiveExportArgs) -> Result<(), String> {
    match args.select.is_some() || args.all || args.from.is_some() {
        true => Ok(()),
        false => Err("give one of --select, --all and --from".into()),
    }
}
