//! Grants for the typed model API. A request cannot supply a backend or a key.
use super::Hosted;
use link_sandbox_route::{Bridge, Route};
use link_secretive::Duplicate;
use serde::{Deserialize, Serialize};
#[derive(link_secretive::Duplicate, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Backend {
    Hosted {
        #[duplicate]
        hosted: Hosted,
    },
    Local {
        port: u16,
    },
}
#[derive(link_secretive::Duplicate, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Api {
    #[serde(default)]
    pub transcribe: Option<String>,
    #[serde(default)]
    pub window: Option<u32>,
    #[serde(default)]
    pub answer: Option<u32>,
    #[duplicate]
    pub backend: Backend,
    pub name: String,
    pub allow: Option<Vec<String>>,
    #[serde(default)]
    pub fallback: bool,
}
impl Api {
    pub fn name<'a>(&'a self, requested: Option<&'a str>) -> anyhow::Result<&'a str> {
        let name = requested.unwrap_or(&self.name);
        anyhow::ensure!(crate::show::plain_name(name), "invalid model name");
        anyhow::ensure!(
            self.allow
                .as_ref()
                .is_none_or(|allow| allow.iter().any(|n| n == name)),
            "model is outside the grant"
        );
        Ok(name)
    }
}
/// Registry-filtered routes, each bound to one proved caller's endpoint.
pub fn routes(bridge: Bridge, api: Api) -> anyhow::Result<Vec<Route>> {
    let handed = matches!(&api.backend,Backend::Hosted{hosted} if matches!(hosted.key,super::HostedKey::Handed(_)));
    let given = serde_json::to_string(&api)?;
    let mut routes = Vec::new();
    for (path, action) in crate::actions() {
        if action.allows(
            link_component::actions::Caller::Program,
            link_component::actions::Surface::Route,
        ) {
            let prefix = format!("/_link/{}", path.replace(' ', "/"));
            let prefix = if api.fallback {
                prefix.replacen("/_link/model/", "/_link/model/fallback/", 1)
            } else {
                prefix
            };
            routes.push(Route {
                name: format!(
                    "model-action-{}{}",
                    if api.fallback { "fallback-" } else { "" },
                    path.trim_start_matches("model ").replace(' ', "-")
                ),
                prefix,
                bridge: bridge.clone(),
                given: given.clone().into(),
                needs_url_secret: true,
                handed,
                ..Route::default()
            });
        }
    }
    if let Some(route) = routes.first_mut() {
        route.env.insert(
            if api.fallback {
                "LNK_FALLBACK_MODEL_URL"
            } else {
                "LNK_MODEL_URL"
            }
            .into(),
            if api.fallback {
                "{proxy}/_link/url/{secret}/model/fallback"
            } else {
                "{proxy}/_link/url/{secret}/model"
            }
            .into(),
        );
    }
    Ok(routes)
}
pub fn url(port: u16, secret: &str, fallback: bool) -> String {
    let suffix = if fallback { "/fallback" } else { "" };
    format!("http://127.0.0.1:{port}/_link/url/{secret}/model{suffix}")
}
