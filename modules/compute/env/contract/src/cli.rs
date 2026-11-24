//! Where `lnk env` commands run: the slots `--on`, `--cloud`,
//! `--zone` and `--here` (`link_workplace`), told apart against the boxes
//! and cloud accounts that exist.

use link_format::{Exit, Foreign, Name, Report, cmd, prose, sentence};
use link_workplace::{Known, Refusal};
pub use link_workplace::{Slots, Target};

/// Where a command acts, once its slots are checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum At {
    /// This machine, directly.
    Here,
    /// A box of yours, by its name.
    Box(String),
    /// A cloud account, and a zone of it; an account that is one machine
    /// is that account, with no zone.
    Cloud { cloud: String, zone: Option<String> },
}

impl At {
    /// The box or cloud account it names; none, this machine.
    pub fn named(&self) -> Option<&str> {
        match self {
            At::Here => None,
            At::Box(name) => Some(name),
            At::Cloud { cloud, .. } => Some(cloud),
        }
    }
}

/// A refusal as a usage report.
pub fn refused(refusal: Refusal) -> Report {
    Report::error(sentence!("{}.", Foreign::new(&refusal.to_string()))).exit(Exit::Usage)
}

/// The accounts `lnk cloud list` has: each one's name, and whether it is
/// one machine rather than a cloud that makes machines.
pub(crate) fn accounts() -> Vec<(String, bool)> {
    crate::remote::lnk_json::<Vec<serde_json::Value>>(&["cloud", "list", "--json"])
        .unwrap_or_default()
        .into_iter()
        .filter_map(|account| {
            Some((
                account["name"].as_str()?.to_string(),
                account["machine"].as_bool().unwrap_or_default(),
            ))
        })
        .collect()
}

/// Where `slots` say, checked against what exists: `--on` is a box or an
/// account that is one machine, `--cloud` a cloud account. None, where
/// they say nothing. `command`, its words after `lnk` (`env setup`), is
/// what a refusal suggests again with the right slot.
pub fn at(slots: &Slots, command: &str) -> Result<Option<At>, Report> {
    let Some(target) = slots.target().map_err(refused)? else {
        return Ok(None);
    };
    if matches!(target, Target::Here) {
        return Ok(Some(At::Here));
    }
    let accounts = accounts();
    let boxes: Vec<String> = crate::remote::boxes()
        .unwrap_or_default()
        .into_iter()
        .map(|b| b.name)
        .collect();
    let clouds: Vec<String> = accounts.iter().map(|(name, _)| name.clone()).collect();
    let mut machines = boxes.clone();
    let single: Vec<String> = accounts
        .iter()
        .filter(|(_, machine)| *machine)
        .map(|(name, _)| name.clone())
        .collect();
    machines.extend(single.iter().cloned());
    // The place positional is a box where it names one and no cloud
    // account. An account that is one machine is an account of its own, so
    // the positional naming one reaches it as a cloud, as `--on` would.
    let known = match slots.place {
        Some(_) => Known {
            machines: &boxes,
            clouds: &clouds,
            single: &single,
        },
        None => Known {
            machines: &machines,
            clouds: &clouds,
            single: &single,
        },
    };
    let Some(target) = slots.resolve(&known).map_err(refused)? else {
        return Ok(None);
    };
    let one_machine = |name: &str| single.iter().any(|n| n == name);
    match target {
        Target::Here => Ok(Some(At::Here)),
        Target::On(name) if one_machine(&name) => Ok(Some(At::Cloud {
            cloud: name,
            zone: None,
        })),
        Target::On(name) if boxes.contains(&name) => Ok(Some(At::Box(name))),
        Target::On(name) => Err(Report::error(sentence!(
            "Couldn't find a machine named {}.",
            Name::new("installation", &name)
        ))
        .why(prose!(
            "It is neither a box of yours nor a cloud account that is one machine."
        ))
        .next(cmd!("lnk box list"), "list your boxes")
        .next(cmd!("lnk cloud list"), "list your cloud accounts")
        .exit(Exit::Usage)),
        Target::Cloud { cloud, .. } if one_machine(&cloud) => Err(Report::error(sentence!(
            "{} is one machine, not a cloud.",
            Name::new("installation", &cloud)
        ))
        .next(
            link_format::Command::fill(
                &format!("lnk {command} --on {{}}"),
                vec![Name::new("installation", &cloud).into()],
            ),
            "name it as a machine",
        )
        .exit(Exit::Usage)),
        Target::Cloud { cloud, zone } => Ok(Some(At::Cloud { cloud, zone })),
    }
}
