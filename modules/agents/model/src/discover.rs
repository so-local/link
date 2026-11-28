//! Finding model runtimes that are already running on this machine.
//!
//! Each runtime Link knows has a default port. Discovery asks each of them
//! for `GET /v1/models` and keeps the ones that answer with an
//! OpenAI-style model list, so `lnk model list` and `lnk model serve --model <model>` work
//! with whatever the user already runs, without installing anything. The
//! addresses added with `lnk model runtime add` are asked the same way.

use std::time::Duration;

use crate::BoxError;
use crate::added::{self, Added};
use futures_util::future::join_all;

/// Runtimes found at their default ports: (name, base URL).
pub const KNOWN_RUNTIMES: &[(&str, &str)] = &[
    ("Ollama", "http://localhost:11434"),
    ("LM Studio", "http://localhost:1234"),
    ("llama.cpp", "http://localhost:8080"),
    ("vLLM", "http://localhost:8000"),
    ("Jan", "http://localhost:1337"),
];

/// A runtime that answered, and the models it serves.
#[derive(Clone, Debug, PartialEq)]
pub struct Runtime {
    /// What usually runs at this address, e.g. `Ollama`. A hint, not a
    /// promise: any OpenAI-compatible server on that port is listed.
    pub name: String,
    pub base_url: String,
    pub models: Vec<String>,
}

/// The runtimes answering on this machine: at their default ports, then
/// at the addresses added with `lnk model runtime add`.
pub async fn discover() -> Vec<Runtime> {
    discover_at(&candidates(&added::load())).await
}

/// A runtime as `lnk model runtime list` lists it: one that answered, or
/// one added with `lnk model runtime add` that didn't, with no models.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    pub runtime: Runtime,
    /// Whether `lnk model runtime add` added it.
    pub added: bool,
    /// Whether it answered with a model list.
    pub answering: bool,
}

/// Every runtime here: each that answered (`found`), then each address in
/// `added` that nothing answered at, so what was added is listed until
/// it's removed, running or not.
pub fn inventory(found: Vec<Runtime>, added: &[Added]) -> Vec<Listed> {
    let silent: Vec<_> = added
        .iter()
        .filter(|a| !found.iter().any(|r| added::same(&a.base_url, &r.base_url)))
        .map(|a| Listed {
            runtime: Runtime {
                name: a.name.clone(),
                base_url: a.base_url.trim_end_matches('/').to_string(),
                models: Vec::new(),
            },
            added: true,
            answering: false,
        })
        .collect();
    found
        .into_iter()
        .map(|runtime| Listed {
            added: added
                .iter()
                .any(|a| added::same(&a.base_url, &runtime.base_url)),
            answering: true,
            runtime,
        })
        .chain(silent)
        .collect()
}

/// The addresses discovery asks: the default ports, then `added`.
pub fn candidates(added: &[Added]) -> Vec<(String, String)> {
    // One added at a runtime's default port is that runtime, asked once.
    let known = |url: &str| KNOWN_RUNTIMES.iter().any(|(_, at)| added::same(at, url));
    KNOWN_RUNTIMES
        .iter()
        .map(|&(name, url)| (name.to_string(), url.to_string()))
        .chain(
            added
                .iter()
                .filter(|a| !known(&a.base_url))
                .map(|a| (a.name.clone(), a.base_url.clone())),
        )
        .collect()
}

/// The runtimes among `candidates` (name, base URL) that answer with a
/// model list, in the order given. All are asked at once.
pub async fn discover_at(candidates: &[(String, String)]) -> Vec<Runtime> {
    let client = client();
    let probes = candidates
        .iter()
        .map(|(name, url)| probe(&client, name, url));
    join_all(probes).await.into_iter().flatten().collect()
}

/// The runtime at `base_url`, if it serves an OpenAI-style model list.
pub async fn probe_one(name: &str, base_url: &str) -> Option<Runtime> {
    probe(&client(), name, base_url).await
}

async fn probe(client: &reqwest::Client, name: &str, base_url: &str) -> Option<Runtime> {
    let base_url = base_url.trim_end_matches('/');
    let response = client
        .get(format!("{base_url}/v1/models"))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = read_capped(response, MOST).await?;
    let list: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let models = list
        .get("data")?
        .as_array()?
        .iter()
        .filter_map(|entry| entry.get("id")?.as_str().map(str::to_string))
        .collect();
    Some(Runtime {
        name: name.to_string(),
        base_url: base_url.to_string(),
        models,
    })
}

/// The most of a model list read: model lists are small, and anything
/// bigger isn't one.
const MOST: usize = 1 << 20;

/// `response`'s body, read a chunk at a time, or none once it passes
/// `most`: what's past it is never held.
async fn read_capped(mut response: reqwest::Response, most: usize) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len() + chunk.len() > most {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    Some(bytes)
}

fn client() -> reqwest::Client {
    link_fetch::install_crypto_provider();
    reqwest::Client::builder()
        // Only this machine is probed (`added` refuses any other address);
        // never through a proxy.
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_millis(500))
        .timeout(Duration::from_secs(3))
        .build()
        .expect("static reqwest config is valid")
}

/// Where `model` is served: the runtime and the model's exact id.
///
/// An exact id wins. Otherwise a name without a tag matches the one model
/// with that name and any tag, so `deepseek-r1` finds `deepseek-r1:7b`
/// (Ollama's ids carry tags). Two candidates are an error that lists them.
pub fn find_model<'a>(
    runtimes: &'a [Runtime],
    model: &str,
) -> Result<(&'a Runtime, String), BoxError> {
    let all = || {
        runtimes
            .iter()
            .flat_map(|r| r.models.iter().map(move |m| (r, m)))
    };
    let exact: Vec<_> = all().filter(|(_, m)| *m == model).collect();
    let matches = if exact.is_empty() {
        all()
            .filter(|(_, m)| m.split_once(':').is_some_and(|(base, _)| base == model))
            .collect()
    } else {
        exact
    };
    match matches.as_slice() {
        [(runtime, id)] => Ok((runtime, id.to_string())),
        [] => Err(not_found(runtimes, model).into()),
        several => {
            let options: Vec<_> = several
                .iter()
                .map(|(r, m)| format!("{m} ({} at {})", r.name, r.base_url))
                .collect();
            Err(format!(
                "{model:?} matches several models: {}. Name one exactly, or pass --upstream",
                options.join(", ")
            )
            .into())
        }
    }
}

fn not_found(runtimes: &[Runtime], model: &str) -> String {
    if runtimes.is_empty() {
        return format!("no model named {model:?}: {}", nothing_running());
    }
    format!("no model named {model:?} is running; lnk model list lists the ones that are")
}

/// The explanation when discovery finds nothing.
pub fn nothing_running() -> String {
    let checked: Vec<_> = KNOWN_RUNTIMES
        .iter()
        .map(|(name, url)| format!("{name} {}", url.trim_start_matches("http://")))
        .collect();
    let added: Vec<_> = added::load()
        .into_iter()
        .map(|a| a.base_url.trim_start_matches("http://").to_string())
        .collect();
    let added = if added.is_empty() {
        String::new()
    } else {
        format!("; added: {}", added.join(", "))
    };
    format!(
        "no model runtime found on this machine (checked {}{added}). Start one, \
         install one with lnk model install, or add yours with lnk model runtime add <url>",
        checked.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime(name: &str, models: &[&str]) -> Runtime {
        Runtime {
            name: name.into(),
            base_url: format!("http://{name}"),
            models: models.iter().map(|m| m.to_string()).collect(),
        }
    }

    /// A model list served at `held`, as `body`, chunked with no length
    /// said: one connection, answered.
    fn serve_once(held: link_test_kit::net::Held, body: Vec<u8>) -> std::thread::JoinHandle<()> {
        use std::io::{Read, Write};
        let listener = held.listener();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut asked = [0u8; 1024];
            let _ = stream.read(&mut asked);
            let mut out = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                            transfer-encoding: chunked\r\nconnection: close\r\n\r\n"
                .to_vec();
            for chunk in body.chunks(64 * 1024) {
                out.extend(format!("{:x}\r\n", chunk.len()).as_bytes());
                out.extend(chunk);
                out.extend(b"\r\n");
            }
            out.extend(b"0\r\n\r\n");
            let _ = stream.write_all(&out);
        })
    }

    /// A model list is read a chunk at a time against its cap: one past
    /// 1 MiB, with no length said, isn't one, and a small one is.
    #[tokio::test]
    async fn discovery_reads_at_most_one_mib() {
        let held = link_test_kit::net::Held::loopback();
        let url = format!("http://{}", held.address());
        let big = format!("{{\"data\": [{{\"id\": \"{}\"}}]}}", "x".repeat(MOST + 1));
        let server = serve_once(held, big.into_bytes());
        assert!(probe_one("big", &url).await.is_none());
        server.join().unwrap();
        let held = link_test_kit::net::Held::loopback();
        let url = format!("http://{}", held.address());
        let server = serve_once(held, br#"{"data": [{"id": "qwen3"}]}"#.to_vec());
        let found = probe_one("small", &url).await.unwrap();
        assert_eq!(found.models, ["qwen3"]);
        server.join().unwrap();
    }

    /// An address added at a runtime's default port is that runtime,
    /// asked once, under its name.
    #[test]
    fn an_added_default_port_is_asked_once() {
        let added = [Added {
            name: "mine".into(),
            base_url: "http://127.0.0.1:11434".into(),
        }];
        let asked = candidates(&added);
        assert_eq!(asked.len(), KNOWN_RUNTIMES.len());
        assert!(asked.iter().any(|(name, _)| name == "Ollama"));
    }

    /// Every added address is listed, answering or not; one that
    /// answers is listed once, as found, marked added.
    #[test]
    fn every_added_runtime_is_listed_answering_or_not() {
        let added = [
            Added {
                name: "mine".into(),
                base_url: "http://localhost:5000".into(),
            },
            Added {
                name: "quiet".into(),
                base_url: "http://localhost:5001".into(),
            },
        ];
        let found = vec![
            Runtime {
                name: "mine".into(),
                base_url: "http://localhost:5000".into(),
                models: vec!["qa-model".into()],
            },
            runtime("ollama", &["qwen3:8b"]),
        ];
        let listed = inventory(found, &added);
        let row = |name: &str| listed.iter().find(|l| l.runtime.name == name).unwrap();
        assert_eq!(listed.len(), 3, "{listed:?}");
        assert!(row("mine").added && row("mine").answering);
        assert_eq!(row("mine").runtime.models, ["qa-model"]);
        assert!(!row("ollama").added && row("ollama").answering);
        let quiet = row("quiet");
        assert!(quiet.added && !quiet.answering && quiet.runtime.models.is_empty());
        assert_eq!(quiet.runtime.base_url, "http://localhost:5001");
    }

    #[test]
    fn models_are_found_by_id_or_by_name_without_a_tag() {
        let runtimes = [
            runtime("ollama", &["deepseek-r1:7b", "qwen3:latest", "qwen3:32b"]),
            runtime("vllm", &["my-org/finetune", "qwen3"]),
        ];
        let found = |model| {
            find_model(&runtimes, model)
                .map(|(r, id)| (r.name.clone(), id))
                .map_err(|e| e.to_string())
        };
        assert_eq!(
            found("deepseek-r1").unwrap(),
            ("ollama".into(), "deepseek-r1:7b".into())
        );
        assert_eq!(
            found("qwen3:32b").unwrap(),
            ("ollama".into(), "qwen3:32b".into())
        );
        // An exact id beats tagged ones.
        assert_eq!(found("qwen3").unwrap(), ("vllm".into(), "qwen3".into()));
        assert_eq!(found("my-org/finetune").unwrap().0, "vllm");
        assert!(found("llama3").unwrap_err().contains("lnk model list"));

        let ambiguous = [runtime("ollama", &["qwen3:8b", "qwen3:32b"])];
        let err = find_model(&ambiguous, "qwen3").unwrap_err().to_string();
        assert!(
            err.contains("qwen3:8b") && err.contains("qwen3:32b"),
            "{err}"
        );

        let err = find_model(&[], "qwen3").unwrap_err().to_string();
        assert!(err.contains("no model runtime found"), "{err}");
    }
}
