//! A sealed secret: the hosts it may be sent to and how it's put on a
//! request ([`Sealing`]).

use super::*;

/// How a sealed secret is given, set with its value: the one host it's
/// sent to, over HTTPS, the header it goes in, and that header's value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sealing {
    pub host: String,
    /// `Authorization`, or one named for an API key (`X-Api-Key`).
    #[serde(default = "authorization")]
    pub header: String,
    /// The header's value, `{}` where the secret goes; none, `Bearer {}`
    /// for `Authorization` (`Basic {}` with `basic`) and `{}` for any other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// A user: `{}` is then HTTP Basic's credentials, the base64 of
    /// `<user>:<secret>`, as git over HTTPS takes a token
    /// (`x-access-token` for GitHub).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basic: Option<String>,
}

fn authorization() -> String {
    "Authorization".into()
}

impl Sealing {
    /// `host`'s, checked: one host's name, a header that may hold a
    /// secret, and a format with `{}` once.
    pub fn new(
        host: &str,
        header: Option<&str>,
        format: Option<&str>,
        basic: Option<&str>,
    ) -> anyhow::Result<Sealing> {
        crate::net::check_host(host)?;
        ensure!(
            !host.contains('*') && host.parse::<std::net::IpAddr>().is_err(),
            "{host} isn't one host's name"
        );
        let sealing = Sealing {
            host: host.to_ascii_lowercase(),
            header: header.map_or_else(authorization, String::from),
            format: format.map(String::from),
            basic: basic.map(String::from),
        };
        if let Some(user) = &sealing.basic {
            ensure!(
                sealing.header.eq_ignore_ascii_case("authorization"),
                "Basic's credentials go in Authorization, not {:?}",
                link_format::clean(&sealing.header)
            );
            ensure!(
                !user.is_empty()
                    && user.len() <= 64
                    && user
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
                "{:?} isn't a user for Basic: letters, digits, ., _ and -",
                link_format::clean(user)
            );
        }
        let lower = sealing.header.to_ascii_lowercase();
        ensure!(
            (lower == "authorization" || lower.ends_with("api-key"))
                && sealing
                    .header
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "a sealed secret goes in Authorization, or a header named for an API key \
             (X-Api-Key), not {:?}",
            link_format::clean(&sealing.header)
        );
        let format = sealing.format();
        ensure!(
            format.matches("{}").count() == 1
                && !format.contains("{secret}")
                && format.bytes().all(|b| (0x20..0x7f).contains(&b)),
            "the format {:?} needs {{}} once, where the secret goes",
            link_format::clean(&format)
        );
        Ok(sealing)
    }

    /// The header's value, `{}` where the secret goes.
    pub fn format(&self) -> String {
        match &self.format {
            Some(format) => format.clone(),
            None if self.basic.is_some() => "Basic {}".into(),
            None if self.header.eq_ignore_ascii_case("authorization") => "Bearer {}".into(),
            None => "{}".into(),
        }
    }

    /// The proxy's rule for it: its header, for its one host, on any path
    /// and the methods a call makes; what the proxy adds it by.
    pub fn rule(&self) -> crate::secrets::route::SecretRule {
        use crate::secrets::route::{SecretHeader, SecretPath, SecretRest, SecretRule};
        SecretRule {
            host: self.host.clone(),
            paths: vec![SecretPath {
                prefix: String::new(),
                rest: SecretRest::Path,
            }],
            header: Some(SecretHeader {
                name: self.header.clone(),
                value: self.format().replace("{}", "{secret}"),
                basic: self.basic.clone(),
            }),
            methods: ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"]
                .map(String::from)
                .to_vec(),
            ..Default::default()
        }
    }

    /// Checked again as it's read: the settings file is only text.
    pub(super) fn checked(&self) -> anyhow::Result<Sealing> {
        Sealing::new(
            &self.host,
            Some(&self.header),
            self.format.as_deref(),
            self.basic.as_deref(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_is_authorizations_and_its_format_basics() {
        let git = Sealing::new("GitHub.com", None, None, Some("x-access-token")).unwrap();
        assert_eq!(git.host, "github.com");
        assert_eq!(git.format(), "Basic {}");
        assert!(Sealing::new("github.com", Some("X-Api-Key"), None, Some("x")).is_err());
        assert!(Sealing::new("github.com", None, None, Some("a:b")).is_err());
        assert!(Sealing::new("*.github.com", None, None, None).is_err());
        assert_eq!(
            Sealing::new("api.stripe.com", None, None, None)
                .unwrap()
                .format(),
            "Bearer {}"
        );
    }
}
