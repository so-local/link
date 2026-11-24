//! A box spec: the machine you rent and the `lnk` on it, written down.
//!
//! One type, generated from the module's `types.toml`, read from a TOML
//! file (`lnk box create <cloud> --spec web.toml`) and from its field
//! flags (`--machine.cpus 4`) alike. It says what box, never whose (the
//! account) or which (`--name`). Every value is typed where it's read,
//! since a spec may be someone else's: nothing from a file reaches a
//! shell or a cloud's CLI as written. [`Fields`] is a machine table's
//! values typed, what a want is made of.

use std::str::FromStr;
use std::sync::LazyLock;

use anyhow::{bail, ensure};

use link_cloud_contract::want::{
    CloudName, DiskType, GpuModel, MAX_TAGS, Range, Size, Tag, Want, from_word, word,
};
/// The processor and the OS: one list, the box's and an environment's.
pub use link_cloud_contract::{Arch, Os};

use link_version::Version;

/// The box spec and its parts, as `types.toml` declares them.
pub use crate::cli::{
    Box as BoxSpec, CountBounds, CountRange, Disk, GpuBounds, Gpus, Isolation, Lnk, Machine,
    SizeBounds, SizeRange,
};

mod reading;
mod values;
mod wanting;
pub(crate) use values::lnk_version;
pub use values::{CloudKind, check_name};
pub use wanting::fit;

/// The first release with modules: a box's `lnk` is this or later.
pub const FLOOR: Version = Version::new(0, 11, 0);
/// A box's disk when the spec says none, as before specs.
pub const DISK_GB: u64 = 20;
/// The least memory a box gets when its spec asks for CPUs or GPUs but
/// says no memory: what today's boxes have, and `lnk` runs in.
pub const LEAST_MEMORY_MB: u64 = 2048;

/// What a spec that says nothing of a part has there.
static NO_MACHINE: LazyLock<Machine> = LazyLock::new(Machine::default);
static NO_LNK: Lnk = Lnk {
    login: None,
    version: None,
};

/// A machine table's values, typed: `[machine]`'s, or a cloud's own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fields {
    pub cpus: Option<Range<u32>>,
    pub memory: Option<Range<Size>>,
    /// How many GPUs, when it says.
    pub gpus: Option<Range<u32>>,
    pub gpu_model: Option<GpuModel>,
    pub arch: Option<Arch>,
    pub disk_min: Option<Size>,
    pub disk_type: Option<DiskType>,
    pub os: Option<Os>,
    pub spot: Option<bool>,
    pub tags: Vec<Tag>,
    pub region: Option<CloudName>,
    pub size: Option<CloudName>,
}

impl Fields {
    /// Every field's name, as a file writes it.
    pub const NAMES: &[&str] = &[
        "cpus", "memory", "gpus", "arch", "disk", "os", "spot", "tags", "region", "size",
    ];

    /// `machine`'s own values, typed, each refused naming its field.
    pub fn of(machine: &Machine) -> anyhow::Result<Fields> {
        let (gpus, gpu_model) = match &machine.gpus {
            None => (None, None),
            Some(Gpus::Number(n)) => (Some(Range::exactly(count(*n, "gpus")?)), None),
            Some(Gpus::Record(GpuBounds { min, max, model })) => (
                bounds(*min, *max, "gpus")?,
                model.as_ref().map(same::<_, GpuModel>).transpose()?,
            ),
        };
        let disk = machine.disk.as_ref();
        Ok(Fields {
            cpus: match &machine.cpus {
                None => None,
                Some(CountRange::Number(n)) => Some(Range::exactly(count(*n, "cpus")?)),
                Some(CountRange::Record(CountBounds { min, max })) => bounds(*min, *max, "cpus")?,
            },
            memory: match &machine.memory {
                None => None,
                Some(SizeRange::Text(text)) => Some(Range::exactly(size(text, "memory")?)),
                Some(SizeRange::Record(SizeBounds { min, max })) => {
                    let min = min.as_deref().map(|t| size(t, "memory")).transpose()?;
                    let max = max.as_deref().map(|t| size(t, "memory")).transpose()?;
                    Some(
                        Range { min, max }
                            .checked()
                            .map_err(|why| anyhow::anyhow!("memory: {why}"))?,
                    )
                }
            },
            gpus,
            gpu_model,
            arch: machine.arch.as_ref().map(same::<_, Arch>).transpose()?,
            disk_min: disk
                .and_then(|d| d.min.as_deref())
                .map(|t| size(t, "disk.min"))
                .transpose()?,
            disk_type: disk
                .and_then(|d| d.r#type.as_ref())
                .map(same::<_, DiskType>)
                .transpose()?,
            os: machine.os,
            spot: machine.spot,
            tags: machine
                .tags
                .iter()
                .map(|(key, value)| Tag::new(key, value).map_err(anyhow::Error::msg))
                .collect::<anyhow::Result<_>>()?,
            region: name(machine.region.as_deref(), "region")?,
            size: name(machine.size.as_deref(), "size")?,
        })
    }

    /// Whether it says anything a machine type has built in. Not `arch`:
    /// a size's processor is checked against it, never ignored.
    fn sizes_it(&self) -> Option<&'static str> {
        if self.cpus.is_some() {
            Some("cpus")
        } else if self.memory.is_some() {
            Some("memory")
        } else if self.gpus.is_some() || self.gpu_model.is_some() {
            Some("gpus")
        } else {
            None
        }
    }
}

/// A count a field gives, refused when it's not one.
fn count(n: u64, field: &str) -> anyhow::Result<u32> {
    <u32 as link_cloud_contract::want::Amount>::from_int(i64::try_from(n).unwrap_or(i64::MAX))
        .map_err(|why| anyhow::anyhow!("{field}: {why}"))
}

/// A range's two ends, either left out, each a count.
fn bounds(min: Option<u64>, max: Option<u64>, field: &str) -> anyhow::Result<Option<Range<u32>>> {
    if min.is_none() && max.is_none() {
        return Ok(None);
    }
    let min = min.map(|n| count(n, field)).transpose()?;
    let max = max.map(|n| count(n, field)).transpose()?;
    Ok(Some(
        Range { min, max }
            .checked()
            .map_err(|why| anyhow::anyhow!("{field}: {why}"))?,
    ))
}

/// A size a field gives, with its unit.
fn size(text: &str, field: &str) -> anyhow::Result<Size> {
    Size::from_str(text).map_err(|why| anyhow::anyhow!("{field}: {why}"))
}

/// A region or machine type a field gives, in the clouds' own characters.
fn name(text: Option<&str>, field: &str) -> anyhow::Result<Option<CloudName>> {
    text.map(|t| {
        CloudName::try_from(t.to_string()).map_err(|why| anyhow::anyhow!("{field}: {why}"))
    })
    .transpose()
}

/// One of the spec's words as the cloud contract's type of the same
/// words.
fn same<A: serde::Serialize, B: serde::de::DeserializeOwned>(value: &A) -> anyhow::Result<B> {
    Ok(serde_json::from_value(serde_json::to_value(value)?)?)
}

impl BoxSpec {
    /// Its machine, an empty one when it says none.
    pub fn machine(&self) -> &Machine {
        self.machine.as_ref().unwrap_or(&NO_MACHINE)
    }

    /// Its `[lnk]`, an empty one when it says none.
    pub fn lnk(&self) -> &Lnk {
        self.lnk.as_ref().unwrap_or(&NO_LNK)
    }

    /// The `lnk` release it names, checked as it was read.
    pub fn lnk_version(&self) -> Option<Version> {
        self.lnk()
            .version
            .as_deref()
            .and_then(|v| lnk_version(v).ok())
    }

    /// What it attaches, nothing when it says none.
    pub fn attach(&self) -> link_cloud_contract::Attach {
        self.attach.clone().unwrap_or_default()
    }

    /// Whether the machine may remove itself when its work ends.
    pub fn self_remove(&self) -> bool {
        self.self_remove.unwrap_or(false)
    }

    /// Link's own tags on the machine.
    pub fn link_tags(&self) -> &[String] {
        self.link_tags.as_deref().unwrap_or_default()
    }

    /// How a box made here is isolated, as the sandbox's own type has it.
    pub fn isolation(&self) -> link_sandbox_contract::isolation::Isolation {
        let given = self.isolation.as_ref();
        link_sandbox_contract::isolation::Isolation {
            vm: given.and_then(|i| i.vm.clone()),
            no_sandbox: given.and_then(|i| i.no_sandbox).unwrap_or(false),
            sandbox: given.and_then(|i| i.sandbox.clone()),
        }
    }

    /// `[machine]`'s values, typed.
    pub fn fields(&self) -> Fields {
        Fields::of(self.machine()).unwrap_or_default()
    }

    /// Cloud `kind`'s own table's values, typed, if the spec has one.
    pub fn cloud(&self, kind: &str) -> Option<Fields> {
        self.machine()
            .others
            .get(kind)
            .map(|table| Fields::of(table).unwrap_or_default())
    }
}

/// The checks every read of a box spec makes, from a file, its flags or a
/// call alike (`actions.toml`'s `checks`): its values typed, then what
/// the file alone can say ([`BoxSpec::check`]), its isolation and its
/// lifetime.
pub(crate) fn checked(spec: &BoxSpec) -> anyhow::Result<()> {
    let machine = spec.machine();
    Fields::of(machine).map_err(|why| anyhow::anyhow!("[machine] {why:#}"))?;
    for (kind, table) in &machine.others {
        CloudKind::try_from(kind.clone()).map_err(anyhow::Error::msg)?;
        Fields::of(table).map_err(|why| anyhow::anyhow!("[machine.{kind}] {why:#}"))?;
    }
    if let Some(name) = &spec.name {
        check_name(name).map_err(anyhow::Error::msg)?;
    }
    if let Some(version) = &spec.lnk().version {
        lnk_version(version).map_err(anyhow::Error::msg)?;
    }
    if let Some(deadline) = spec.deadline {
        link_cloud_contract::deadline::check(deadline).map_err(anyhow::Error::msg)?;
    }
    spec.check("", false)?;
    spec.isolation().check()?;
    Ok(())
}
