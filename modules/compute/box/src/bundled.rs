//! `lnk box create --bundle <folder or name>`: a box made from a bundle you built,
//! by that bundle's own `lnk`. Only an `lnk` that trusts a bundle's signer
//! installs it, and a dev bundle's own `lnk` is the one that does, so the
//! command is handed to it with `--bundle` left out; it installs from the
//! bundle it is part of (`link_core::release::Source::bundle_of`).

use std::path::PathBuf;
use std::process::Stdio;

use anyhow::{Context, bail, ensure};
use link_format::Done;

/// The bundle's absolute folder, as `lnk bundle show --root` finds and
/// checks it: `named` is its folder, or the name of one `lnk bundle` kept.
fn folder(named: &str) -> anyhow::Result<PathBuf> {
    let output = link_core::lnk()
        .args(["bundle", "show", named, "--root"])
        .stdin(Stdio::null())
        .output()
        .context("asking lnk bundle where the bundle is")?;
    if !output.status.success() {
        bail!(
            "lnk bundle show {named} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    ensure!(path.is_dir(), "lnk bundle named no folder for {named}");
    Ok(path)
}

/// Hands `box create` to bundle `named`'s own `lnk`, pointed at the bundle.
pub fn create(named: &str) -> anyhow::Result<Done> {
    let root = folder(named)?;
    let lnk = root.join("bin/lnk");
    ensure!(
        lnk.is_file(),
        "bundle {named} holds no lnk for this computer: build it with this computer's target"
    );
    let version = std::fs::read_to_string(root.join(link_core::release::BUNDLE_VERSION))
        .with_context(|| format!("bundle {named} has no VERSION"))?;
    let status = link_secretive::process::command(&lnk)
        // As typed after `lnk`, `box` first: the bundle's `lnk` runs it.
        .args(link_spec::entry::replay_typed_without("bundle")?)
        .env_remove(link_core::release::RELEASES_VAR)
        .env("LNK_VERSION", version.trim())
        .status()
        .with_context(|| format!("running {}", lnk.display()))?;
    Ok(Done::passed(status.code().unwrap_or(1)))
}
