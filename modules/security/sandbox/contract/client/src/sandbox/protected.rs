//! A protected sandbox: a spec Link keeps in its settings folder, which
//! no sandbox reaches, written by none but its owner (an agent's grants
//! for its jobs, which the harness module writes). A run given one is
//! run with its grants as flags, and only once the file here still
//! says, byte for byte, what the run was given: a run sent from
//! elsewhere can't name an arbitrary spec as one.

use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};

use link_sandbox_contract::spec::{Os, Spec};

/// `path`, made whole, when it's a protected sandbox: in Link's settings
/// folder, and trusted there. None for any other spec.
pub fn of(path: &Path) -> anyhow::Result<Option<PathBuf>> {
    let path = path
        .canonicalize()
        .with_context(|| format!("reading {}", path.display()))?;
    let settings = link_files::try_installation_dir()?;
    let settings = settings.canonicalize().unwrap_or(settings);
    if !path.starts_with(&settings) {
        return Ok(None);
    }
    trusted(&path)?;
    Ok(Some(path))
}

/// The flags of protected sandbox `path`, whose text a run was given as
/// `text`: an error unless the file here is trusted and says the same,
/// and grants a command.
pub fn flags(path: &Path, text: &str) -> anyhow::Result<Vec<String>> {
    ensure!(
        trusted(path)?.text == text,
        "the run's sandbox no longer matches {}",
        path.display()
    );
    let spec = link_spec::parse_spec::<Spec>(text)?.on(Os::here())?;
    ensure!(
        spec.commands
            .as_ref()
            .and_then(|c| c.allow.as_ref())
            .is_some_and(|allow| !allow.is_empty()),
        "{} grants no command",
        path.display()
    );
    link_spec::flags_of(&spec)
}

/// The file at `path`, read as its owner (or root) wrote it, written by no
/// one else, followed through no link.
fn trusted(path: &Path) -> anyhow::Result<link_files::fs::Trusted> {
    // SAFETY: geteuid only reads this process's user ID.
    let owner = unsafe { libc::geteuid() };
    link_files::fs::read_trusted(
        path,
        link_files::fs::Trust {
            owners: &[0, owner],
            deny_mode: 0o022,
            ancestors: true,
            no_link: true,
        },
        link_spec::MAX_BYTES,
        "a protected sandbox",
    )
}
