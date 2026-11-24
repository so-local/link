//! A sandbox reaching other sandboxes' proxies, each held alone (Linux).

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::ensure;
use link_format::WithinExt;
use link_sandbox_client::{Confinement, Net, Wrapped};

/// Linux: a sandbox reaching other sandboxes' proxies, each held alone
/// ([`Net::Reaching`]): a network of its own, one `sandbox proxy-inside`
/// relaying each proxy's port in, through its folder's sockets, and the
/// program under it. A folder its proxy shares with it by a group, the
/// proxy running as another user ([`link_sandbox_proxy::listen::shared`]), is
/// kept as it is; any other is made this user's alone.
pub(super) fn reaching(
    confinement: &Confinement,
    mut rules: link_sandbox_confine::Rules,
    profile: &Path,
    exe: &Path,
    argv: &[String],
) -> anyhow::Result<Wrapped> {
    let Net::Reaching { reached, ports } = &confinement.net else {
        anyhow::bail!("no proxy");
    };
    let mut taken = BTreeSet::new();
    let shared = reached
        .first()
        .is_some_and(|r| link_sandbox_proxy::listen::shared(&r.dir));
    for reached in reached {
        ensure!(
            reached.dir.is_absolute(),
            "{} isn't an absolute path",
            reached.dir.display()
        );
        ensure!(
            taken.insert(reached.port) && !ports.contains(&reached.port),
            "port {} is two proxies' or the program's",
            reached.port
        );
        ensure!(
            link_sandbox_proxy::listen::shared(&reached.dir) == shared,
            "{} isn't shared with its proxy as the others are: every proxy a sandbox reaches \
             shares its folder alike",
            reached.dir.display()
        );
        if shared {
            continue;
        }
        link_files::create_private_dir(&reached.dir)
            .within(format!("creating {}", reached.dir.display()))?;
    }
    let (first, rest) = reached.split_first().within("no proxy")?;
    let inside = link_sandbox_contract::spec::SandboxProxyInsideArgs {
        dir: first.dir.clone(),
        port: first.port,
        shared,
        also: rest
            .iter()
            .map(|reached| format!("{}={}", reached.dir.display(), reached.port))
            .collect(),
        argv: argv.to_vec(),
        reach: None,
        tcp: Vec::new(),
    };
    let inner = super::args::line(exe, inside.argv());
    rules.common.read.push(link_sandbox_confine::resolve(exe));
    rules.linux.own = reached
        .iter()
        .map(|r| link_sandbox_confine::resolve(&r.dir))
        .collect();
    Ok(Wrapped {
        argv: link_sandbox_confine::wrap(&rules, profile, &inner)?,
        env: Default::default(),
    })
}
