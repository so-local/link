//! What wrapping a program needs of this machine, for the effective,
//! resolved policy: on Linux bubblewrap and user namespaces, and Landlock
//! when its programs are listed; on macOS Seatbelt. Each is a dependency
//! of `module.toml`, read from there by its name, and which a wrap
//! needs is `requirements.toml`'s.

use anyhow::Context;

/// The module's declaration, which names each dependency.
const MODULE: &str = include_str!("../module.toml");

/// Each operating system's needs, by policy, as `requirements.toml` says.
const REQUIREMENTS: &str = include_str!("requirements.toml");

/// The dependencies a wrap of `policy` (`restricted` or `unrestricted`)
/// needs here, by their names in `module.toml`.
fn needed(policy: &str) -> anyhow::Result<Vec<String>> {
    let system = match cfg!(target_os = "macos") {
        true => "macos",
        false => "linux",
    };
    needed_on(system, policy)
}

fn needed_on(system: &str, policy: &str) -> anyhow::Result<Vec<String>> {
    let table: std::collections::BTreeMap<String, std::collections::BTreeMap<String, Vec<String>>> =
        toml::from_str(REQUIREMENTS).context("requirements.toml")?;
    table
        .get(system)
        .and_then(|policies| policies.get(policy))
        .cloned()
        .with_context(|| format!("requirements.toml names no {system} {policy}"))
}

/// Probes each dependency `command` needs for `policy`.
fn probe(command: &str, policy: &str) -> anyhow::Result<()> {
    let module: toml::Table = toml::from_str(MODULE).context("module.toml")?;
    let dependencies = module
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .context("module.toml's dependencies")?;
    for name in needed(policy)? {
        let name = name.as_str();
        let dependency = dependencies
            .get(name)
            .and_then(toml::Value::as_table)
            .with_context(|| format!("module.toml's dependency {name}"))?;
        let id = dependency
            .get("id")
            .and_then(toml::Value::as_str)
            .with_context(|| format!("{name}'s id"))?;
        match dependency.get("kind").and_then(toml::Value::as_str) {
            Some("external") if id == "bubblewrap" => anyhow::ensure!(
                link_files::bubblewrap().is_some(),
                "{command} requires bubblewrap (bwrap)"
            ),
            Some("capability") => {
                let min_abi = dependency
                    .get("min_abi")
                    .and_then(toml::Value::as_integer)
                    .map(u32::try_from)
                    .transpose()?
                    .unwrap_or(1);
                link_core::descriptor::probe_capability(id, min_abi)
                    .map_err(|e| anyhow::anyhow!("{command}: {e:#}"))?;
            }
            other => anyhow::bail!(
                "{name} ({id}) is a dependency of kind {other:?}, which no probe serves"
            ),
        }
    }
    Ok(())
}

/// Whether this machine can wrap a program as `command` does for
/// `policy`, the effective command policy.
pub fn check(command: &str, policy: &str) -> anyhow::Result<()> {
    let check = || probe(command, policy);
    // On Linux, met once per boot, kernel, bubblewrap and this program
    // (whose requirements they are): its probes start bubblewrap, and
    // each wrap is a new process.
    #[cfg(target_os = "linux")]
    if let Ok(bwrap) = link_sandbox_confine::linux::bwrap::bwrap() {
        let probe = format!("requires {command:?} {policy:?}");
        return link_sandbox_confine::linux::kept::checked(bwrap, &probe, check);
    }
    check()
}

#[cfg(test)]
mod tests {
    /// Each dependency a wrap needs, on each system, is one `module.toml`
    /// declares.
    #[test]
    fn every_needed_dependency_is_declared() {
        let module: toml::Table = toml::from_str(super::MODULE).unwrap();
        for system in ["linux", "macos"] {
            for policy in ["restricted", "unrestricted"] {
                for name in super::needed_on(system, policy).unwrap() {
                    assert!(module["dependencies"].get(&name).is_some(), "{name}");
                }
            }
        }
    }
}
