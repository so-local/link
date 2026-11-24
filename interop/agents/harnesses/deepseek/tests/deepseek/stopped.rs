//! `lnk-deepseek serve` stopped with a SIGTERM stops the `dsh` it started,
//! rather than leave it running. A shell script stands in for `node`.

#![cfg(unix)]

use std::io::Write;
use std::process::Stdio;
use std::time::Duration;

use link_test_kit::{ChildGroup, TempDir, env};

#[test]
fn serve_stopped_stops_dsh() {
    let root = TempDir::new("deepseek-stopped");
    let home = root.join("home");
    let files = root.join("files");
    std::fs::create_dir_all(&files).unwrap();
    link_harness_contract::harness::continuation::Context::configure(
        &home,
        "deepseek",
        &link_harness_contract::harness::Settings::default(),
    )
    .unwrap();
    let pid = root.join("dsh.pid");
    // `dsh`, standing in: says its process and waits, answering nothing.
    let node = link_files::layout::Hidden::DeepseekPrograms
        .in_home(&home)
        .join(format!("node-v{}/bin/node", crate::node_version()));
    std::fs::create_dir_all(node.parent().unwrap()).unwrap();
    let script = format!("#!/bin/sh\necho $$ > {}\nexec sleep 300\n", pid.display());
    link_test_kit::write_program(&node, &script).unwrap();
    // Its port, which it binds itself: held until the test ends.
    let held = link_test_kit::HeldNumber::port();
    let port = held.port_number();
    let mut command = env::command(env!("CARGO_BIN_EXE_lnk-deepseek"), &home);
    command
        .arg("serve")
        .env("LNK_FILES", &files)
        .env("LNK_PORT", port.to_string())
        .stdin(Stdio::null())
        .stderr(Stdio::inherit());
    let serve = ChildGroup::spawn(&mut command).unwrap();
    // Its health check, asked once it listens, starts `dsh`.
    let mut asked = None;
    let dsh = link_test_kit::wait::eventually("dsh to start", Duration::from_secs(30), || {
        assert!(serve.try_status().unwrap().is_none(), "serve ended");
        let started = std::fs::read_to_string(&pid)
            .unwrap_or_default()
            .trim()
            .parse::<i32>()
            .ok();
        if started.is_none()
            && asked.is_none()
            && let Ok(mut tcp) = std::net::TcpStream::connect(("127.0.0.1", port))
        {
            let _ = tcp.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n");
            asked = Some(tcp);
        }
        started
    });
    // SAFETY: a signal to the child this test started.
    unsafe { libc::kill(serve.id() as i32, libc::SIGTERM) };
    link_test_kit::wait::eventually("serve to stop", Duration::from_secs(5), || {
        serve.try_status().unwrap().map(|status| {
            assert!(
                status.success(),
                "serve did not shut down cleanly: {status}"
            );
        })
    });
    // SAFETY: signal 0 only asks whether the process is there.
    let gone = || unsafe { libc::kill(dsh, 0) } != 0;
    link_test_kit::wait::eventually("dsh to stop with serve", Duration::from_secs(5), gone);
}
