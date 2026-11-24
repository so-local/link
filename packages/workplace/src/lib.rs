//! Where work goes: a [`Target`], parsed once from the slots `--on`,
//! `--cloud`, `--zone` and `--here`, and the place positional short for
//! `--on`, `--cloud` or `--here` ([`Slots`]). A value of the wrong type
//! is refused with the slot it belongs in ([`Refusal`]), never guessed.

use std::fmt;

use serde::{Deserialize, Serialize};

mod shared;
pub use shared::{Confirm, GROUP_PREFIX, Output, Quiet, Request};

/// The slots that say where work goes, as a command's generated parser
/// reads them: at most one of them, and `--zone` only with a cloud. The
/// cloud positional, after the command's own, is short for `--cloud
/// <cloud>`, and its value `here` for `--here`. They are the shared type
/// `workplace`, which declares them and their help once
/// (`link-component`'s `shared.toml`), so every command composing it takes
/// them all.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slots {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    #[serde(default)]
    pub on: Option<String>,
    #[serde(default)]
    pub cloud: Option<String>,
    #[serde(default)]
    pub zone: Option<String>,
    #[serde(default)]
    pub here: bool,
}

/// Where work goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A machine by its real name: a box on any cloud, or a cloud account
    /// that is one machine.
    On(String),
    /// A cloud account from `lnk cloud list`, and a region or zone of it.
    Cloud { cloud: String, zone: Option<String> },
    /// This machine, directly.
    Here,
}

/// A slot of the command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    On,
    Cloud,
    Zone,
    Here,
    /// The place positional, short for `--on <machine>`, `--cloud
    /// <cloud>` or `--here`.
    Place,
}

impl Slot {
    fn flag(self) -> &'static str {
        match self {
            Slot::On => "--on",
            Slot::Cloud => "--cloud",
            Slot::Zone => "--zone",
            Slot::Here => "--here",
            Slot::Place => "<place>",
        }
    }
}

/// Why a target is refused, each saying the right slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Two of the slots that say where work goes.
    Two(Slot, Slot),
    /// `--zone` with no `--cloud`.
    ZoneAlone,
    /// `--zone` with a cloud account that is one machine, which has no
    /// zones and is always `--on`.
    ZoneOfMachine(String),
    /// A value of another type than its slot takes, and what to say instead.
    WrongType {
        slot: Slot,
        value: String,
        is: &'static str,
        instead: String,
    },
    /// A positional naming both a machine and a cloud account, which
    /// only its strict form tells apart.
    Both(String),
    /// A value that cannot name what its slot takes.
    NotAName {
        slot: Slot,
        value: String,
        why: String,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Two(a, b) => write!(
                f,
                "{} and {} both say where: give one of <place>, --on, --cloud or --here",
                a.flag(),
                b.flag()
            ),
            Refusal::ZoneAlone => {
                write!(f, "--zone goes with a cloud: --cloud <cloud> --zone <zone>")
            }
            Refusal::Both(name) => write!(
                f,
                "{name} is both a machine and a cloud account: give --on {name} or --cloud {name}"
            ),
            Refusal::ZoneOfMachine(name) => {
                write!(f, "{name} is one machine, with no zones: --on {name}")
            }
            Refusal::WrongType {
                value, is, instead, ..
            } => write!(f, "{value} is {is}: {instead}"),
            Refusal::NotAName { slot, value, why } => {
                write!(f, "{} {value:?}: {why}", slot.flag())
            }
        }
    }
}

impl std::error::Error for Refusal {}

/// The machines and cloud accounts that exist, which tell a machine from a
/// cloud when a name could be either.
#[derive(Clone, Copy, Debug, Default)]
pub struct Known<'a> {
    pub machines: &'a [String],
    pub clouds: &'a [String],
    /// Of the cloud accounts, those that are each one machine, as their
    /// adapters say: none has zones.
    pub single: &'a [String],
}

impl Slots {
    /// The target the slots name, if they name one. The positional names
    /// what its strict form does, so nothing past here knows which was
    /// typed.
    pub fn target(&self) -> Result<Option<Target>, Refusal> {
        let said = [
            (Slot::Place, self.place.is_some()),
            (Slot::On, self.on.is_some()),
            (Slot::Cloud, self.cloud.is_some()),
            (Slot::Here, self.here),
        ]
        .into_iter()
        .filter(|(_, said)| *said)
        .map(|(slot, _)| slot)
        .collect::<Vec<_>>();
        if let [a, b, ..] = said[..] {
            return Err(Refusal::Two(a, b));
        }
        if self.here || self.place.as_deref() == Some(HERE) {
            return match self.zone {
                Some(_) => Err(Refusal::ZoneAlone),
                None => Ok(Some(Target::Here)),
            };
        }
        if let Some(machine) = &self.on {
            if self.zone.is_some() {
                return Err(Refusal::ZoneAlone);
            }
            return machine_name(machine).map(|name| Some(Target::On(name)));
        }
        let named = match (&self.cloud, &self.place) {
            (Some(cloud), _) => Some((Slot::Cloud, cloud)),
            (None, place) => place.as_ref().map(|place| (Slot::Place, place)),
        };
        let Some((slot, cloud)) = named else {
            return match self.zone {
                Some(_) => Err(Refusal::ZoneAlone),
                None => Ok(None),
            };
        };
        let cloud = cloud_name(slot, cloud)?;
        let zone = self.zone.as_deref().map(zone_name).transpose()?;
        Ok(Some(Target::Cloud { cloud, zone }))
    }

    /// [`Slots::target`], with the place positional read as a machine
    /// where it names one of `known`'s and no cloud account, and checked
    /// against `known` ([`Target::check`]). A name that is both is
    /// refused, giving both strict forms.
    pub fn resolve(&self, known: &Known<'_>) -> Result<Option<Target>, Refusal> {
        let target = self.target()?;
        if let (Some(place), Some(Target::Cloud { zone, .. })) = (&self.place, &target) {
            let has = |names: &[String]| names.iter().any(|n| n == place);
            match (has(known.machines), has(known.clouds)) {
                (true, true) => return Err(Refusal::Both(link_format::clean(place))),
                (true, false) if zone.is_some() => return Err(Refusal::ZoneAlone),
                (true, false) => return machine_name(place).map(|name| Some(Target::On(name))),
                _ => {}
            }
        }
        if let Some(named) = &target {
            named.check(known)?;
        }
        Ok(target)
    }

    /// [`Slots::target`], or this machine where the slots name none.
    pub fn target_or_here(&self) -> Result<Target, Refusal> {
        Ok(self.target()?.unwrap_or(Target::Here))
    }
}

impl Target {
    /// An error unless the target's name is of the type its slot takes: a
    /// name that is a cloud and no machine isn't one for `--on`, and a name
    /// that is a machine and no cloud isn't one for `--cloud`. A name both
    /// are is right in either.
    pub fn check(&self, known: &Known<'_>) -> Result<(), Refusal> {
        let has = |names: &[String], name: &str| names.iter().any(|n| n == name);
        match self {
            Target::Cloud {
                cloud,
                zone: Some(_),
            } if has(known.single, cloud) => Err(Refusal::ZoneOfMachine(cloud.clone())),
            Target::On(machine) if has(known.clouds, machine) && !has(known.machines, machine) => {
                Err(Refusal::WrongType {
                    slot: Slot::On,
                    value: machine.clone(),
                    is: "a cloud",
                    instead: format!("--cloud {machine}"),
                })
            }
            Target::Cloud { cloud, .. }
                if has(known.machines, cloud) && !has(known.clouds, cloud) =>
            {
                Err(Refusal::WrongType {
                    slot: Slot::Cloud,
                    value: cloud.clone(),
                    is: "a machine",
                    instead: format!("--on {cloud}"),
                })
            }
            _ => Ok(()),
        }
    }
}

impl fmt::Display for Target {
    /// The strict form, as a command line spells it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::On(machine) => write!(f, "--on {machine}"),
            Target::Cloud { cloud, zone: None } => write!(f, "--cloud {cloud}"),
            Target::Cloud {
                cloud,
                zone: Some(zone),
            } => write!(f, "--cloud {cloud} --zone {zone}"),
            Target::Here => write!(f, "--here"),
        }
    }
}

/// The positional's word for this machine, short for `--here`.
const HERE: &str = "here";

fn not_a_name(slot: Slot, value: &str, why: impl fmt::Display) -> Refusal {
    Refusal::NotAName {
        slot,
        value: link_format::clean(value),
        why: why.to_string(),
    }
}

fn machine_name(value: &str) -> Result<String, Refusal> {
    match value {
        HERE => Err(Refusal::WrongType {
            slot: Slot::On,
            value: value.into(),
            is: "this machine",
            instead: "use --here".into(),
        }),
        _ => link_machine::names::check_name(value)
            .map(|()| value.to_string())
            .map_err(|e| not_a_name(Slot::On, value, e)),
    }
}

fn cloud_name(slot: Slot, value: &str) -> Result<String, Refusal> {
    match value {
        HERE => Err(Refusal::WrongType {
            slot,
            value: value.into(),
            is: "this machine",
            instead: "use --here".into(),
        }),
        _ => link_machine::names::check_shape(value)
            .map(|()| value.to_string())
            .map_err(|e| not_a_name(slot, value, e)),
    }
}

/// A region or zone: a cloud's own word for it, so only its characters are
/// held to (`us-east-1`, `europe-west4-b`).
fn zone_name(value: &str) -> Result<String, Refusal> {
    let plain = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    match plain {
        true => Ok(value.to_string()),
        false => Err(not_a_name(
            Slot::Zone,
            value,
            "a region or zone is lowercase letters, digits, dashes and underscores",
        )),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
link_test_kit::program!(system);
