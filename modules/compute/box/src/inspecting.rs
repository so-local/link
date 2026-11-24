//! `lnk box show`: one box as this computer last saw it, asking no cloud.

use super::*;
use link_format::{Fields, Time};

pub(super) fn show(out: &mut Out, name: &str) -> anyhow::Result<Done> {
    let machine = match link_box::machine::bindings()?
        .iter()
        .find(|binding| binding.name == name)
    {
        Some(binding) => link_box::machine::listed(binding),
        None => Boxes::load()?.pick(Some(name))?.clone(),
    };
    let shown = Name::new("box", &machine.name);
    let mut fields = Fields::new()
        .key("State", listing::BoxState::of(&machine))
        .row("Account", &machine.account)
        .row("Place", &machine.place)
        .row("Size", &machine.size)
        .row("Address", machine.ip.clone().unwrap_or_default())
        .row(
            "Made",
            Time(std::time::UNIX_EPOCH + std::time::Duration::from_secs(machine.created)),
        );
    if let Some(here) = &machine.here {
        fields = fields.row("Home", here.home.display().to_string()).row(
            "Runs",
            match here.confinement {
                link_box::state::Confinement::Sandbox { .. } => "in its sandbox",
                link_box::state::Confinement::Direct => "directly, with no sandbox",
            },
        );
    }
    if let Some(deadline) = machine.deadline {
        fields = fields.row(
            "Deadline",
            Time(std::time::UNIX_EPOCH + std::time::Duration::from_secs(deadline)),
        );
    }
    out.block(fields);
    Ok(Done::shown()
        .next(cmd!("lnk box ssh {}", &shown), "open a shell on it")
        .next(
            cmd!("lnk box spec show {}", &shown),
            "see the spec it was made from",
        )
        .next(
            cmd!("lnk box list --refresh"),
            "ask its cloud for its state",
        ))
}
