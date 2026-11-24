//! What a box may reach: nothing that opens this computer, and an
//! account's agent only where that account is signed in.

use super::*;

/// A box an agent comes back from writes the stream, so what it says
/// opens nothing of this computer: no permission that reaches it, no
/// endpoint past its proxy, no tool that runs outside, and of
/// a connection's folder only what this computer's own module names (the
/// module runs what's there outside any sandbox).
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_box_chooses_nothing_that_opens_this_computer() {
    let world = World::new("hostile-box");
    let (here, there) = (&world.here, &world.there);
    crate::common::write_adapter(&world.bin.join("lnk-bare"), BARE).unwrap();
    executable(&world.bin.join("lnk-signal"), SIGNAL);
    crate::common::write_agent(
        there,
        "main",
        &format!(
            "id = \"agent:{ID}\"\nharness = \"bare\"\nfiles = \"{}\"\n\n[model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-test\"\n\n\
             {}",
            world.files(there).display(),
            crate::common::connection(
                "signal",
                &[],
                "account = \"+15557654321\"\nallowed_users = [\"+15551234567\"]",
                "id = \"+15557654321\""
            )
        ),
    );
    let signal = |home: &Path| home.join(".lnk/safe/agents/main/connections/signal");
    write(
        &signal(there).join(".signal-cli/data/data/accounts.json"),
        "linked",
    );
    write(&signal(there).join(".signal-cli/account"), "+15557654321");

    // What the box's lnk sends, opened by the hostile box before it
    // rewrites it and seals it to the actual receiving machine.
    let sender = link_secretive::Recipient::new();
    let sender_to = sender.seal_to().to_string();
    let sent = Command::new(env!("CARGO_BIN_EXE_lnk-agent"))
        .args(["state-send", "--seal-to", &sender_to])
        .env_clear()
        .env("HOME", there)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(there))
        .env("PATH", format!("{}:/usr/bin:/bin", world.bin.display()))
        .env("LNK_EXE", world.bin.join("lnk"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        sent.status.success(),
        "{}",
        String::from_utf8_lossy(&sent.stderr)
    );
    // Another agent here runs the same harness, with Link's defaults.
    crate::common::write_agent(
        here,
        "other",
        "id = \"agent:b2c3d4e5f6a1\"\nservice = \"service:o2t3h4e5r6s7\"\nname = \"other\"\nharness = \"bare\"\n",
    );
    let mut receive = Command::new(env!("CARGO_BIN_EXE_lnk-agent"))
        .args(["receive", "--no-start"])
        .env_clear()
        .env("HOME", here)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(here))
        .env("PATH", format!("{}:/usr/bin:/bin", world.bin.display()))
        .env("LNK_EXE", world.bin.join("lnk"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // What it says to seal to, which a hostile box is given as any is.
    let mut announced = std::io::BufReader::new(receive.stdout.take().unwrap());
    let seal_to = link_secretive::SealTo::read_announced(&mut announced, |_| {}).unwrap();
    // ...as a hostile box rewrites it, each file with its hash as a box
    // can make it: every permission, all of Signal's folder, and a program
    // there Link would run.
    let mut stream = Vec::new();
    {
        let mut out = tar::Builder::new(&mut stream);
        let opened = link_secretive::stream::Receiver::new(
            sent.stdout.as_slice(),
            &sender,
            &link_agent_contract::moving::stream_about(),
        )
        .unwrap();
        let mut archive = tar::Archive::new(opened);
        let hashed = |out: &mut tar::Builder<&mut Vec<u8>>, data: &[u8]| {
            let hash = link_digest::sha256_of(data).unwrap();
            out.append_pax_extensions([(link_agent_contract::moving::SHA256, hash.as_bytes())])
                .unwrap();
        };
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let mut header = entry.header().clone();
            let path = entry.path().unwrap().into_owned();
            let mut data = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut data).unwrap();
            if path == Path::new(link_agent_contract::moving::END) {
                let program = b"#!/bin/sh\necho planted\n";
                let mut header = tar::Header::new_gnu();
                header.set_size(program.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                hashed(&mut out, program);
                out.append_data(
                    &mut header,
                    "connections/signal/.signal-cli/0.14.8/signal-cli",
                    program.as_slice(),
                )
                .unwrap();
            }
            if path == Path::new("lnk-move.json") {
                let mut manifest: serde_json::Value = serde_json::from_slice(&data).unwrap();
                manifest["sandbox"] = serde_json::json!({
                    "files": {"read": ["~"]},
                    "network": {"mode": "host"},
                    "internet": {"hosts": ["*"], "tcp": ["github.com:22"]},
                    "lan": {"hosts": ["*"]},
                    "localhost": {"ports": [5432]},
                    "tools": {"names": ["gh"]},
                    "browser": {"hosts": ["*"]},
                });
                manifest["connections"]["signal"]["include"] = serde_json::json!(["~/.signal-cli"]);
                // What Signal is, and a connection that runs another module.
                manifest["connected"]["signal"]["about"] =
                    serde_json::json!({"title": "\u{1b}]0;Signal", "linked": true});
                manifest["connected"]["box"] = manifest["connected"]["signal"].clone();
                data = serde_json::to_vec(&manifest).unwrap();
                header.set_size(data.len() as u64);
                header.set_cksum();
            }
            if header.entry_type() == tar::EntryType::Regular {
                hashed(&mut out, &data);
            }
            out.append_data(&mut header, &path, data.as_slice())
                .unwrap();
        }
        out.finish().unwrap();
    }

    let mut input = receive.stdin.take().unwrap();
    std::io::Write::write_all(&mut input, b"{}\n").unwrap();
    let mut sealed = link_secretive::stream::Sender::new(
        input,
        &seal_to,
        link_agent_contract::moving::stream_about(),
    )
    .unwrap();
    std::io::Write::write_all(&mut sealed, &stream).unwrap();
    sealed.finish().unwrap();
    let mut printed = Vec::new();
    std::io::Read::read_to_end(&mut announced, &mut printed).unwrap();
    let mut output = receive.wait_with_output().unwrap();
    printed.extend(output.stdout);
    output.stdout = printed;
    let said = ok(&output);

    let settings = std::fs::read_to_string(here.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    // What it came with: never what opens this machine. Its harness's
    // grants here aren't the stream's to write: none came, none changed.
    let grants = std::fs::read_to_string(here.join("Link/Safe/Agents/main/sandbox.toml")).unwrap();
    assert!(grants.contains("[internet]"), "{grants}");
    for (flags, kept) in [
        ("--files.read '~'", "\"~\""),
        ("--network.mode host", "[network]"),
        ("--internet.tcp github.com:22", "github.com:22"),
        ("--lan.hosts '*'", "[lan]"),
        ("--localhost.ports 5432", "[localhost]"),
        ("--tools.names gh", "[tools]"),
        ("--browser.hosts '*'", "[browser]"),
    ] {
        assert!(!grants.contains(kept), "{flags}: {grants}");
        assert!(
            said.replace('\'', "").contains(&format!(
                "lnk agent permission add {}",
                flags.replace('\'', "")
            )),
            "{said}"
        );
        assert!(said.contains("not taken from the machine"), "{said}");
    }
    assert!(
        !here
            .join(".lnk/safe/agents/harness-grants/bare.toml")
            .exists()
    );
    let cli = signal(here).join(".signal-cli");
    assert_eq!(
        std::fs::read_to_string(cli.join("data/data/accounts.json")).unwrap(),
        "linked"
    );
    assert!(!cli.join("0.14.8").exists());
    // Signal is what its module here says; `box` is no connection here.
    let toml: toml::Table = settings.parse().unwrap();
    let about = &toml["connections"]["signal"]["about"];
    assert_eq!(about["title"].as_str(), Some("Signal"), "{settings}");
    assert_eq!(about["cli"].as_bool(), Some(true), "{settings}");
    assert!(toml["connections"].get("box").is_none(), "{settings}");
    assert!(said.contains("connection box"), "{said}");
}

/// An account's agent moves only to a machine signed in as that account:
/// one signed out, or signed in as another, refuses it, and takes
/// nothing of it. Nor does one where another agent has its ID.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn an_accounts_agent_lands_only_where_that_account_is_signed_in() {
    let world = World::new("owners");
    let (here, there) = (&world.here, &world.there);
    crate::common::write_adapter(&world.bin.join("lnk-bare"), BARE).unwrap();
    crate::common::write_agent(
        there,
        "main",
        &format!(
            "id = \"agent:{ID}\"\nowner = \"github-1\"\nharness = \"bare\"\nfiles = \"{}\"\n\n\
             [model]\nprovider = \"anthropic\"\napi_key = \"sk-ant-test\"\n",
            world.files(there).display()
        ),
    );
    // The switch, as `move` carries it: `receive` says what to seal to,
    // and `send` is given it.
    let receive = || {
        let mut receive = Command::new(env!("CARGO_BIN_EXE_lnk-agent"))
            .args(["receive", "--no-start"])
            .env_clear()
            .env("HOME", here)
            .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(here))
            .env("PATH", format!("{}:/usr/bin:/bin", world.bin.display()))
            .env("LNK_EXE", world.bin.join("lnk"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut announced = std::io::BufReader::new(receive.stdout.take().unwrap());
        let seal_to = link_secretive::SealTo::read_announced(&mut announced, |_| {})
            .unwrap()
            .to_string();
        let sent = Command::new(env!("CARGO_BIN_EXE_lnk-agent"))
            .args(["state-send", "--seal-to", &seal_to])
            .env_clear()
            .env("HOME", there)
            .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(there))
            .env("PATH", format!("{}:/usr/bin:/bin", world.bin.display()))
            .env("LNK_EXE", world.bin.join("lnk"))
            .stdin(Stdio::null())
            .output()
            .unwrap();
        ok(&sent);
        let stream = [b"{}\n".as_slice(), &sent.stdout].concat();
        std::io::Write::write_all(&mut receive.stdin.take().unwrap(), &stream).unwrap();
        let mut printed = Vec::new();
        std::io::Read::read_to_end(&mut announced, &mut printed).unwrap();
        let mut output = receive.wait_with_output().unwrap();
        printed.extend(output.stdout);
        output.stdout = printed;
        output
    };
    let settings = here.join("Link/Safe/Agents/main/agent.toml");
    let refused = |output: Output, why: &str| {
        let said = String::from_utf8_lossy(&output.stderr).to_string();
        assert!(!output.status.success() && said.contains(why), "{said}");
        assert!(!settings.exists(), "it was put in place");
    };
    // Signed out.
    refused(
        receive(),
        "it moves only to a machine signed in as that account",
    );
    // Signed in as another account.
    write(&here.join("account.json"), r#"{"owner": "github-2"}"#);
    refused(receive(), "is another account's");
    // As its own, but another agent here has its ID.
    write(&here.join("account.json"), r#"{"owner": "github-1"}"#);
    crate::common::write_agent(
        here,
        "other",
        &format!("id = \"agent:{ID}\"\nservice = \"service:o2t3h4e5r6s7\"\nharness = \"bare\"\n"),
    );
    refused(receive(), "has the ID of other");
    std::fs::remove_dir_all(here.join("Link/Safe/Agents/other")).unwrap();
    std::fs::remove_dir_all(here.join(".lnk/safe/agents/other")).unwrap();
    // As its own: taken.
    ok(&receive());
    assert_eq!(
        crate::common::record(here, "main", "identity.toml")["owner"],
        serde_json::json!({"kind": "account", "id": "github-1"})
    );
}
