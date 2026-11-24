//! Only named sealed grants enter the run tool; values stay at the bridge.
use super::*;
use anyhow::ensure;
use link_run_tool_contract::context::Context;

pub(super) fn of(agent: &Settings) -> anyhow::Result<Option<Context>> {
    let sealed = agent.sealed_variables()?;
    if sealed.is_empty() {
        return Ok(None);
    }
    let variables = sealed
        .into_iter()
        .map(|(name, wired)| (name, wired.named))
        .collect();
    let route = route(variables, link_secret_contract::route::bridge()?)?;
    let context = Context {
        agent: agent.key(),
        owner: agent.identity().owner,
        keys: link_sandbox_binding::Keyring::here().dir().to_path_buf(),
        routes: vec![route],
    };
    context.check()?;
    Ok(Some(context))
}

fn route(
    variables: Vec<(String, link_secret_contract::route::Named)>,
    bridge: link_sandbox_route::Bridge,
) -> anyhow::Result<link_sandbox_route::Route> {
    let mut env = BTreeMap::new();
    for (name, named) in &variables {
        for (key, value) in [
            (name.clone(), named.placeholder.expose_owned()),
            (
                link_secret_contract::sealed_url_var(name),
                format!("{{proxy}}/_link/secret/{}", named.name),
            ),
        ] {
            ensure!(
                key != link_run_tool_contract::context::ENV && env.insert(key, value).is_none(),
                "sealed task variables collide"
            );
        }
    }
    let mut route = link_secret_contract::route::route(
        bridge,
        Vec::new(),
        variables.into_iter().map(|(_, named)| named).collect(),
    )?;
    route.env = env;
    route.check()?;
    route.names_only()?;
    Ok(route)
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_secret_contract::route::{Kept, Named, SecretHeader, SecretPath, SecretRule};
    fn named(variable: &str) -> Named {
        Named {
            name: link_secret_contract::proxy_name(variable),
            kept: Kept::Variable(variable.into()),
            placeholder: "a-start-specific-placeholder".to_string().into(),
            rule: SecretRule {
                host: "api.example.com".into(),
                paths: vec![SecretPath {
                    prefix: String::new(),
                    ..Default::default()
                }],
                header: Some(SecretHeader {
                    name: "Authorization".into(),
                    value: "Bearer {secret}".into(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        }
    }
    fn bridge() -> link_sandbox_route::Bridge {
        link_sandbox_route::Bridge {
            socket: "/outside/secret.sock".into(),
            start: Vec::new(),
        }
    }
    #[test]
    fn task_routes_name_grants_and_expand_the_tasks_own_proxy_url() {
        let route = route(vec![("QA_TOKEN".into(), named("QA_TOKEN"))], bridge()).unwrap();
        assert!(!route.handed);
        let given: link_secret_contract::route::Given =
            serde_json::from_str(route.given.expose()).unwrap();
        assert!(given.secrets.is_empty());
        assert_eq!(given.granted[0].kept, Kept::Variable("QA_TOKEN".into()));
        assert_eq!(route.env["QA_TOKEN"], "a-start-specific-placeholder");
        assert_eq!(
            route.env["LNK_SEALED_QA_TOKEN_URL"],
            "{proxy}/_link/secret/qa-token"
        );
        let context = Context {
            agent: "aaaaaaaaaaaa".into(),
            owner: "local-link-aaaaaaaaaaaa".into(),
            keys: "/outside/keys".into(),
            routes: vec![route],
        };
        assert!(
            context
                .check_variables(&[("QA_TOKEN".into(), "cache override".into())])
                .is_err()
        );
        assert!(
            context
                .check_variables(&[("CARGO_HOME".into(), "cache".into())])
                .is_ok()
        );
        assert!(
            context
                .check_variables(&[(link_run_tool_contract::context::ENV.into(), "leak".into())])
                .is_err()
        );
    }
    #[test]
    fn a_sealed_variable_cannot_replace_another_service_url_or_the_context() {
        assert!(
            route(
                vec![
                    ("QA_TOKEN".into(), named("QA_TOKEN")),
                    (
                        "LNK_SEALED_QA_TOKEN_URL".into(),
                        named("LNK_SEALED_QA_TOKEN_URL")
                    )
                ],
                bridge()
            )
            .is_err()
        );
        assert!(
            route(
                vec![(
                    link_run_tool_contract::context::ENV.into(),
                    named("LNK_RUN_CONTEXT")
                )],
                bridge()
            )
            .is_err()
        );
    }
}
