//! `lnk sandbox run --with-fd`: what a run is given beside its spec.

use std::collections::BTreeSet;

use anyhow::ensure;
use link_format::WithinExt;
use link_sandbox_client::{Confinement, Net, Proxy};

use crate::spec;

/// `confinement` given what `with` holds (`lnk sandbox run --with-fd`):
/// its secrets on its proxy and the ports it serves, a proxy with no
/// internet made for them when its spec opens no network; the folders it
/// writes, each refused as a spec's write would be on `machine`
/// ([`spec::given_write`]), and where it may write none of its programs;
/// and the variables its program gets, those saying where each secret is
/// among them.
pub fn given(
    policy: &mut Confinement,
    with: link_sandbox_client::RunWith,
    machine: &spec::Machine,
) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
    use link_sandbox_client::check_given_variable;
    use link_secret_contract::route::url as secret_url;
    if let Some(named) = &with.named {
        named.check()?;
        ensure!(
            with.secrets.is_empty(),
            "a run cannot mix named routes with inline secrets"
        );
        ensure!(
            !with.env.keys().any(|name| named
                .routes
                .iter()
                .any(|route| route.env.contains_key(name))),
            "a run variable replaces a named route variable"
        );
        if let Net::Proxied { proxy, .. } = &policy.net {
            ensure!(
                !proxy
                    .routes
                    .iter()
                    .flat_map(|route| route.env.keys())
                    .any(|name| named
                        .routes
                        .iter()
                        .any(|route| route.env.contains_key(name))),
                "named run route variables replace an existing route variable"
            );
        }

        if let Net::Proxied { proxy, .. } = &policy.net {
            ensure!(
                proxy
                    .caller
                    .as_ref()
                    .is_none_or(|caller| caller == &named.caller),
                "run routes have a different caller from the sandbox"
            );
            for route in &named.routes {
                ensure!(
                    !proxy
                        .routes
                        .iter()
                        .any(|existing| existing.serves(&route.prefix)
                            || route.serves(&existing.prefix)),
                    "run route replaces an existing sandbox route"
                );
            }
        }
        ensure!(
            matches!(policy.net, Net::Closed | Net::Proxied { .. }),
            "named run routes need a proxy"
        );
    }
    let mut env = std::collections::BTreeMap::new();
    for (name, value) in with.env {
        check_given_variable(&name)?;
        ensure!(!value.contains('\0'), "{name} has a NUL in it");
        env.insert(name, value);
    }
    for folder in with.write {
        let resolved = link_sandbox_confine::resolve_grant(&folder)?;
        spec::given_write(&folder, machine)?;
        spec::given_write(&resolved, machine)?;
        if !policy.write.contains(&resolved) {
            policy.write.push(resolved);
        }
    }
    // Its programs, already found, judged again with these writes.
    if let link_sandbox_client::Programs::Only(_) = policy.commands {
        policy.commands = spec::programs(policy, &[])?;
    }
    if with.secrets.is_empty() && with.serve.is_empty() && with.named.is_none() {
        return Ok(env);
    }
    if let Net::Closed = policy.net {
        policy.net = Net::Proxied {
            proxy: Proxy {
                upstream: "direct".into(),
                dir: crate::run_proxy_dir()?,
                no_internet: true,
                ..Default::default()
            },
            ports: BTreeSet::new(),
        };
    }
    let mut sealed = Vec::new();
    let Net::Proxied { proxy: proxied, .. } = &mut policy.net else {
        anyhow::bail!(
            "secrets and served ports are its proxy's, and a sandbox with [network] mode = \
             \"host\" has none"
        );
    };
    if let Some(named) = with.named {
        proxied.routes.extend(named.routes);
        proxied.caller = Some(named.caller);
    }
    proxied.serve.extend(with.serve);
    // A loopback proxy binds before its program starts. Port zero in
    // these URLs is filled with its bound port by that proxy alone.
    let port = proxied.listen.unwrap_or(match cfg!(target_os = "macos") {
        true => 0,
        false => link_sandbox_proxy::client::PROXY_PORT,
    });
    // This run's own: never a hash of a secret, so a placeholder read
    // from one run is no use in the next.
    let placeholders = link_sandbox_route::Placeholders::new()?;
    let mut git: Vec<(String, String)> = Vec::new();
    for mut given in with.secrets {
        check_given_variable(&given.url)?;
        check_given_variable(&given.placeholder)?;
        given.secret.placeholder = link_secret_contract::route::placeholder(
            &placeholders,
            &given.secret.name,
            &given.secret.value,
            given.secret.rule.public.as_deref(),
        );
        ensure!(
            !given.secret.value.expose().trim().is_empty(),
            "secret {} is empty",
            given.secret.name
        );
        let url = secret_url(port, &given.secret.name);
        // What the program is given in its place: not the secret, and the
        // only form of it git sees.
        let placeholder = given.secret.placeholder.expose_owned();
        if given.git {
            let header = given.secret.rule.header.as_ref().within(format!(
                "secret {} goes in a path, and git sends a token in a header",
                given.secret.name
            ))?;
            let filled = header.filled(&placeholder);
            git.push((
                format!("url.{url}/.insteadOf"),
                format!("https://{}/", given.secret.rule.host),
            ));
            git.push((
                format!("http.{url}/.extraHeader"),
                format!("{}: {filled}", header.name),
            ));
        }
        env.insert(given.url, url);
        env.insert(given.placeholder, placeholder);
        sealed.push(given.secret);
    }
    // Its secrets' route, handed with their values, as its proxy starts at
    // once: lnk-secret's bridge adds each, checked there.
    if !sealed.is_empty() {
        ensure!(
            proxied.route("secret").is_none(),
            "a run given secrets has no secret route of its own beside them"
        );
        let route = link_secret_contract::route::route(
            link_secret_contract::route::bridge()?,
            sealed,
            Vec::new(),
        )?;
        proxied.routes.push(route);
    }
    if !git.is_empty() {
        ensure!(
            !env.keys().any(|k| k.starts_with("GIT_CONFIG_")),
            "a run given a secret for git sets git's configuration itself: no GIT_CONFIG_ \
             variable of its own"
        );
        env.insert("GIT_CONFIG_COUNT".into(), git.len().to_string());
        for (i, (key, value)) in git.into_iter().enumerate() {
            env.insert(format!("GIT_CONFIG_KEY_{i}"), key);
            env.insert(format!("GIT_CONFIG_VALUE_{i}"), value);
        }
    }
    Ok(env)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::tests::proxied;
    fn context() -> link_sandbox_client::RunWith {
        let route = link_sandbox_route::Route {
            name: "secret".into(),
            prefix: "/_link/secret".into(),
            bridge: link_sandbox_route::Bridge {
                socket: "/outside/secret.sock".into(),
                start: vec![],
            },
            env: [("QA_TOKEN".into(), "placeholder".into())].into(),
            ..Default::default()
        };
        link_sandbox_client::RunWith {
            named: Some(link_sandbox_client::RunRoutes {
                routes: vec![route],
                caller: link_sandbox_binding::Binding {
                    identity: link_sandbox_binding::Identity {
                        agent: "aaaaaaaaaaaa".into(),
                        owner: "local-link-aaaaaaaaaaaa".into(),
                        ..Default::default()
                    },
                    key: "a-binding-key-with-at-least-32-characters"
                        .to_string()
                        .into(),
                },
            }),
            ..Default::default()
        }
    }
    #[test]
    fn named_run_routes_keep_the_caller_and_refuse_changes_before_policy_mutation() {
        use link_secretive::Duplicate;
        let machine = spec::Machine {
            home: Path::new("/home/me"),
            settings: Path::new("/home/me/.lnk"),
            hidden: &[],
            spec: None,
            run_path: Default::default(),
        };
        let mut policy = proxied();
        let expected = context().named.unwrap().caller;
        given(&mut policy, context(), &machine).unwrap();
        let Net::Proxied { proxy, .. } = &policy.net else {
            panic!("no proxy")
        };
        assert_eq!(proxy.caller.as_ref(), Some(&expected));
        assert_eq!(
            proxy.route("secret").unwrap().env["QA_TOKEN"],
            "placeholder"
        );
        for mutation in 0..10 {
            let mut policy = proxied();
            let mut with = context();
            match mutation {
                0 => with.named.as_mut().unwrap().routes[0].handed = true,
                1 => with.named.as_mut().unwrap().routes[0].prefix = "https://evil.example".into(),
                2 => {
                    let Net::Proxied { proxy, .. } = &mut policy.net else {
                        unreachable!()
                    };
                    let mut foreign = expected.duplicate();
                    foreign.identity.agent = "bbbbbbbbbbbb".into();
                    proxy.caller = Some(foreign);
                }
                3 => {
                    with.env.insert("QA_TOKEN".into(), "override".into());
                }
                4 => {
                    let Net::Proxied { proxy, .. } = &mut policy.net else {
                        unreachable!()
                    };
                    let mut route = with.named.as_ref().unwrap().routes[0].duplicate();
                    route.prefix.push_str("/child");
                    proxy.routes.push(route);
                }
                5 => with.named.as_mut().unwrap().caller.key = Default::default(),
                6 => with.named.as_mut().unwrap().caller.identity.agent = "../foreign".into(),
                7 => {
                    with.named.as_mut().unwrap().routes[0]
                        .env
                        .insert("LD_PRELOAD".into(), "evil.so".into());
                }
                8 => {
                    with.named.as_mut().unwrap().routes[0]
                        .env
                        .insert("QA_TOKEN".into(), "not\0a-variable".into());
                }
                _ => with.named.as_mut().unwrap().caller.identity.owner = "../foreign".into(),
            }
            let before = serde_json::to_value(&policy).unwrap();
            assert!(
                given(&mut policy, with, &machine).is_err(),
                "mutation {mutation}"
            );
            assert_eq!(
                serde_json::to_value(&policy).unwrap(),
                before,
                "mutation {mutation} changed policy"
            );
        }
    }

    /// A secret given to git: git's own configuration in the run's
    /// variables, sending what's asked of its host to the proxy with the
    /// placeholder; refused for a secret in a path, and beside a
    /// `GIT_CONFIG_` variable of the caller's.
    #[test]
    fn a_secret_given_to_git_is_gits_configuration() {
        use link_sandbox_client::{RunSecret, RunWith};
        use link_secret_contract::route::{Sealed, SecretHeader, SecretPath, SecretRule};
        let secret = |header: bool| RunSecret {
            secret: Sealed {
                name: "github-token".into(),
                value: "ghs_real".to_string().into(),
                placeholder: Default::default(),
                rule: SecretRule {
                    host: "github.com".into(),
                    paths: vec![SecretPath {
                        prefix: match header {
                            true => String::new(),
                            false => "x/{secret}/".into(),
                        },
                        rest: Default::default(),
                    }],
                    header: header.then(|| SecretHeader {
                        name: "Authorization".into(),
                        value: "Basic {secret}".into(),
                        basic: Some("x-access-token".into()),
                    }),
                    ..Default::default()
                },
            },
            url: "LNK_SEALED_GITHUB_TOKEN_URL".into(),
            placeholder: "GITHUB_TOKEN".into(),
            git: true,
        };
        let machine = spec::Machine {
            home: Path::new("/home/me"),
            settings: Path::new("/home/me/.lnk"),
            hidden: &[],
            spec: None,
            run_path: Default::default(),
        };
        let run = |with: RunWith| given(&mut proxied(), with, &machine);
        let env = run(RunWith {
            secrets: vec![secret(true)],
            ..Default::default()
        })
        .unwrap();
        let url = env["LNK_SEALED_GITHUB_TOKEN_URL"].clone();
        assert_eq!(env["GIT_CONFIG_COUNT"], "2");
        assert_eq!(env["GIT_CONFIG_KEY_0"], format!("url.{url}/.insteadOf"));
        assert_eq!(env["GIT_CONFIG_VALUE_0"], "https://github.com/");
        assert_eq!(env["GIT_CONFIG_KEY_1"], format!("http.{url}/.extraHeader"));
        let header = &env["GIT_CONFIG_VALUE_1"];
        assert!(header.starts_with("Authorization: Basic "), "{header}");
        assert!(!env.values().any(|v| v.contains("ghs_real")), "{env:?}");
        let refused = run(RunWith {
            secrets: vec![secret(false)],
            ..Default::default()
        });
        assert!(refused.is_err());
        let refused = run(RunWith {
            env: [("GIT_CONFIG_COUNT".to_string(), "0".to_string())].into(),
            secrets: vec![secret(true)],
            ..Default::default()
        });
        assert!(refused.unwrap_err().to_string().contains("GIT_CONFIG_"));
    }
}
