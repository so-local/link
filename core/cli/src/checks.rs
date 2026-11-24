//! The hooks `actions.toml` names: rules across a core command's
//! arguments, run after the declared checks pass.

use crate::cli::{CoreUninstallArgs, CoreUpArgs, ModuleAddArgs};

/// `lnk uninstall --parts` lists what modules keep, for a program: it
/// answers in JSON alone.
pub fn parts_with_json(args: &CoreUninstallArgs) -> Result<(), String> {
    match args.parts && !args.json {
        true => Err("--parts answers for another program: give --json too".into()),
        false => Ok(()),
    }
}

/// `lnk up --module` and `--stale` choose among a checkout's modules, so
/// each comes with `--source`, unless this `lnk` is built from a checkout,
/// which is then the one they choose from.
pub fn up_from_source(args: &CoreUpArgs) -> Result<(), String> {
    let chooses = !args.module.is_empty() || args.stale;
    match chooses && args.source.is_empty() && link_core::CHECKOUT.is_none() {
        true => Err("--module and --stale choose a checkout's modules: give --source too".into()),
        false => Ok(()),
    }
}

/// `lnk module add` adds Link's modules by name, or one from outside Link
/// by `--url`: at least one of them.
pub fn module_or_url(args: &ModuleAddArgs) -> Result<(), String> {
    match args.modules.is_empty() && args.url.is_empty() {
        true => Err("name a module, or give --url for one from outside Link".into()),
        false => Ok(()),
    }
}
