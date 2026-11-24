//! Native provider carriers retain the caller's approved exit, key checks and server-tool filter.
use link_format::WithinExt;
use link_model_contract::{
    chat,
    route::{
        HostedKey,
        actions::{Api, Backend},
    },
};
use link_secretive::Duplicate;
use std::{path::PathBuf, time::Duration};

pub(super) struct Endpoint {
    pub model: chat::client::Model,
    relay: Option<Relay>,
}
struct Relay {
    socket: PathBuf,
    serving: tokio::task::JoinHandle<()>,
}
impl Drop for Relay {
    fn drop(&mut self) {
        self.serving.abort();
        let _ = std::fs::remove_file(&self.socket);
    }
}
impl Endpoint {
    pub fn client(&self) -> anyhow::Result<reqwest::Client> {
        if let Some(relay) = &self.relay {
            // This carrier belongs to this grant and lives only as long as
            // its endpoint. Never pool it with another grant's relay.
            return Ok(client().unix_socket(relay.socket.clone()).build()?);
        }
        static LOCAL: std::sync::LazyLock<reqwest::Client> =
            std::sync::LazyLock::new(|| client().build().expect("static loopback client is valid"));
        Ok(LOCAL.clone())
    }
}

/// Both carriers use plain HTTP: loopback, or an already proved Unix
/// socket. The hosted relay owns TLS to its provider, outside this client.
fn client() -> reqwest::ClientBuilder {
    link_fetch::install_crypto_provider();
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .tls_certs_only(std::iter::empty())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(600))
}
/// Where `grant`'s model is reached, with the key the store grants
/// `service` when the grant hands none.
pub(super) async fn target(grant: &Api, service: &str, via: &str) -> anyhow::Result<Endpoint> {
    match &grant.backend {
        Backend::Local { port } => Ok(Endpoint {
            model: chat::client::Model {
                provider: "local".into(),
                base_url: Some(format!("http://127.0.0.1:{port}")),
                name: Some(grant.name.clone()),
                ..Default::default()
            },
            relay: None,
        }),
        Backend::Hosted { hosted } => {
            let key = match &hosted.key {
                HostedKey::Handed(key) => key.duplicate(),
                named => {
                    let service = service.to_owned();
                    let named = named.duplicate();
                    tokio::task::spawn_blocking(move || super::super::granted_key(&service, &named))
                        .await??
                }
            };
            let key = hosted.with_key(key)?;
            let upstream = link_sandbox_route::dial::upstream(via)?;
            let socket = std::env::temp_dir().join(format!(
                "lnk-model-{}.sock",
                link_sandbox_route::url_secret()?
            ));
            let listener = link_sandbox_route::socket::listen_async(&socket, Default::default())?;
            let model = chat::client::Model {
                provider: key.provider.clone(),
                name: Some(grant.name.clone()),
                api_key: key.placeholder.duplicate(),
                base_url: Some(
                    if key.provider == "anthropic" {
                        "http://model/_link/model"
                    } else {
                        "http://model/_link/model/v1"
                    }
                    .into(),
                ),
            };
            let via = via.to_owned();
            let serving = tokio::spawn(async move {
                let mut calls = tokio::task::JoinSet::new();
                loop {
                    tokio::select! {
                        accepted=listener.accept()=>{
                            let Ok((mut stream,_))=accepted else {return;};let key=key.duplicate();let via=via.clone();
                            calls.spawn(async move {
                                let (head,rest)=tokio::time::timeout(link_sandbox_route::http::HEAD_TIMEOUT,link_sandbox_route::http::read_head(&mut stream)).await.within("no native request in time")??;
                                let counter=super::super::usage::Counter::new();
                                // The canonical action records its usage once, after its typed answer.
                                super::super::hosted::relay_to(link_sandbox_route::Unsaid(stream),&head,rest,&key,true,&super::super::KEPT,&via,&counter,|host| async move {link_sandbox_route::tls::connect(&host,upstream).await}).await
                            });
                        },
                        Some(_)=calls.join_next(),if !calls.is_empty()=>{},
                    }
                }
            });
            Ok(Endpoint {
                model,
                relay: Some(Relay { socket, serving }),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn local_actions_reuse_transport_and_read_fresh_answers() {
        use axum::{Json, Router, extract::ConnectInfo, routing::get};
        use std::{
            collections::BTreeMap,
            net::SocketAddr,
            sync::{Arc, Mutex},
        };

        let peers = Arc::new(Mutex::new(BTreeMap::new()));
        let seen = peers.clone();
        let app = Router::new().route(
            "/v1/models",
            get(move |ConnectInfo(peer): ConnectInfo<SocketAddr>| {
                let seen = seen.clone();
                async move {
                    let mut seen = seen.lock().unwrap();
                    let requests = seen.entry(peer).or_insert(0);
                    *requests += 1;
                    Json(serde_json::json!({"requests": requests}))
                }
            }),
        );
        let listener = link_test_kit::Held::loopback();
        let port = listener.address().port();
        let listener = listener.listener();
        listener.set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let serving = tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        let grant = Api {
            backend: Backend::Local { port },
            name: "test-model".into(),
            allow: None,
            fallback: false,
            transcribe: None,
            window: None,
            answer: None,
        };
        for expected in [1, 2] {
            // Each action gets a new endpoint; only the transport is shared.
            let endpoint = target(&grant, "agent", "direct").await.unwrap();
            let answer = endpoint
                .client()
                .unwrap()
                .get(chat::client::models_url(&endpoint.model).unwrap())
                .send()
                .await
                .unwrap()
                .bytes()
                .await
                .unwrap();
            let answer: serde_json::Value = serde_json::from_slice(&answer).unwrap();
            assert_eq!(answer["requests"], expected);
        }
        assert_eq!(peers.lock().unwrap().len(), 1);
        serving.abort();
    }

    #[tokio::test]
    async fn hosted_actions_use_the_proved_upstream_and_remove_the_private_carrier() {
        let proxy = link_test_kit::Held::loopback();
        let via = format!("http://{}", proxy.address());
        let proxy = proxy.listener();
        proxy.set_nonblocking(true).unwrap();
        let proxy = tokio::net::TcpListener::from_std(proxy).unwrap();
        let contacted = tokio::spawn(async move {
            let (mut stream, _) = proxy.accept().await.unwrap();
            let (head, _) = link_sandbox_route::http::read_head(&mut stream)
                .await
                .unwrap();
            assert!(head.starts_with(b"CONNECT api.openai.com:443 "));
            stream
                .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
        });
        let made = link_sandbox_route::Placeholders::new().unwrap();
        let secret = link_secretive::Secret::from("sk-proj-not-a-real-secret".to_string());
        let grant = Api {
            backend: Backend::Hosted {
                hosted: link_model_contract::route::Hosted::new(
                    &made,
                    "openai",
                    HostedKey::Handed(secret.duplicate()),
                    &secret,
                    false,
                ),
            },
            name: "test-model".into(),
            allow: None,
            fallback: false,
            transcribe: None,
            window: None,
            answer: None,
        };
        let endpoint = target(&grant, "agent", &via).await.unwrap();
        let socket = endpoint.relay.as_ref().unwrap().socket.clone();
        // The client's production deadlines bound this request. Observe the
        // actual refusal instead of racing it against a second startup timer.
        let response = endpoint
            .client()
            .unwrap()
            .get(chat::client::models_url(&endpoint.model).unwrap())
            .bearer_auth(endpoint.model.api_key.expose())
            .send()
            .await
            .unwrap();
        assert!(!response.status().is_success());
        contacted.await.unwrap();
        drop(endpoint);
        assert!(!socket.exists());
    }
}
