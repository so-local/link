//! The sandbox's side of the seam, as an adapter's `command` meets it.
//! The sandbox points the harness at its proxy in `HTTPS_PROXY`, the
//! run's secret in its address, and the proxy takes no request without
//! it; the machine's CAs reach it as the machine set them. A `command`
//! that sets a proxy variable of its own takes the harness past the
//! proxy or drops the secret, and one that empties a CA variable makes
//! its runtime refuse the proxy's TLS.

use super::*;

/// What the sandbox sets, and no `command` may.
const PROXY: &[&str] = &[
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
    "npm_config_https_proxy",
    "npm_config_proxy",
];

/// Where a runtime reads the CAs it trusts.
const CAS: &[&str] = &[
    "SSL_CERT_FILE",
    "NODE_EXTRA_CA_CERTS",
    "REQUESTS_CA_BUNDLE",
    "CURL_CA_BUNDLE",
];

/// `launch`, as `command` printed it, meets the sandbox's proxy.
pub(super) fn proxy_checks(launch: &Launch) -> anyhow::Result<()> {
    for name in PROXY {
        ensure!(
            !launch.env.contains_key(*name),
            "command sets {name}: the sandbox points the harness at its proxy, \
             the run's secret in the address, and this takes it past the proxy or drops it"
        );
    }
    for name in CAS {
        ensure!(
            launch.env.get(*name).is_none_or(|v| !v.is_empty()),
            "command empties {name}: its runtime then refuses the proxy's TLS"
        );
    }
    // Node reads `HTTPS_PROXY` only when told to.
    if let Some(told) = launch.env.get("NODE_USE_ENV_PROXY") {
        ensure!(
            told == "1",
            "command sets NODE_USE_ENV_PROXY to {told:?}: Node then ignores the proxy"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_meets_the_proxy_only_as_the_sandbox_points_it() {
        let launch = |env: &[(&str, &str)]| Launch {
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..Launch::default()
        };
        assert!(proxy_checks(&launch(&[("NODE_USE_ENV_PROXY", "1"), ("HOME", "/h")])).is_ok());
        for env in [
            ("HTTPS_PROXY", "http://127.0.0.1:3128"),
            ("no_proxy", "*"),
            ("SSL_CERT_FILE", ""),
            ("NODE_USE_ENV_PROXY", "0"),
        ] {
            assert!(proxy_checks(&launch(&[env])).is_err(), "{env:?}");
        }
    }
}
