//! A secret for one address of its own, `https://<host>/<path>`: a
//! service a program is pointed at by its base URL (an OpenAI-compatible
//! API elsewhere), the secret sent to that host alone and only
//! under that path, never to wherever else the program's client would
//! send it.

use anyhow::Context;

use super::{SecretHeader, SecretPath, SecretRest, SecretRule, url};

/// A secret's rule for one address ([`Address::new`]), and where under
/// its host the address is.
#[derive(Clone, Debug, PartialEq)]
pub struct Address {
    pub rule: SecretRule,
    /// The address's path, without its slashes (`v1`), or empty.
    pub path: String,
}

impl Address {
    /// The rule for `base_url`, the secret sent in `header`: to its one
    /// host over HTTPS, at any path under its own, by GET or POST. An
    /// error for any other address: plain HTTP, a port of its own, a
    /// user, a query or fragment, or an IP address.
    pub fn new(base_url: &str, header: SecretHeader) -> anyhow::Result<Address> {
        let refused = || format!("{base_url:?} isn't an address over HTTPS on its own port");
        let rest = base_url.strip_prefix("https://").with_context(refused)?;
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        anyhow::ensure!(
            !host.is_empty() && !host.contains([':', '@', '[']) && !rest.contains(['?', '#']),
            "{}",
            refused()
        );
        let path = path.trim_matches('/').to_string();
        let rule = SecretRule {
            host: host.to_ascii_lowercase(),
            paths: vec![SecretPath {
                prefix: match path.is_empty() {
                    true => String::new(),
                    false => format!("{path}/"),
                },
                rest: SecretRest::Path,
            }],
            header: Some(header),
            refuse: Vec::new(),
            methods: vec!["GET".into(), "POST".into()],
            public: None,
            socket: None,
        };
        rule.check()
            .with_context(|| format!("{base_url:?} as a secret's address"))?;
        Ok(Address { rule, path })
    }

    /// Where a program behind a proxy on `port` reaches the address as
    /// secret `name`: in place of `base_url`.
    pub fn url(&self, port: u16, name: &str) -> String {
        let root = url(port, name);
        match self.path.is_empty() {
            true => root,
            false => format!("{root}/{}", self.path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bearer() -> SecretHeader {
        SecretHeader {
            name: "Authorization".into(),
            value: "Bearer {secret}".into(),
            basic: None,
        }
    }

    /// An address over HTTPS is one host's rule, under its path, and the
    /// program is given the proxy's route in its place.
    #[test]
    fn an_address_is_its_one_hosts_rule() {
        let address = Address::new("https://Models.Example/v1/", bearer()).unwrap();
        assert_eq!(address.rule.host, "models.example");
        assert_eq!(address.path, "v1");
        assert_eq!(address.rule.paths[0].prefix, "v1/");
        assert_eq!(address.rule.paths[0].rest, SecretRest::Path);
        assert_eq!(address.rule.header, Some(bearer()));
        assert_eq!(
            address.url(40781, "model"),
            "http://127.0.0.1:40781/_link/secret/model/v1"
        );
        let bare = Address::new("https://models.example", bearer()).unwrap();
        assert_eq!(bare.rule.paths[0].prefix, "");
        assert_eq!(
            bare.url(40781, "model"),
            "http://127.0.0.1:40781/_link/secret/model"
        );
    }

    /// Anything but one host's name over HTTPS on its own port is refused.
    #[test]
    fn an_address_the_proxy_cant_hold_to_is_refused() {
        for url in [
            "http://models.example/v1",
            "https://models.example:8443/v1",
            "https://user@models.example/v1",
            "https://192.0.2.1/v1",
            "https://[::1]/v1",
            "https://models.example/v1?x=1",
            "https://models.example/v1#x",
            "https://*.example/v1",
            "https://localhost/v1",
            "https:///v1",
            "models.example/v1",
            "https://models.example/../v1",
        ] {
            assert!(Address::new(url, bearer()).is_err(), "{url}");
        }
    }
}
