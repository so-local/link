//! Managed models: installing Ollama and the models it runs.
//!
//! Link wraps Ollama: it installs the official Ollama
//! when it's missing (its signed app on macOS, its own install script on
//! Linux), starts it, and pulls and deletes models through Ollama's API.
//! Link keeps no catalog of its own: a model is whatever Ollama's library
//! calls it (`qwen3:8b`).

use std::{
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{bail, ensure};
use futures_util::StreamExt;

use crate::discover;
use link_format::{
    Address, Name, Note, Out, Question, Refuser, Report, Sentence, WithinExt, cmd, phrase, sentence,
};

/// `report`, a failure to install Ollama, with its way on: a runtime of
/// your own (`lnk model install` declares it in its `fail`).
fn your_runtime(report: Report) -> Report {
    report.next(
        cmd!("lnk model runtime add <url>"),
        "use a runtime of yours instead",
    )
}

/// Where Ollama listens once it runs.
pub const OLLAMA: &str = "http://localhost:11434";

/// Ollama's official downloads.
fn download_url(os: &str) -> &'static str {
    static RECORD: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    RECORD.get_or_init(|| {
        serde_json::from_str(include_str!("../externals.json")).expect("owned external inventory")
    })["ollama"]["platforms"][os]["url"]
        .as_str()
        .expect("declared installer platform")
}

/// The model Link installs when you don't name one: what runs well here.
#[derive(Debug, Clone, PartialEq)]
pub struct Recommendation {
    pub model: String,
    /// Its download, roughly, e.g. `5.2 GB`.
    pub size: String,
    /// Why this one, e.g. `this Mac has 16 GB of memory`.
    pub why: String,
}

/// The model to install on this machine, or why there isn't one: Link
/// recommends a local model only where one runs well, on Apple Silicon
/// (its memory is the GPU's) with enough of it.
pub fn recommend() -> Result<Recommendation, String> {
    if !(cfg!(target_os = "macos") && cfg!(target_arch = "aarch64")) {
        return Err("Link recommends a local model only on Apple Silicon Macs; \
                    elsewhere your agent is better off with an API key"
            .into());
    }
    let memory = memory_gb().ok_or("can't tell how much memory this Mac has")?;
    recommend_for(memory)
}

/// [`recommend`] for a Mac with `memory` GB: the largest Qwen3 that leaves
/// room for everything else. Qwen3 answers in tools, which agents need.
pub fn recommend_for(memory: u64) -> Result<Recommendation, String> {
    let (model, size) = match memory {
        32.. => ("qwen3:14b", "9.3 GB"),
        16.. => ("qwen3:8b", "5.2 GB"),
        8.. => ("qwen3:4b", "2.5 GB"),
        _ => {
            return Err(format!(
                "this Mac has {memory} GB of memory, too little for a useful local model"
            ));
        }
    };
    Ok(Recommendation {
        model: model.into(),
        size: size.into(),
        why: format!("this Mac has {memory} GB of memory"),
    })
}

/// This machine's memory, in whole GB.
fn memory_gb() -> Option<u64> {
    let bytes: u64 = if cfg!(target_os = "macos") {
        let output = link_secretive::process::command("/usr/sbin/sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()?;
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .ok()?
    } else {
        let info = std::fs::read_to_string("/proc/meminfo").ok()?;
        let kb: u64 = info
            .lines()
            .find_map(|l| l.strip_prefix("MemTotal:"))?
            .trim()
            .trim_end_matches("kB")
            .trim()
            .parse()
            .ok()?;
        kb * 1024
    };
    Some((bytes + (1 << 29)) >> 30)
}

/// Ollama's program or app on this machine, if it's installed.
pub fn ollama_installed() -> Option<PathBuf> {
    let home = link_files::home_dir();
    let apps = [
        PathBuf::from("/Applications/Ollama.app"),
        home.join("Applications/Ollama.app"),
    ];
    if cfg!(target_os = "macos")
        && let Some(app) = apps.into_iter().find(|a| a.is_dir())
    {
        return Some(app);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("ollama"))
        .find(|p| p.is_file())
}

/// Whether Ollama answers at [`OLLAMA`].
pub async fn ollama_running() -> bool {
    discover::probe_one("Ollama", OLLAMA).await.is_some()
}

/// Starts an installed Ollama that isn't running, and waits until it
/// answers. Best effort: false if it doesn't within 30 s.
pub async fn start_ollama(installed: &Path) -> bool {
    start_ollama_with(
        installed,
        Path::new("systemctl"),
        OLLAMA,
        Duration::from_secs(30),
    )
    .await
}

/// The same start boundary with owned programs and readiness for isolated tests.
async fn start_ollama_with(
    installed: &Path,
    controller: &Path,
    endpoint: &str,
    timeout: Duration,
) -> bool {
    if cfg!(target_os = "macos") && installed.extension().is_some_and(|e| e == "app") {
        // The app runs the server, and starts it again at login.
        let _ = link_secretive::process::command("/usr/bin/open")
            .args(["-g", "-a"])
            .arg(installed)
            .status();
    } else {
        // Ollama's Linux installer runs it as a system service; without
        // one (a manual install), run the server ourselves.
        let service = link_secretive::process::command(controller)
            .args(["is-enabled", "--quiet", "ollama"])
            .status()
            .is_ok_and(|s| s.success());
        if service {
            // Keep systemctl's normal authorization and terminal interaction.
            // A refusal must not fall back to a competing unmanaged server.
            if !link_secretive::process::command(controller)
                .args(["start", "ollama"])
                .status()
                .is_ok_and(|status| status.success())
            {
                return false;
            }
        } else {
            let mut command = link_secretive::process::command(installed);
            command
                .arg("serve")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            #[cfg(unix)]
            std::os::unix::process::CommandExt::process_group(&mut command, 0);
            let _ = command.spawn();
        }
    }
    wait_for_ollama(endpoint, timeout).await
}

#[expect(clippy::disallowed_methods, reason = "polls Ollama until it answers")]
async fn wait_for_ollama(endpoint: &str, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if discover::probe_one("Ollama", endpoint).await.is_some() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    false
}

/// Makes sure Ollama is installed and running: installs it (asking first,
/// unless `yes`) and starts it.
pub async fn ensure_ollama(out: &mut Out, yes: bool) -> Result<(), Report> {
    if ollama_running().await {
        return Ok(());
    }
    if let Some(installed) = ollama_installed() {
        return start(out, &installed).await;
    }
    let from = match cfg!(target_os = "macos") {
        true => download_url("macos"),
        false => download_url("linux"),
    };
    let sure = out.ask(
        Question::confirm(
            sentence!(
                "Ollama runs local models and isn't installed. Install it from {}?",
                Address::new(from)
            ),
            true,
        )
        .yes(yes),
    )?;
    if !sure {
        return Err(your_runtime(Report::refusal(
            Refuser::Person,
            sentence!("Not installing Ollama."),
        )));
    }
    let adding =
        |why: anyhow::Error| your_runtime(Report::error(Sentence::adopt(&why.to_string())));
    if cfg!(target_os = "macos") {
        install_mac(out).await.map_err(adding)?;
    } else if cfg!(target_os = "linux") {
        install_linux(out).await.map_err(adding)?;
    } else {
        return Err(adding(anyhow::anyhow!(
            "Link can't install Ollama on this system; install it from ollama.com"
        )));
    }
    let installed = ollama_installed()
        .within("Ollama's installer finished, but no Ollama")
        .map_err(adding)?;
    start(out, &installed).await
}

/// Starts an installed Ollama, a wait while it does.
async fn start(out: &mut Out, installed: &Path) -> Result<(), Report> {
    let started = {
        let _wait = out.activity(phrase!("Starting Ollama"));
        start_ollama(installed).await
    };
    match started {
        true => Ok(()),
        false => Err(Report::error(sentence!(
            "Ollama is installed ({}) but didn't start.",
            link_format::Path::new(installed)
        ))
        .next(cmd!("ollama serve"), "start it yourself, then try again")),
    }
}

/// Ollama's signed app, into /Applications (else ~/Applications).
async fn install_mac(out: &mut Out) -> anyhow::Result<()> {
    let dir = temp_dir("ollama-app")?;
    let zip = dir.join("Ollama-darwin.zip");
    download_to(out, download_url("macos"), &zip).await?;
    run(link_secretive::process::command("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(&zip)
        .arg(&dir))
    .within("unpacking Ollama")?;
    let app = dir.join("Ollama.app");
    // Gatekeeper's own check: signed by its developer and notarized by
    // Apple, as opening it from the Finder would require.
    run(link_secretive::process::command("/usr/sbin/spctl")
        .args(["--assess", "--type", "execute"])
        .arg(&app))
    .within("Ollama's download isn't signed and notarized; not installing it")?;
    let system = Path::new("/Applications");
    let target = if writable(system) {
        system.join("Ollama.app")
    } else {
        link_files::home_dir().join("Applications/Ollama.app")
    };
    run(link_secretive::process::command("/usr/bin/ditto")
        .arg(&app)
        .arg(&target))
    .within(format!("copying Ollama to {}", target.display()))?;
    let _ = std::fs::remove_dir_all(&dir);
    out.block(Note(sentence!(
        "Installed Ollama in {}.",
        link_format::Path::new(target)
    )));
    Ok(())
}

/// Ollama's own install script, which asks for sudo and sets Ollama up
/// as a system service.
async fn install_linux(out: &mut Out) -> anyhow::Result<()> {
    let dir = temp_dir("ollama-script")?;
    let script = dir.join("install.sh");
    download_to(out, download_url("linux"), &script).await?;
    out.block(Note(sentence!(
        "Running Ollama's installer ({}); it may ask for your password.",
        Address::new(download_url("linux"))
    )));
    // Its prompts are on this terminal.
    out.hand_over();
    let status = link_secretive::process::command("sh")
        .arg(&script)
        .status()
        .within("running Ollama's installer")?;
    let _ = std::fs::remove_dir_all(&dir);
    ensure!(status.success(), "Ollama's installer failed ({status})");
    Ok(())
}

fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!("lnk-probe-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// A folder of this run's own: another user can't have made it first, to
/// swap the installer before it runs.
fn temp_dir(name: &str) -> anyhow::Result<PathBuf> {
    Ok(link_files::private_temp_dir(name)?)
}

async fn download_to(out: &mut Out, url: &str, path: &Path) -> anyhow::Result<()> {
    let mut res = http(Duration::from_secs(60))?
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .within(format!("downloading {url}"))?;
    let mut file = std::fs::File::create(path)?;
    let mut bar = out.bar(
        phrase!("Downloading {}", Address::new(url)),
        res.content_length().unwrap_or(0),
    );
    while let Some(chunk) = res.chunk().await.within(format!("downloading {url}"))? {
        file.write_all(&chunk)?;
        bar.add(chunk.len() as u64);
    }
    Ok(())
}

fn http(read_timeout: Duration) -> anyhow::Result<reqwest::Client> {
    link_fetch::install_crypto_provider();
    Ok(reqwest::Client::builder()
        .user_agent(concat!("lnk/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(read_timeout)
        .build()?)
}

/// Pulls `model` into the Ollama at `base`, its progress a bar on `out`.
pub async fn pull(out: &mut Out, base: &str, model: &str) -> anyhow::Result<()> {
    // Layers can take a while to verify: no short read timeout.
    let res = http(Duration::from_secs(600))?
        .post(format!("{base}/api/pull"))
        .header("content-type", "application/json")
        .body(serde_json::json!({"model": model, "stream": true}).to_string())
        .send()
        .await
        .within(format!("asking Ollama at {base} for {model}"))?;
    ensure!(
        res.status().is_success(),
        "Ollama refused to pull {model}: {}",
        res.text().await.unwrap_or_default().trim()
    );
    let mut progress = Progress::new(model);
    let mut bar = out.bar(phrase!("Pulling {}", Name::new("model", model)), 0);
    let mut stream = res.bytes_stream();
    let mut line = Vec::new();
    while let Some(chunk) = stream.next().await {
        for byte in chunk.within("downloading from Ollama")? {
            if byte != b'\n' {
                line.push(byte);
                continue;
            }
            progress.update(&serde_json::from_slice(&line).unwrap_or_default())?;
            bar.status(&progress.shown);
            if progress.total > 0 {
                bar.set(progress.completed, progress.total);
            }
            line.clear();
        }
    }
    progress.finish()
}

/// What `/api/pull`'s lines add up to: what it's doing, and its bytes
/// so far of a total that grows as each layer is found.
struct Progress {
    model: String,
    /// Each layer's (total, completed) bytes.
    layers: std::collections::BTreeMap<String, (u64, u64)>,
    shown: String,
    total: u64,
    completed: u64,
    done: bool,
}

impl Progress {
    fn new(model: &str) -> Self {
        Self {
            model: model.into(),
            layers: Default::default(),
            shown: String::new(),
            total: 0,
            completed: 0,
            done: false,
        }
    }

    fn update(&mut self, line: &serde_json::Value) -> anyhow::Result<()> {
        if let Some(error) = line["error"].as_str() {
            bail!(
                "Ollama couldn't pull {}: {}",
                self.model,
                link_format::clean(error)
            );
        }
        let status = line["status"].as_str().unwrap_or_default();
        if status == "success" {
            self.done = true;
            return Ok(());
        }
        if let (Some(digest), Some(total)) = (line["digest"].as_str(), line["total"].as_u64()) {
            let completed = line["completed"].as_u64().unwrap_or(0);
            self.layers.insert(digest.into(), (total, completed));
        }
        (self.total, self.completed) = self
            .layers
            .values()
            .fold((0, 0), |(t, c), (total, done)| (t + total, c + done));
        self.shown = if status.starts_with("pulling") && status != "pulling manifest" {
            format!("Downloading {}", self.model)
        } else {
            capitalized(&link_format::clean(status))
        };
        Ok(())
    }

    fn finish(self) -> anyhow::Result<()> {
        ensure!(
            self.done,
            "Ollama stopped before {} was complete; run the same command again to resume",
            self.model
        );
        Ok(())
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Deletes `model` from the Ollama at `base`.
pub async fn delete(base: &str, model: &str) -> anyhow::Result<()> {
    let res = http(Duration::from_secs(60))?
        .delete(format!("{base}/api/delete"))
        .header("content-type", "application/json")
        .body(serde_json::json!({"model": model}).to_string())
        .send()
        .await
        .within(format!("asking Ollama at {base} to delete {model}"))?;
    ensure!(
        res.status().is_success(),
        "Ollama couldn't delete {model}: {}",
        res.text().await.unwrap_or_default().trim()
    );
    Ok(())
}

fn run(command: &mut std::process::Command) -> anyhow::Result<()> {
    let output = command.output().within(format!("running {command:?}"))?;
    ensure!(
        output.status.success(),
        "{command:?} failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn an_enabled_stopped_service_is_started_and_awaited_without_manual_fallback() {
        use axum::{Json, http::StatusCode, routing::get};
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let dir = link_test_kit::TempDir::new("ollama-service-start");
        let controller = dir.join("systemctl");
        let installed = dir.join("ollama");
        link_test_kit::write_program_with_mode(
            &controller,
            concat!(
                "#!/bin/sh\n",
                "dir=$(dirname \"$0\")\n",
                "printf '%s\\n' \"$*\" >>\"$dir/calls\"\n",
                "case \"$1\" in\n",
                "is-enabled) test ! -f \"$dir/manual-only\";;\n",
                "start) test ! -f \"$dir/deny\" || exit 1; touch \"$dir/started\";;\n",
                "*) exit 2;;\nesac\n",
            ),
            0o700,
        )
        .unwrap();
        link_test_kit::write_program_with_mode(
            &installed,
            "#!/bin/sh\ndir=$(dirname \"$0\")\ntouch \"$dir/manual\" \"$dir/started\"\n",
            0o700,
        )
        .unwrap();
        let started = dir.join("started");
        let attempts = Arc::new(AtomicUsize::new(0));
        let probes = attempts.clone();
        let ready = started.clone();
        let app = axum::Router::new().route(
            "/v1/models",
            get(move || {
                let attempt = probes.fetch_add(1, Ordering::SeqCst);
                let available = ready.is_file() && attempt >= 2;
                async move {
                    (
                        if available {
                            StatusCode::OK
                        } else {
                            StatusCode::SERVICE_UNAVAILABLE
                        },
                        Json(serde_json::json!({"data":[{"id":"qa-owned-ollama"}]})),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let serving = tokio::spawn(async move { axum::serve(listener, app).await });

        assert!(
            start_ollama_with(&installed, &controller, &endpoint, Duration::from_secs(3)).await
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("calls")).unwrap(),
            "is-enabled --quiet ollama\nstart ollama\n"
        );
        assert!(attempts.load(Ordering::SeqCst) >= 3);
        assert!(!dir.join("manual").exists());

        // Denied normal service authorization never starts a competing process,
        // even if another process happens to answer at the readiness endpoint.
        std::fs::write(dir.join("deny"), "").unwrap();
        std::fs::remove_file(dir.join("calls")).unwrap();
        let before = attempts.load(Ordering::SeqCst);
        assert!(
            !start_ollama_with(&installed, &controller, &endpoint, Duration::from_secs(3)).await
        );
        assert_eq!(attempts.load(Ordering::SeqCst), before);
        assert!(!dir.join("manual").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("calls")).unwrap(),
            "is-enabled --quiet ollama\nstart ollama\n"
        );

        // A manual installation still launches its own serve command.
        std::fs::remove_file(dir.join("deny")).unwrap();
        std::fs::remove_file(&started).unwrap();
        std::fs::write(dir.join("manual-only"), "").unwrap();
        assert!(
            start_ollama_with(&installed, &controller, &endpoint, Duration::from_secs(3)).await
        );
        assert!(dir.join("manual").exists());
        serving.abort();
    }

    #[test]
    fn a_failed_ollama_install_gives_only_nexts_model_install_declares() {
        // Its declared fail, each an action, as the command a person types.
        let actions = link_component::actions::parse(include_str!("../actions.toml")).unwrap();
        let declared = &actions["model install"].fail;
        let report = your_runtime(Report::error(link_format::sentence!(
            "Ollama didn't install."
        )));
        assert!(!report.next.0.is_empty());
        for suggestion in &report.next.0 {
            let command = format!("lnk {}", suggestion.command.path()[1..].join(" "));
            assert!(
                declared.contains(&command),
                "{} is not in lnk model install's fail {declared:?}",
                suggestion.command.text()
            );
        }
    }

    #[test]
    fn the_model_fits_the_memory() {
        let model = |gb| recommend_for(gb).map(|r| r.model);
        assert_eq!(model(8).unwrap(), "qwen3:4b");
        assert_eq!(model(16).unwrap(), "qwen3:8b");
        assert_eq!(model(24).unwrap(), "qwen3:8b");
        assert_eq!(model(64).unwrap(), "qwen3:14b");
        assert!(model(4).unwrap_err().contains("too little"));
    }

    /// A fake Ollama: `/api/pull` streams a pull of `qwen3:8b` and fails
    /// any other model; `/api/delete` deletes only `qwen3:8b`.
    async fn fake_ollama() -> String {
        use axum::{
            Json,
            http::StatusCode,
            routing::{delete, post},
        };
        let app = axum::Router::new()
            .route(
                "/api/pull",
                post(|Json(body): Json<serde_json::Value>| async move {
                    match body["model"].as_str() {
                        Some("qwen3:8b") => concat!(
                            r#"{"status":"pulling manifest"}"#,
                            "\n",
                            r#"{"status":"pulling a","digest":"a","total":10,"completed":10}"#,
                            "\n",
                            r#"{"status":"verifying sha256 digest"}"#,
                            "\n",
                            r#"{"status":"success"}"#,
                            "\n",
                        ),
                        _ => concat!(
                            r#"{"error":"pull model manifest: file does not exist"}"#,
                            "\n"
                        ),
                    }
                }),
            )
            .route(
                "/api/delete",
                delete(|Json(body): Json<serde_json::Value>| async move {
                    match body["model"].as_str() {
                        Some("qwen3:8b") => StatusCode::OK,
                        _ => StatusCode::NOT_FOUND,
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await });
        url
    }

    #[tokio::test]
    async fn models_are_pulled_and_deleted_through_ollamas_api() {
        let ollama = fake_ollama().await;
        let (mut out, _) = Out::capture(false, &["lnk-model", "model", "install"]);
        pull(&mut out, &ollama, "qwen3:8b").await.unwrap();
        let err = pull(&mut out, &ollama, "nope")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("file does not exist"), "{err}");
        delete(&ollama, "qwen3:8b").await.unwrap();
        assert!(delete(&ollama, "nope").await.is_err());
    }

    #[test]
    fn progress_adds_up_the_layers_and_stops_on_errors() {
        let mut progress = Progress::new("qwen3:8b");
        let line = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap();
        progress
            .update(&line(r#"{"status":"pulling manifest"}"#))
            .unwrap();
        assert_eq!(progress.shown, "Pulling manifest");
        progress
            .update(&line(
                r#"{"status":"pulling a","digest":"a","total":3000000000,"completed":1500000000}"#,
            ))
            .unwrap();
        progress
            .update(&line(
                r#"{"status":"pulling b","digest":"b","total":1000000000,"completed":0}"#,
            ))
            .unwrap();
        assert_eq!(progress.shown, "Downloading qwen3:8b");
        assert_eq!(
            (progress.completed, progress.total),
            (1_500_000_000, 4_000_000_000)
        );
        progress.update(&line(r#"{"status":"success"}"#)).unwrap();
        progress.finish().unwrap();

        let mut failed = Progress::new("nope");
        let err = failed
            .update(&line(
                r#"{"error":"pull model manifest: file does not exist"}"#,
            ))
            .unwrap_err();
        assert!(err.to_string().contains("file does not exist"), "{err}");
        // Cut off before "success": run it again.
        assert!(Progress::new("qwen3").finish().is_err());
    }
}
