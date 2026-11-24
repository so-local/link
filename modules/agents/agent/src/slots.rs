//! Where a command's slots send work (`link_workplace`), a wrong one
//! refused with the slot it belongs in.

use link_format::{Exit, Foreign, Report, sentence};
use link_workplace::{Known, Refusal, Slots, Target};

use crate::moving::machines::{boxes, clouds};

/// A refusal as a usage report.
pub(crate) fn refused(refusal: Refusal) -> Report {
    Report::error(sentence!("{}.", Foreign::new(&refusal.to_string()))).exit(Exit::Usage)
}

/// `--zone`, refused: an agent goes to a machine or a cloud account, never
/// a zone of one.
pub(crate) fn no_zone(slots: &Slots) -> Result<(), Report> {
    if slots.zone.is_some() {
        return Err(Report::error(sentence!(
            "Agents don't choose a zone: give --cloud <cloud> alone."
        ))
        .exit(Exit::Usage));
    }
    Ok(())
}

/// The target `slots` name, if they name one. The place positional is a
/// box of yours where it names one and no cloud account, and a name both
/// are is refused. A name that is a cloud account and no box of yours
/// isn't one for `--on`, and a box and no account isn't one for
/// `--cloud`.
pub(crate) fn target(slots: &Slots) -> Result<Option<Target>, Report> {
    no_zone(slots)?;
    let target = slots.target().map_err(refused)?;
    if !matches!(target, Some(Target::On(_) | Target::Cloud { .. })) {
        return Ok(target);
    }
    let machines: Vec<String> = boxes(true)
        .unwrap_or_default()
        .into_iter()
        .map(|b| b.name)
        .collect();
    let clouds = clouds();
    slots
        .resolve(&Known {
            machines: &machines,
            clouds: &clouds,
            ..Known::default()
        })
        .map_err(refused)
}
