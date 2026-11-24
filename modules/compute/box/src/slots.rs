//! Where a command's slots send work (`link_workplace`), a wrong one
//! refused with the slot it belongs in.

use link_format::{Exit, Foreign, Report, sentence};
use link_workplace::{Known, Refusal, Slots, Target};

use crate::listing::accounts;
use link_box::said::said;
use link_box::state::Boxes;

/// A refusal as a usage report.
pub(super) fn refused(refusal: Refusal) -> anyhow::Error {
    said(Report::error(sentence!("{}.", Foreign::new(&refusal.to_string()))).exit(Exit::Usage))
}

/// The target `slots` name, if they name one. A name that is a cloud
/// account and no machine of yours isn't one for `--on`, and a machine
/// and no account isn't one for `--cloud`.
pub(super) fn target(slots: &Slots) -> anyhow::Result<Option<Target>> {
    let target = slots.target().map_err(refused)?;
    let Some(named) = &target else {
        return Ok(None);
    };
    if matches!(named, Target::On(_) | Target::Cloud { .. }) {
        let clouds = accounts();
        let machines: Vec<String> = Boxes::load()
            .unwrap_or_default()
            .boxes
            .into_iter()
            .map(|b| b.name)
            .collect();
        named
            .check(&Known {
                machines: &machines,
                clouds: &clouds,
                ..Known::default()
            })
            .map_err(refused)?;
    }
    Ok(target)
}
