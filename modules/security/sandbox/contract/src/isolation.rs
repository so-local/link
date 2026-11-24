//! How a workload on this machine is isolated: in its sandbox, the
//! default, or directly, with the user's own access (`--no-sandbox`).
//! The flags are declared by each command's spec in its `types.toml`,
//! and `lnk box create --here` reads its spec's into this one value.
//!
//! Its `vm` names a VM spec for a workload in a VM. This build has no
//! VM provider, so a file naming one is refused
//! ([`Isolation::check`]): never run in a sandbox instead.
//!
//! Where this machine's agent policy says `unsandboxed = false`, the
//! direct mode is refused ([`check_unsandboxed`]).

use std::path::{Path, PathBuf};

/// How a workload is isolated, as a command's generated spec gives it
/// (box's `isolation`, the agent's): the flags and their help are each
/// command's, declared in its `types.toml`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Isolation {
    /// A VM spec, for a workload in a VM, relative to its file. This
    /// build has no VM provider, so one named is refused, never run in a
    /// sandbox instead.
    pub vm: Option<PathBuf>,
    /// Whether it runs directly, with your own access and no sandbox;
    /// refused where this machine's policy says `unsandboxed = false`.
    pub no_sandbox: bool,
    /// The sandbox spec it runs in; given with no file, it only runs in
    /// its sandbox again.
    pub sandbox: Option<PathBuf>,
}

/// Why a VM is refused: this build of Link runs nothing in a VM.
pub const VM_REFUSED: &str = "isolation.vm names a VM, and this build of Link has no VM provider: nothing is made in its place";

/// Which of the three modes the flags ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode<'a> {
    /// In a sandbox, from this file when one is named.
    Sandbox(Option<&'a Path>),
    /// Directly, with the user's own access.
    Direct,
    /// In a VM, from this VM spec.
    Vm(&'a Path),
}

impl Isolation {
    /// The mode asked for. A VM's guest is sandboxed in turn, so
    /// a VM wins over `--sandbox`; `--no-sandbox` with `--sandbox` is
    /// refused by [`Isolation::check`].
    pub fn mode(&self) -> Mode<'_> {
        match (&self.vm, self.no_sandbox) {
            (Some(vm), _) => Mode::Vm(vm),
            (None, true) => Mode::Direct,
            (None, false) => Mode::Sandbox(self.sandbox_file()),
        }
    }

    /// Its sandbox file, when `--sandbox` names one.
    pub fn sandbox_file(&self) -> Option<&Path> {
        self.sandbox
            .as_deref()
            .filter(|path| !path.as_os_str().is_empty())
    }

    /// Refuses no sandbox together with a sandbox or a VM, and a VM at all
    /// ([`Isolation::check_vm`]).
    pub fn check(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.no_sandbox || (self.vm.is_none() && self.sandbox.is_none()),
            "no_sandbox runs directly, so it takes neither a sandbox nor a VM"
        );
        self.check_vm()
    }

    /// Refuses a VM, which this build has no provider for: what names
    /// one is never made in a sandbox instead.
    pub fn check_vm(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.vm.is_none(), "{VM_REFUSED}");
        Ok(())
    }

    /// Whether it says nothing: the default, a sandbox.
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// Refuses running directly where this machine's agent policy forbids
/// it (`unsandboxed = false`): in its policy file, or in the machine
/// file it was kept in before, not yet moved. Neither file, or neither
/// saying so, allows it: `--no-sandbox` is always an explicit choice.
pub fn check_unsandboxed() -> anyhow::Result<()> {
    check_unsandboxed_at(&link_files::layout::Hidden::AgentPolicy.try_path()?)?;
    check_unsandboxed_at(&link_files::layout::Hidden::Installation.try_path()?)
}

/// [`check_unsandboxed`] for the policy file at `path`, read with the
/// ownership and link checks every trusted setting has.
pub fn check_unsandboxed_at(path: &Path) -> anyhow::Result<()> {
    // Read as trusted settings are: an untrusted file refuses, a missing
    // one allows.
    let table: toml::Table = link_files::settings::read(path)?;
    match table.get("unsandboxed") {
        None | Some(toml::Value::Boolean(true)) => Ok(()),
        Some(toml::Value::Boolean(false)) => anyhow::bail!(
            "this machine forbids running with no sandbox at all (unsandboxed = false in {})",
            path.display()
        ),
        Some(_) => anyhow::bail!("in {}: unsandboxed is true or false", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_mode_comes_from_its_values() {
        let given = |no_sandbox: bool, sandbox: Option<&str>| Isolation {
            vm: None,
            no_sandbox,
            sandbox: sandbox.map(PathBuf::from),
        };
        assert_eq!(given(false, None).mode(), Mode::Sandbox(None));
        assert_eq!(
            given(false, Some("s.toml")).mode(),
            Mode::Sandbox(Some(Path::new("s.toml")))
        );
        // `--sandbox` alone turns a sandbox back on, naming no file.
        assert_eq!(given(false, Some("")).mode(), Mode::Sandbox(None));
        assert_eq!(given(true, None).mode(), Mode::Direct);
        assert!(given(true, Some("s.toml")).check().is_err());
    }

    #[test]
    fn a_vm_is_refused_never_dropped() {
        let vm = Some(PathBuf::from("vm.toml"));
        let file = Isolation {
            no_sandbox: true,
            vm: vm.clone(),
            ..Isolation::default()
        };
        assert!(file.check().is_err());
        let file = Isolation {
            vm,
            ..Isolation::default()
        };
        assert_eq!(file.mode(), Mode::Vm(Path::new("vm.toml")));
        let refused = file.check().unwrap_err().to_string();
        assert!(refused.contains("no VM provider"), "{refused}");
        let file = Isolation {
            no_sandbox: true,
            ..Isolation::default()
        };
        file.check().unwrap();
        assert_eq!(file.mode(), Mode::Direct);
    }

    #[test]
    fn running_directly_is_refused_only_where_the_policy_says_so() {
        let dir = link_test_kit::TempDir::new("isolation-policy");
        let path = dir.join("agents.toml");
        check_unsandboxed_at(&path).unwrap();
        std::fs::write(&path, "unsandboxed = true\n").unwrap();
        check_unsandboxed_at(&path).unwrap();
        std::fs::write(&path, "storage = {}\n").unwrap();
        check_unsandboxed_at(&path).unwrap();
        std::fs::write(&path, "unsandboxed = false\n").unwrap();
        let refused = check_unsandboxed_at(&path).unwrap_err().to_string();
        assert!(
            refused.contains("forbids running with no sandbox"),
            "{refused}"
        );
        std::fs::write(&path, "unsandboxed = 'no'\n").unwrap();
        assert!(check_unsandboxed_at(&path).is_err());
    }
}
