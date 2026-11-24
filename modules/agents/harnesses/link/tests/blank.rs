//! A harness started blank, as a machine that runs an agent only while it
//! answers keeps one ahead: `lnk-harness serve --control <control>`.
//! Blank, nothing answers on its port, so no message reaches it and no
//! proof comes from it; its control socket is the user's alone. Bound
//! once ([`link_harness_contract::harness::Bind`]), it answers as that agent alone,
//! with the binding's key, its settings held in memory and its
//! conversations kept through the store bridge it was bound with; a
//! second binding is refused.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use link_agent_spec::{Spec, Storage};
use link_harness_contract::harness::{Bind, BindAgent, Model, Settings};
use link_secretive::Duplicate;
use link_test_kit::TempDir;
use serde_json::json;

use crate::common;

/// The agent it's bound to, and the key made for that binding.
const ID: &str = "ada000000001";
const OWNER: &str = "acct-ada";
const KEY: &str = "b1d0000000000000000000000000000000000000000000000000000000000000";

/// The harness process, killed when dropped.
struct Running(std::process::Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

/// Asks the harness on `port` over ACP as a bridge would, with `key`
/// and naming `named` (`Link-Agent-Id`, `Link-Agent`): the status and
/// the answer or why not.
async fn ask(port: u16, key: &str, named: &[(&str, &str)], text: &str) -> Option<(u16, String)> {
    let named = |header: &str| named.iter().find(|(h, _)| *h == header).map(|(_, v)| *v);
    let asking = common::Asking {
        key,
        agent: named(link_connection_contract::AGENT_HEADER),
        id: named(link_connection_contract::AGENT_ID_HEADER),
        ..Default::default()
    };
    common::answered(common::ask_as(port, "telegram:42", text, asking).await)
}

/// Whether the endpoint on `port` proves it holds `key`, as a connection
/// asks before it sends the key.
async fn proves(port: u16, key: &str) -> bool {
    let challenge = "c0ffee".repeat(8);
    let Ok(response) = client()
        .get(format!(
            "http://127.0.0.1:{port}{}",
            link_harness_contract::harness::PROOF_PATH
        ))
        .header(link_harness_contract::harness::CHALLENGE_HEADER, &challenge)
        .send()
        .await
    else {
        return false;
    };
    let proof = response
        .headers()
        .get(link_harness_contract::harness::PROOF_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    link_harness_contract::harness::proof_matches(
        &link_secretive::Secret::from(key.to_string()),
        &challenge,
        &proof,
    )
}

/// Binds the harness at `control` with `bind`, off the runtime.
async fn bind(control: &Path, bind: &Bind) -> anyhow::Result<()> {
    let (control, bind) = (
        control.to_path_buf(),
        link_secretive::Duplicate::duplicate(bind),
    );
    tokio::task::spawn_blocking(move || link_harness_contract::harness::bind(&control, &bind))
        .await
        .unwrap()
}

/// The SQLite driver behind a store bridge at `socket`, taking calls
/// proved with the keyring in `keys`; how many it took.
fn store_bridge(socket: &Path, keys: &Path, conversations: &Path) -> Arc<AtomicUsize> {
    let driver = link_store_contract::store::Driver::at("sqlite", common::sqlite(), conversations);
    let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let keys = link_sandbox_binding::Keyring::at(keys);
    std::thread::spawn(move || {
        link_store_contract::store::serve_bridge(
            listener,
            Arc::new(keys),
            Arc::new(move |_: &link_sandbox_binding::Identity| {
                counted.fetch_add(1, Ordering::Relaxed);
                Ok(driver.clone())
            }),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
    });
    calls
}

#[tokio::test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn a_blank_harness_answers_only_the_agent_it_was_bound_to() {
    link_fetch::install_crypto_provider();
    let root = TempDir::new("blank");
    let (home, files) = (root.join("home"), root.join("files"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(files.join("instructions")).unwrap();
    std::fs::write(files.join("instructions/instruction.md"), "Be brief.\n").unwrap();
    let (model_port, asked) = common::fake_model(|_, _| {
        vec![common::delta(
            json!({"content": "Hello from Ada's harness"}),
        )]
    })
    .await;

    // The machine's side: the agent's key bound in its keyring, and the
    // store bridge its conversations go through.
    let keys = root.join("keys");
    let identity = link_sandbox_binding::Identity {
        agent: ID.into(),
        service: String::new(),
        owner: OWNER.into(),
        teams: Vec::new(),
    };
    link_sandbox_binding::Keyring::at(&keys)
        .bind(&identity, &link_secretive::Secret::from(KEY.to_string()))
        .unwrap();
    let socket = root.join("store.sock");
    let calls = store_bridge(&socket, &keys, &files.join("chats"));

    let control = root.join("control.sock");
    // Its ports, the one given and the one after it, which it binds
    // itself: held as a block until the test ends.
    let held = crate::common::free_block();
    let port = held.number() as u16;
    let _harness = Running(
        Command::new(env!("CARGO_BIN_EXE_lnk-harness"))
            .args(["serve", "--control"])
            .arg(&control)
            .env_clear()
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
            .env("PATH", "/usr/bin:/bin")
            .env(link_harness_contract::harness::PORT_ENV, port.to_string())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let started = std::time::Instant::now();
    while !control.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "it didn't listen"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Its control socket is the user's alone.
    let mode = std::fs::metadata(&control).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);

    // Blank: nothing answers on its port, so no message reaches it and
    // it proves nothing, whatever the key.
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
    assert!(!proves(port, KEY).await);

    let mut spec: Spec = link_agent_spec::link();
    spec.storage = Some(Storage {
        conversations: Some("sqlite".into()),
        ..Storage::default()
    });
    let binding = Bind {
        agent: BindAgent {
            name: "ada".into(),
            id: ID.into(),
            service: String::new(),
            owner: OWNER.into(),
            teams: Vec::new(),
        },
        key: KEY.to_string().into(),
        settings: Settings {
            files: files.clone(),
            model: Model {
                provider: "local".into(),
                name: Some("fake".into()),
                base_url: Some(format!("http://127.0.0.1:{model_port}")),
                ..Model::default()
            },
            // A placeholder: the binding's key stands for it.
            endpoint_key: Some("placeholder".to_string().into()),
            spec: Some(spec),
            ..Settings::default()
        },
        store: Some(socket.clone()),
        files: Some(root.join("files.sock")),
    };

    // A binding it can't take is refused, and it stays blank.
    let keyless = Bind {
        key: link_secretive::Secret::default(),
        ..binding.duplicate()
    };
    let refused = bind(&control, &keyless).await.unwrap_err();
    assert!(format!("{refused:#}").contains("no key"), "{refused:#}");
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());

    bind(&control, &binding).await.unwrap();

    // Bound: it proves the binding's key, and no other.
    assert!(proves(port, KEY).await);
    assert!(!proves(port, "placeholder").await);
    // It answers as the agent, named or not, with the binding's key.
    let answered = ask(port, KEY, &[], "Hi, I'm Ada").await;
    assert_eq!(
        answered,
        Some((200, "Hello from Ada's harness".to_string()))
    );
    let answered = ask(
        port,
        KEY,
        &[
            (link_connection_contract::AGENT_ID_HEADER, ID),
            (link_connection_contract::AGENT_HEADER, "ada"),
        ],
        "Again",
    )
    .await;
    assert_eq!(answered.map(|(status, _)| status), Some(200));
    // Never with the settings' placeholder, nor as another agent.
    let answered = ask(port, "placeholder", &[], "Let me in").await;
    assert_eq!(answered.map(|(status, _)| status), Some(401));
    for named in [
        (link_connection_contract::AGENT_ID_HEADER, "bea000000002"),
        (link_connection_contract::AGENT_HEADER, "bea"),
    ] {
        let answered = ask(port, KEY, &[named], "As Bea").await;
        assert_eq!(answered.map(|(status, _)| status), Some(404), "{named:?}");
    }
    assert_eq!(asked.lock().unwrap().len(), 2);

    // A second binding is refused: it stays the agent's it was bound to.
    let again = Bind {
        agent: BindAgent {
            name: "bea".into(),
            id: "bea000000002".into(),
            service: String::new(),
            owner: OWNER.into(),
            teams: Vec::new(),
        },
        ..binding.duplicate()
    };
    let refused = bind(&control, &again).await.unwrap_err();
    assert!(format!("{refused:#}").contains("bound"), "{refused:#}");
    let answered = ask(
        port,
        KEY,
        &[(link_connection_contract::AGENT_HEADER, "bea")],
        "Hi",
    )
    .await;
    assert_eq!(answered.map(|(status, _)| status), Some(404));

    // Its conversation went through the store bridge, proved with the
    // binding's key; its settings were never a file.
    assert!(calls.load(Ordering::Relaxed) > 0);
    assert!(files.join("chats/events.sqlite").is_file());
    let mut written: Vec<PathBuf> = Vec::new();
    let mut stack = vec![home.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            match entry.path().is_dir() {
                true => stack.push(entry.path()),
                false => written.push(entry.path()),
            }
        }
    }
    assert!(
        !written.iter().any(|path| path.ends_with("config.json")),
        "{written:?}"
    );
    for path in &written {
        let text = String::from_utf8_lossy(&std::fs::read(path).unwrap_or_default()).to_string();
        assert!(!text.contains(KEY), "{} holds the key", path.display());
    }
}
