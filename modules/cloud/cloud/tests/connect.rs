//! `lnk cloud connect` with a fake adapter: what it keeps, and what other
//! modules read back.

link_test_kit::program!(system);

use std::path::Path;
use std::process::Command;

use link_test_kit::TempDir;

#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn lnk_cloud(home: &Path, path: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lnk-cloud"));
    command
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .env("PATH", format!("{}:/usr/bin:/bin", path.display()))
        // Never this machine's own Keychain: a test that wants one fakes it.
        .env_remove("LNK_SECURITY")
        .envs(link_test_kit::keychain::isolated(home))
        .env("LNK_SERVICES", "off");
    command
}

#[test]
fn an_account_is_what_its_adapter_describes() {
    let dir = TempDir::new("connect-fake");
    let home = dir.join("home");
    let bin = dir.join("bin");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    let adapter = bin.join("lnk-fake");
    link_test_kit::write_program(
        &adapter,
        format!(
            "#!/bin/sh\n[ \"$1\" = agent-runs ] && {{ echo \"$*\" >> {log}.runs; exit 1; }}\n\
             echo \"$*\" > {log}\necho 'a question' >&2\n\
             echo '{{\"name\": \"work\", \"kind\": \"fake\", \"place\": \"moon-1\", \
             \"identity\": \"Fake account 42\", \"login\": \"a key\", \
             \"env\": {{\"FAKE_SECRET\": \"s3cret\"}}}}'\n",
            log = dir.join("args").display()
        ),
    )
    .unwrap();

    let output = lnk_cloud(&home, &bin)
        .args(["connect", "fake", "--name", "work", "--region", "moon-1"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("Connected work: Fake account 42, moon-1."),
        "{stdout}"
    );
    // It makes no bucket; it says how.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("lnk storage create fake --cloud work"),
        "{stderr}"
    );
    let args = std::fs::read_to_string(dir.join("args")).unwrap();
    assert_eq!(args.trim(), "connect --name work --region moon-1");
    // A machine's own account is the adapter's to read: --machine reaches it.
    let machine = lnk_cloud(&home, &bin)
        .args(["connect", "fake", "--name", "work", "--machine"])
        .output()
        .unwrap();
    assert!(
        machine.status.success(),
        "{}",
        String::from_utf8_lossy(&machine.stderr)
    );
    let args = std::fs::read_to_string(dir.join("args")).unwrap();
    assert_eq!(args.trim(), "connect --name work --machine");

    let saved = home.join(".lnk/safe/clouds/accounts.toml");
    let mode = std::fs::metadata(&saved).unwrap().permissions();
    assert_eq!(
        std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777,
        0o600
    );

    let env = lnk_cloud(&home, &bin)
        .args(["env", "work", "--json"])
        .output()
        .unwrap();
    let env: serde_json::Value = serde_json::from_slice(&env.stdout).unwrap();
    assert_eq!(env["kind"], "fake");
    assert_eq!(env["env"]["FAKE_SECRET"], "s3cret");

    // The list never carries the keys.
    let list = lnk_cloud(&home, &bin)
        .args(["list", "--json"])
        .output()
        .unwrap();
    let list = String::from_utf8_lossy(&list.stdout);
    assert!(
        list.contains("\"work\"") && !list.contains("s3cret"),
        "{list}"
    );
    // Shown as JSON, it's the account as listed, never its keys.
    let shown = lnk_cloud(&home, &bin)
        .args(["show", "work", "--json"])
        .output()
        .unwrap();
    let shown: serde_json::Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(shown["name"], "work");
    assert_eq!(shown["kind"], "fake");
    assert_eq!(shown["identity"], "Fake account 42");
    assert!(shown.get("env").is_none(), "{shown}");
    // Whether it runs agents is another module's question: lnk cloud
    // never asks, and its list doesn't say.
    assert!(!list.contains("\"runs_"), "{list}");
    assert!(!dir.join("args.runs").exists());

    // An adapter describing another account than asked for is refused.
    let output = lnk_cloud(&home, &bin)
        .args(["connect", "fake", "--name", "other"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let disconnected = lnk_cloud(&home, &bin)
        .args(["disconnect", "work"])
        .output()
        .unwrap();
    assert!(disconnected.status.success());
    assert!(!std::fs::read_to_string(&saved).unwrap().contains("work"));
    // The adapter forgets what it kept for the account.
    let args = std::fs::read_to_string(dir.join("args")).unwrap();
    assert_eq!(args.trim(), "disconnect");
}

#[test]
fn signed_in_modules_expose_accounts_without_persisting_a_connection() {
    let dir = TempDir::new("discovered-cloud-account");
    let home = dir.join("home");
    let bin = dir.join("bin");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    let lnk = bin.join("lnk");
    link_test_kit::write_program(
        &lnk,
        "#!/bin/sh\ncase \"$1\" in\nmodule) cat \"$HOME/modules.json\";;\n*) exit 1;;\nesac\n",
    )
    .unwrap();
    // The module's program, beside lnk, answers in its envelope.
    link_test_kit::write_program(&bin.join("lnk-fake"), "#!/bin/sh\n[ \"$*\" = \"--link-answer cloud.accounts\" ] || exit 1\ncat >/dev/null\nprintf '{\"protocol\":1,\"answer\":'; cat \"$HOME/answer.json\"; echo '}'\n").unwrap();
    std::fs::write(
        home.join("modules.json"),
        r#"[{"name":"fake","installed":true,"answers":["cloud.accounts"]}]"#,
    )
    .unwrap();
    std::fs::write(
        home.join("answer.json"),
        r#"{"accounts":[{"name":"host","kind":"fake","place":"home","identity":"owner"}]}"#,
    )
    .unwrap();
    let listed = lnk_cloud(&home, &bin)
        .env("LNK_EXE", &lnk)
        .args(["list", "--json"])
        .output()
        .unwrap();
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let listed: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(listed[0]["name"], "host");
    let account = lnk_cloud(&home, &bin)
        .env("LNK_EXE", &lnk)
        .args(["env", "host", "--json"])
        .output()
        .unwrap();
    assert!(
        account.status.success(),
        "{}",
        String::from_utf8_lossy(&account.stderr)
    );
    let account: serde_json::Value = serde_json::from_slice(&account.stdout).unwrap();
    assert_eq!(account["identity"], "owner");
    assert!(!home.join(".lnk/safe/clouds/accounts.toml").exists());
    // Signing out removes the discovered account, with no stale file.
    std::fs::write(home.join("answer.json"), r#"{"accounts":[]}"#).unwrap();
    let listed = lnk_cloud(&home, &bin)
        .env("LNK_EXE", &lnk)
        .args(["list", "--json"])
        .output()
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&listed.stdout).unwrap(),
        serde_json::json!([])
    );
}

/// On a Mac an account's secrets are in the Keychain (here a stand-in for
/// Apple's `security`): put there on connect, read back for other modules,
/// and removed with the account.
#[test]
fn secrets_live_in_the_keychain() {
    let dir = TempDir::new("connect-keychain");
    let home = dir.join("home");
    let file = home.join(".lnk/safe/clouds/accounts.toml");
    let bin = dir.join("bin");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    link_test_kit::write_program(
        &bin.join("lnk-fake"),
        "#!/bin/sh\n\
         echo '{\"name\": \"work\", \"kind\": \"fake\", \"place\": \"us-east-1\", \
         \"identity\": \"x\", \"env\": {\"FAKE_ID\": \"AKIAEXAMPLE\", \
         \"FAKE_SECRET\": \"s3cret\"}}'\n",
    )
    .unwrap();
    let security = link_test_kit::keychain::stand_in(&dir);
    let run = |args: &[&str]| {
        let out = lnk_cloud(&home, &bin)
            .env("LNK_SECURITY", &security)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    };
    run(&["connect", "fake", "--name", "work"]);
    let env = run(&["env", "work", "--json"]);
    assert!(
        env.contains("s3cret") && env.contains("AKIAEXAMPLE"),
        "{env}"
    );
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        !text.contains("s3cret") && text.contains("AKIAEXAMPLE"),
        "{text}"
    );
    assert!(text.contains("keychain = [\"FAKE_SECRET\"]"), "{text}");
    let item = dir.join("items/cloud/work/FAKE_SECRET");
    assert_eq!(std::fs::read_to_string(&item).unwrap(), "s3cret");
    // Read from there again, the file untouched.
    assert!(run(&["env", "work", "--json"]).contains("s3cret"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), text);

    run(&["disconnect", "work"]);
    assert!(!item.exists());
}

/// A Keychain that can't keep an account's secret (locked, or asking for
/// a person) fails `lnk cloud connect`, and the secret is in no file;
/// one that can't give it back fails `lnk cloud env`.
#[test]
fn a_failing_keychain_fails_connect_and_env() {
    let dir = TempDir::new("connect-refusing");
    let home = dir.join("home");
    let file = home.join(".lnk/safe/clouds/accounts.toml");
    let bin = dir.join("bin");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    link_test_kit::write_program(
        &bin.join("lnk-fake"),
        "#!/bin/sh\n\
         echo '{\"name\": \"work\", \"kind\": \"fake\", \"place\": \"us-east-1\", \
         \"identity\": \"x\", \"env\": {\"FAKE_SECRET\": \"s3cret\"}}'\n",
    )
    .unwrap();
    let refusing = link_test_kit::keychain::refusing(&dir);
    let out = lnk_cloud(&home, &bin)
        .env("LNK_SECURITY", &refusing)
        .args(["connect", "fake", "--name", "work"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "connect succeeded");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("Keychain"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let saved = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(!saved.contains("s3cret"), "{saved}");

    // Kept by a working Keychain, then asked of one that can't give it.
    let security = link_test_kit::keychain::stand_in(&dir);
    let out = lnk_cloud(&home, &bin)
        .env("LNK_SECURITY", &security)
        .args(["connect", "fake", "--name", "work"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = lnk_cloud(&home, &bin)
        .env("LNK_SECURITY", &refusing)
        .args(["env", "work", "--json"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "env succeeded");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("s3cret"));
}

/// `lnk cloud env --json` hands an account's keys to the module that ran
/// it, through a pipe; to a terminal it's refused.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn an_accounts_keys_never_reach_a_terminal() {
    let dir = TempDir::new("cloud-env-terminal");
    let home = dir.join("home");
    let accounts = home.join(".lnk/safe/clouds/accounts.toml");
    std::fs::create_dir_all(accounts.parent().unwrap()).unwrap();
    std::fs::write(
        &accounts,
        "schema = 1\n[[account]]\nname = \"work\"\nkind = \"aws\"\nplace = \"us-east-1\"\nidentity = \"\"\n\
         login = \"\"\n\n[account.env]\nAWS_SECRET_ACCESS_KEY = \"s3cret-key\"\n",
    )
    .unwrap();
    let piped = lnk_cloud(&home, &dir)
        .args(["env", "work", "--json"])
        .output()
        .unwrap();
    assert!(piped.status.success(), "{piped:?}");
    assert!(String::from_utf8_lossy(&piped.stdout).contains("s3cret-key"));
    let keychain = link_test_kit::keychain::isolated(&home)
        .map(|(name, value)| format!("{name}={} ", value.display()))
        .unwrap_or_default();
    let line = format!(
        "HOME={} {keychain}{} env work --json",
        home.display(),
        env!("CARGO_BIN_EXE_lnk-cloud")
    );
    let mut script = Command::new("script");
    match cfg!(target_os = "macos") {
        true => script.args(["-q", "/dev/null", "sh", "-c", &line]),
        false => script.args(["-qec", &line, "/dev/null"]),
    };
    if let Ok(out) = script.output() {
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(!text.contains("s3cret-key"), "{text}");
        assert!(text.contains("lnk cloud list"), "{text}");
    }
}
