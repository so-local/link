//! A sandbox whose only way out is this machine's ports (Linux).

use std::path::Path;

use anyhow::ensure;
use link_format::WithinExt;
use link_sandbox_client::Wrapped;

use link_sandbox_contract::spec::SandboxLoopbackArgs;

use super::args::{line, list};

/// Linux: the sandbox in a network namespace of its own whose only way
/// out is this machine's ports, relayed through `dir` by `sandbox
/// loopback` (outside) and `sandbox loopback --inside` (in it).
pub(super) fn looped(
    mut rules: link_sandbox_confine::Rules,
    dir: &Path,
    profile: &Path,
    exe: &Path,
    argv: &[String],
) -> anyhow::Result<Wrapped> {
    ensure!(
        dir.is_absolute(),
        "{} isn't an absolute path",
        dir.display()
    );
    link_files::create_private_dir(dir).within(format!("creating {}", dir.display()))?;
    // This program, read only, but not its folder: it relays inside.
    rules.common.read.push(link_sandbox_confine::resolve(exe));
    rules.linux.own = vec![link_sandbox_confine::resolve(dir)];
    let reach = list(&mut rules.linux.ports.iter());
    let relay = |inside: bool, argv: Vec<String>| SandboxLoopbackArgs {
        inside,
        dir: dir.to_path_buf(),
        reach: Some(reach.clone()),
        argv,
    };
    let inner = line(exe, relay(true, argv.to_vec()).argv());
    let wrapped = link_sandbox_confine::wrap(&rules, profile, &inner)?;
    Ok(Wrapped {
        argv: line(exe, relay(false, wrapped).argv()),
        env: Default::default(),
    })
}
