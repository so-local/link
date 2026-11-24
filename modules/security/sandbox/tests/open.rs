//! Opening a page from a proxied sandbox, for real, on Linux: Python's
//! `webbrowser` in `lnk sandbox run --internet.hosts '*' --browser.hosts
//! '*'` reaches its
//! proxy through `$BROWSER`, outside, which refuses it here, with no
//! desktop to open it on, before asking anyone; the wire's log keeps the
//! page's host, never its path. The verb is only at `$LNK_SANDBOX_OPEN`,
//! behind the proxy's secret. Without `open`, the program has no
//! `$BROWSER` and the proxy refuses the verb. (What's asked, the page
//! opened and a login's callback passed in: `open.rs`'s own tests.)

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use link_test_kit::TempDir;

/// A login in the sandbox: the page opened, as Python opens one.
const LOGIN: &str = r#"
import os, webbrowser
print("BROWSER", os.environ.get("BROWSER"), flush=True)
url = "https://id.example.com/auth?client_id=x&redirect_uri=http%3A%2F%2Flocalhost%3A8085%2Fcb&state=secret"
print("OPENED", webbrowser.open(url), flush=True)
"#;

/// `lnk-sandbox` with `args`, HOME `root`, no desktop and no terminal (a
/// session of its own).
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn lnk(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lnk-sandbox"));
    command
        .args(args)
        .current_dir(root)
        .env("HOME", root)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("BROWSER")
        .env_remove("XDG_CONFIG_HOME")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: setsid only, between fork and exec.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    command
}

fn ran(command: &mut Command) -> (bool, String) {
    let output = command.output().unwrap();
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    (output.status.success(), text)
}

#[test]
fn a_page_goes_to_the_proxy_through_browser_and_only_with_open() {
    if !link_test_kit::gate::bubblewrap()
        || !link_test_kit::gate::program("curl", &["--version"])
        || !link_test_kit::gate::program("python3", &["--version"])
    {
        return;
    }
    let root = TempDir::new("open");

    let (_, text) = ran(&mut lnk(
        &root,
        &[
            "run",
            "--commands.toolchains",
            "shell",
            "--commands.toolchains",
            "python",
            "--internet.hosts",
            "*",
            "--browser.hosts",
            "*",
            "--",
            "python3",
            "-c",
            LOGIN,
        ],
    ));
    let sandbox = Path::new(env!("CARGO_BIN_EXE_lnk-sandbox"))
        .canonicalize()
        .unwrap();
    assert!(
        text.contains(&format!("BROWSER {}", sandbox.display())),
        "{text}"
    );
    assert!(text.contains("OPENED False"), "{text}");
    assert!(text.contains("no desktop here to open a page on"), "{text}");
    // Nothing was asked, and nothing waits.
    let (_, waiting) = ran(&mut lnk(&root, &["question", "list"]));
    assert!(waiting.contains("No questions are waiting"), "{waiting}");
    let (_, log) = ran(&mut lnk(&root, &["log", "show", "--json"]));
    assert!(
        log.lines().any(|l| l.contains(r#""verb":"open""#)
            && l.contains(r#""to":"id.example.com""#)
            && l.contains(r#""outcome":"refused""#)),
        "{log}"
    );
    assert!(!log.contains("secret"), "no path in the log: {log}");

    // The verb is behind the proxy's secret: its old path is refused.
    let (_, text) = ran(&mut lnk(
        &root,
        &[
            "run",
            "--commands.toolchains",
            "shell",
            "--internet.hosts",
            "*",
            "--browser.hosts",
            "*",
            "--",
            "sh",
            "-c",
            "echo \"OPEN=$LNK_SANDBOX_OPEN\"; \
             curl -s -o /dev/null -w 'bare %{http_code}\\n' -X POST --data https://id.example.com/ \"$HTTP_PROXY/_link/web/open\"; \
             curl -s -X POST --data https://id.example.com/ \"$LNK_SANDBOX_OPEN\"",
        ],
    ));
    assert!(
        text.contains("OPEN=http://127.0.0.1:40781/_link/url/"),
        "{text}"
    );
    assert!(text.contains("bare 403"), "{text}");
    assert!(text.contains("no desktop here to open a page on"), "{text}");
    let (_, log) = ran(&mut lnk(&root, &["log", "show", "--json"]));
    assert!(!log.contains("/_link/url/"), "no secret in the log: {log}");

    // Not allowed: no BROWSER, and the proxy refuses the verb.
    let (_, text) = ran(&mut lnk(
        &root,
        &[
            "run",
            "--commands.toolchains",
            "shell",
            "--internet.hosts",
            "*",
            "--",
            "sh",
            "-c",
            "echo \"BROWSER=$BROWSER OPEN=$LNK_SANDBOX_OPEN\"; \
             curl -s -o /dev/null -w 'bare %{http_code}\\n' -X POST --data https://id.example.com/ \"$HTTP_PROXY/_link/web/open\"",
        ],
    ));
    assert!(text.contains("BROWSER= OPEN=\n"), "{text}");
    assert!(text.contains("bare 403"), "{text}");
    // Pages alone: a proxy with no internet carries them.
    let (_, text) = ran(&mut lnk(
        &root,
        &[
            "run",
            "--commands.toolchains",
            "shell",
            "--browser.hosts",
            "*",
            "--",
            "sh",
            "-c",
            "echo \"BROWSER=$BROWSER\"",
        ],
    ));
    assert!(
        text.contains(&format!("BROWSER={}", sandbox.display())),
        "{text}"
    );
}
