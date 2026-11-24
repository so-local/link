//! Saved wrappers never follow a grant replaced with a symbolic link,
//! and mount-source descriptors cannot bypass read-only mounts.

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

struct Running(std::process::Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn terminal() -> (std::fs::File, std::fs::File) {
    let (mut master, mut slave) = (-1, -1);
    // SAFETY: valid descriptor outputs, with default terminal settings.
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        },
        0
    );
    for fd in [master, slave] {
        // SAFETY: a flag on descriptors just opened by this test.
        assert_eq!(
            unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) },
            0
        );
    }
    // SAFETY: two fresh descriptors, each owned exactly once.
    unsafe {
        (
            std::fs::File::from_raw_fd(master),
            std::fs::File::from_raw_fd(slave),
        )
    }
}

fn receive(terminal: &mut std::fs::File, bytes: &mut Vec<u8>) {
    let mut poll = libc::pollfd {
        fd: terminal.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: a valid pollfd for this open terminal; a bounded wait.
    if unsafe { libc::poll(&mut poll, 1, 100) } > 0 {
        let mut buf = [0; 4096];
        if let Ok(n) = terminal.read(&mut buf) {
            bytes.extend_from_slice(&buf[..n]);
        }
    }
}

#[test]
fn a_specs_approved_grants_cannot_be_redirected_before_wrapping() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let root = link_test_kit::TempDir::under(
        Path::new(env!("CARGO_TARGET_TMPDIR")),
        "spec-grant-snapshot",
    );
    let home = root.join("home");
    let grant = home.join("grant");
    let outside = home.join("outside");
    std::fs::create_dir_all(&grant).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("sentinel"), "outside-grant").unwrap();
    let spec = root.join("run.toml");
    std::fs::write(
        &spec,
        format!(
            "schema = 1\nname = \"snapshot\"\nversion = 1\n[files]\nread = [{}]\n[commands]\ntoolchains = [\"shell\"]\n",
            serde_json::to_string(&grant.display().to_string()).unwrap()
        ),
    )
    .unwrap();
    let (mut master, slave) = terminal();
    let script = format!("echo target-started; cat '{}/sentinel'", outside.display());
    let mut child = Running(
        link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-sandbox"), &home)
            .args(["run", "--spec"])
            .arg(&spec)
            .args(["--", "/bin/sh", "-c", &script])
            .env_clear()
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave))
            .current_dir(&home)
            .spawn()
            .unwrap(),
    );
    let started = Instant::now();
    let mut output = Vec::new();
    while !String::from_utf8_lossy(&output).contains("Run it?") {
        receive(&mut master, &mut output);
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "{}",
            String::from_utf8_lossy(&output)
        );
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "no permission question: {}",
            String::from_utf8_lossy(&output)
        );
    }
    // Authorization has captured the original grant. Change the name
    // before answering, while wrapping has not yet begun.
    std::fs::rename(&grant, home.join("moved")).unwrap();
    std::os::unix::fs::symlink(&outside, &grant).unwrap();
    master.write_all(b"y\n").unwrap();
    let status = loop {
        receive(&mut master, &mut output);
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "sandbox did not end"
        );
    };
    receive(&mut master, &mut output);
    let text = String::from_utf8_lossy(&output);
    assert!(!status.success(), "{text}");
    assert!(
        !text.contains("target-started") && !text.contains("outside-grant"),
        "{text}"
    );
    assert!(
        text.contains("pinning sandbox source") && text.contains("target not started"),
        "{text}"
    );
}

fn wrapped(home: &Path, grant: &Path, script: &str) -> Vec<String> {
    let mut child = link_test_kit::env::command(env!("CARGO_BIN_EXE_lnk-sandbox"), home)
        .args(["wrap"])
        .env_clear()
        .env("HOME", home)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(home))
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let request = serde_json::json!({
        "confinement": {
            "name": "pinned-grant", "write": [], "read": [grant],
            "commands": "any", "net": "closed"
        },
        "argv": ["/bin/sh", "-c", script]
    });
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reply: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    serde_json::from_value(reply["argv"].clone()).unwrap()
}

#[test]
fn a_saved_grant_replaced_with_a_link_never_starts_its_target() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let temp = link_test_kit::TempDir::new("sandbox-saved-grant");
    let home = temp.join("home");
    let parent = home.join("shared");
    let grant = parent.join("grant");
    let outside = temp.join("outside");
    for dir in [&grant, &outside] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(outside.join("sentinel"), "outside-grant").unwrap();
    let argv = wrapped(
        &home,
        &grant,
        &format!("echo target-started; cat '{}/sentinel'", grant.display()),
    );
    std::fs::rename(&grant, parent.join("moved")).unwrap();
    std::os::unix::fs::symlink(&outside, &grant).unwrap();
    let output = link_test_kit::env::command(&argv[0], &home)
        .args(&argv[1..])
        .current_dir("/")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "target started: {:?}",
        output.stdout
    );
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("pinning sandbox source") && error.contains("target not started"),
        "{error}"
    );
}

#[test]
fn mount_source_handles_are_closed_before_the_target_starts() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let temp = link_test_kit::TempDir::new("sandbox-mount-handles");
    let home = temp.join("home");
    let grant = home.join("read-only");
    std::fs::create_dir_all(&grant).unwrap();
    let argv = wrapped(
        &home,
        &grant,
        "echo target-started; for fd in /proc/self/fd/*; do readlink \"$fd\" || :; done",
    );
    let output = link_test_kit::env::command(&argv[0], &home)
        .args(&argv[1..])
        .current_dir("/")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("target-started"), "{text}");
    assert!(
        !text.contains("read-only") && !text.lines().any(|line| line == "/usr" || line == "/etc"),
        "host mount handles were inherited: {text}"
    );
}

/// A sandbox started inside another, as a program starts its
/// installers and tools: the outer one shows the machine's `/etc/group`,
/// which the machine has since replaced (`useradd` writes a new one and
/// renames it over the old), so the file it shows has no name left. The
/// inner sandbox still starts, and its program reads what the outer one
/// shows.
#[test]
fn a_file_replaced_outside_an_outer_sandbox_still_starts_the_inner_one() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let temp = link_test_kit::TempDir::new("sandbox-replaced-source");
    let home = temp.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let machine = temp.join("group");
    std::fs::write(&machine, "shown-before").unwrap();
    let argv = wrapped(&home, &home, "cat /etc/group");
    let replace = format!(
        "echo replaced > '{0}.new' && mv '{0}.new' '{0}' && exec \"$@\"",
        machine.display()
    );
    let output = link_test_kit::env::command("bwrap", &home)
        .args(["--dev-bind", "/", "/", "--ro-bind"])
        .arg(&machine)
        .args(["/etc/group", "--", "/bin/sh", "-c", &replace, "sh"])
        .args(&argv)
        .current_dir("/")
        .output()
        .unwrap();
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{error}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "shown-before",
        "{error}"
    );
}
