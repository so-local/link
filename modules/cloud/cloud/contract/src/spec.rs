//! What a box is made as: its [`Spec`], the machine sizes a place offers,
//! and what it's attached to that it doesn't own ([`Attach`]).

use super::*;

/// What `lnk box create` asks an adapter for, generated from the
/// contract's declarations (`types.toml`). Each field after `user_data`
/// has its default, so an older `lnk box` makes the same box with a newer
/// adapter.
pub use crate::declared::Spec;

/// A box's processor and OS, as every machine names them.
pub use link_machine::machine::{Arch, Os};

/// The OSes a cloud box can have, the first the default. The shared
/// machine OS type also names guests outside this cloud contract.
pub const OSES: &[&str] = &[Os::Ubuntu2404.word(), Os::Ubuntu2204.word()];

/// A spec naming nothing but what each field defaults to, its defaults
/// the declaration's.
impl Default for Spec {
    fn default() -> Self {
        serde_json::from_value(serde_json::json!({"name": "", "size": "", "user_data": ""}))
            .expect("the declared defaults")
    }
}

impl Spec {
    /// An error unless every machine field is one an adapter takes: an
    /// OS from [`OSES`], `x86_64` or `arm64` (without GPUs), a disk of 8
    /// GiB to 16 TiB, `balanced` or `fast`, a GPU model of lowercase
    /// letters, digits and dashes.
    pub fn check(&self) -> anyhow::Result<()> {
        let attach = self.attach.clone().unwrap_or_default();
        attach.check()?;
        anyhow::ensure!(
            !self.self_remove() || attach.identity.is_none(),
            "a box that removes itself uses its own identity"
        );
        if !OSES.contains(&self.os.as_str()) {
            bail!(
                "{:?} isn't an OS a cloud box can have: {}",
                link_format::clean(&self.os),
                OSES.join(", ")
            );
        }
        match self.arch.as_str() {
            "x86_64" => {}
            "arm64" if self.gpus > 0 => {
                bail!("an arm64 box has no GPUs: the images with NVIDIA's driver are x86_64")
            }
            "arm64" => {}
            other => bail!(
                "{:?} isn't a processor a box can have",
                link_format::clean(other)
            ),
        }
        if !(8..=16 * 1024).contains(&self.disk_gb) {
            bail!("a box's disk is 8 GB to 16 TB, not {} GB", self.disk_gb);
        }
        if !["balanced", "fast"].contains(&self.disk_type.as_str()) {
            bail!(
                "{:?} isn't a disk type: balanced or fast",
                link_format::clean(&self.disk_type)
            );
        }
        if !self.gpu_model().is_empty() {
            check_label(self.gpu_model())?;
        }
        ensure!(
            !self.self_remove() || self.deadline.is_some(),
            "a disposable machine needs a cloud deadline"
        );
        if let Some(deadline) = self.deadline {
            crate::deadline::check(deadline).map_err(|why| anyhow::anyhow!(why))?;
        }
        Ok(())
    }
}

/// A machine type `sizes` lists, and what a box attaches without owning
/// it, generated from the contract's declarations.
pub use crate::declared::{Attach, MachineSize};

/// A count or size as the number its readers take, which its shape's
/// bounds keep it within.
fn unsigned<F, T: TryFrom<F> + Default>(value: F) -> T {
    T::try_from(value).unwrap_or_default()
}

impl MachineSize {
    pub fn cpus(&self) -> u32 {
        unsigned(self.cpus)
    }
    /// Its memory in MiB.
    pub fn memory_mb(&self) -> u64 {
        unsigned(self.memory_mb)
    }
    pub fn gpus(&self) -> u32 {
        unsigned(self.gpus)
    }
    /// Its GPUs' model in Link's words; empty without.
    pub fn gpu_model(&self) -> &str {
        self.gpu_model.as_deref().unwrap_or_default()
    }
}

/// Nothing attached: what a spec naming no attachments has.
static NOTHING_ATTACHED: Attach = Attach {
    address: None,
    disks: None,
    identity: None,
    network: None,
};

impl Spec {
    /// What the machine attaches without owning it.
    pub fn attach(&self) -> &Attach {
        self.attach.as_ref().unwrap_or(&NOTHING_ATTACHED)
    }

    /// The box's installation ID; empty from an older `lnk box`.
    pub fn id(&self) -> &str {
        self.id.as_deref().unwrap_or_default()
    }
    /// An SSH public key from an older `lnk box`; empty, none.
    pub fn public_key(&self) -> &str {
        self.public_key.as_deref().unwrap_or_default()
    }
    /// Its GPUs' model in Link's words; empty without.
    pub fn gpu_model(&self) -> &str {
        self.gpu_model.as_deref().unwrap_or_default()
    }
    /// Whether the machine may remove itself when its work ends.
    pub fn self_remove(&self) -> bool {
        self.self_remove.unwrap_or_default()
    }
    /// Whether it's reached only through its cloud's tunnel.
    pub fn tunnel(&self) -> bool {
        self.tunnel.unwrap_or_default()
    }

    /// The disk, in GiB.
    pub fn disk_gb(&self) -> u64 {
        unsigned(self.disk_gb)
    }
    /// How many GPUs `size` has.
    pub fn gpus(&self) -> u32 {
        unsigned(self.gpus)
    }
}

impl Eq for Attach {}

impl Attach {
    /// The data disks it attaches.
    pub fn disks(&self) -> &[String] {
        self.disks.as_deref().unwrap_or_default()
    }
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
    pub fn check(&self) -> anyhow::Result<()> {
        for value in self
            .network
            .iter()
            .chain(&self.identity)
            .chain(&self.address)
            .chain(self.disks())
        {
            anyhow::ensure!(
                !value.is_empty()
                    && value.len() <= 256
                    && !value.starts_with('-')
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-@:/".contains(&b)),
                "invalid attachment name"
            );
        }
        let mut unique = std::collections::BTreeSet::new();
        anyhow::ensure!(
            self.disks.iter().all(|d| unique.insert(d)),
            "a disk is attached once"
        );
        Ok(())
    }
}
