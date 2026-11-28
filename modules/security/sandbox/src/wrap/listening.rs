//! A program under its proxy on this machine's loopback: a sandbox on
//! macOS, which has no network of its own, and a program with no wall.

use std::collections::BTreeSet;
use std::path::Path;

use link_sandbox_client::Wrapped;

use super::args::{line, proxied_env, proxy_args, proxy_secret};
use super::{Proxied, Secrets};

/// `program` under `sandbox proxy --listen`, which serves `way`'s proxy
/// on `port` of this machine's loopback, and what its environment gets,
/// the proxy's address only where it's `pointed` there: a sandbox on
/// macOS, which has no network of its own, and a program with no wall.
pub(super) fn on_loopback(
    way: &Proxied,
    port: u16,
    exe: &Path,
    program: Vec<String>,
    secrets: Secrets,
    pointed: bool,
) -> anyhow::Result<Wrapped> {
    let proxied = way.proxy;
    let (mut proxy, secret) = proxy_args(way, secrets)?;
    proxy.listen = Some(port);
    proxy.tcp = proxied.tcp.clone();
    // Any process of the machine reaches this port: a way out only with
    // this wrap's secret, for a program pointed here. One that isn't
    // reaches the network itself.
    let proxy_secret = match pointed {
        true => {
            let (proxy_secret, by) = proxy_secret()?;
            proxy.pointed_by = Some(by);
            Some(proxy_secret)
        }
        false => None,
    };
    let env = proxied_env(port, exe, way, &secret, proxy_secret.as_deref(), pointed);
    if port == 0 {
        proxy.launch = Some(super::launch::Launch::keep(way.name, &program, &env)?);
    }
    proxy.argv = program;
    Ok(Wrapped {
        argv: line(exe, proxy.argv()),
        env,
    })
}

/// macOS: the rules of a proxied sandbox (no network of its own), able
/// to connect to its proxy's `port` on this machine's loopback, besides
/// its own `ports`; not to a model runtime's (`models`), which it
/// reaches on its proxy, for inference only. It listens on the ports it
/// serves alone, never its proxy's or a model's.
pub fn seatbelted(
    rules: link_sandbox_confine::Rules,
    ports: &BTreeSet<u16>,
    models: &BTreeSet<u16>,
    port: u16,
) -> link_sandbox_confine::Rules {
    let mut ports: BTreeSet<u16> = ports.difference(models).copied().collect();
    ports.insert(port);
    let mut rules = rules;
    rules.macos.network = false;
    rules.macos.localhost = false;
    rules.macos.ports = ports;
    rules.macos.serves = rules
        .macos
        .serves
        .iter()
        .copied()
        .filter(|p| *p != port && !models.contains(p))
        .collect();
    rules
}

#[cfg(test)]
mod tests {
    use link_sandbox_client::Net;

    use super::*;
    use crate::tests::proxied;
    #[test]
    fn on_macos_the_proxy_is_the_only_way_out() {
        let sandbox = proxied();
        let Net::Proxied { proxy, ports, .. } = &sandbox.net else {
            unreachable!()
        };
        let offline = seatbelted(
            link_sandbox_confine::Rules::of(&sandbox).unwrap(),
            ports,
            &proxy.models(),
            50123,
        );
        // No network, no every-port localhost: its ports and the proxy's,
        // not the model's, which it reaches on its proxy.
        assert!(!offline.macos.network && !offline.macos.localhost);
        assert_eq!(offline.macos.ports, [18789, 50123].into());
        let mut rules = offline;
        rules.common.home = "/Users/me".into();
        let profile = link_sandbox_confine::macos::seatbelt::profile(&rules.common, &rules.macos);
        assert!(!profile.contains("com.apple.trustd"), "{profile}");
        // With network, behind its proxy: certificate checks, not DNS.
        rules.macos.certificates = true;
        let checked = link_sandbox_confine::macos::seatbelt::profile(&rules.common, &rules.macos);
        assert!(checked.contains("com.apple.trustd"), "{checked}");
        assert!(!checked.contains("configd"), "{checked}");
        assert!(profile.contains(r#"(allow network-outbound (remote ip "localhost:50123"))"#));
        assert!(!profile.contains("localhost:11434"), "{profile}");
        assert!(!profile.contains(r#"(remote ip "*:*")"#), "{profile}");
        assert!(
            !profile.contains(r#"(remote ip "localhost:*")"#),
            "{profile}"
        );
        assert!(
            !profile.contains("DNSConfiguration"),
            "no DNS: the proxy resolves"
        );
        assert_eq!(
            link_sandbox_proxy::client::proxy_env(50123, Some("0123456789abcdef"))["HTTPS_PROXY"],
            "http://lnk:0123456789abcdef@127.0.0.1:50123"
        );
    }
}
