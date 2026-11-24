//! What `lnk env here` refuses of the secrets a request carries.
use link_env_fixture::*;

/// `lnk env here` checks a request as where it's sent from does: one sent
/// to it straight, a job whose stage runs as root and given a sealed
/// secret, is refused before anything runs, since root runs beside the
/// proxy and could read it.
#[test]
fn a_request_with_a_root_stage_and_a_sealed_secret_is_refused_there() {
    if !box_identity() {
        return;
    }
    use link_env::here::{self, Action, Request};
    use link_env::values::{Given, Sealing};
    let world = World::new("here-root-sealed");
    let mut child = world
        .command(&["env", "here"])
        .env("LNK_TEST_MACHINE_KIND", world.dir.join("box/kind.toml"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (seal_to, _rest) = link_core::announced(stdout).unwrap();
    let given = Given {
        sealed: [(
            "STRIPE_KEY".to_string(),
            link_secretive::Secret::from("sk_live_sealed_value".to_string()),
        )]
        .into(),
        sealing: [(
            "STRIPE_KEY".to_string(),
            toml::from_str::<Sealing>("host = \"api.stripe.com\"").unwrap(),
        )]
        .into(),
        ..Given::default()
    };
    let request = Request {
        workdir: None,
        name: "ci".into(),
        file: None,
        action: Action::Run {
            stages: vec![
                link_env::stage::Run {
                    upstream: None,
                    protected: None,
                    name: "command".into(),
                    run: "touch ../root-ran".into(),
                    sandbox: None,
                    user: Some(link_env::file::User::root()),
                    timeout: None,
                }
                .into(),
            ],
            variables: Default::default(),
            secrets: Default::default(),
            out: None,
            profile: None,
        },
        given: link_secretive::Sealed::seal_json(&given, &seal_to, here::about("ci")).unwrap(),
        mark: "lnk-env-test".into(),
        direct: false,
    };
    let mut input = child.stdin.take().unwrap();
    let mut line = serde_json::to_vec(&request).unwrap();
    line.push(b'\n');
    input.write_all(&line).unwrap();
    let mut said = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut said)
        .unwrap();
    drop(input);
    let status = child.wait().unwrap();
    assert!(!status.success(), "{said}");
    assert!(
        said.contains("a run with a root stage is given no sealed secret"),
        "{said}"
    );
    assert!(!world.dir.join("root-ran").exists());
    assert!(!world.read("as-root").contains("root-ran"));
}

/// `lnk env approval` gives an environment file a secret it asks for
/// ahead of a run, kept for the file's real path, its name and the
/// secret's ID; one it doesn't ask for, or a secret that isn't kept, is
/// refused; `remove` takes it back.
#[test]
fn an_approval_is_given_listed_and_taken_back() {
    let world = World::new("approvals");
    let id = link_id::candidate().unwrap();
    let store = world.dir.join("home/.lnk/safe/secrets/secrets.toml");
    std::fs::create_dir_all(store.parent().unwrap()).unwrap();
    std::fs::write(
        &store,
        format!(
            "[entry.{id}]\nkind = \"named\"\nname = \"NPM_TOKEN\"\nvalue = \"npm_value\"\nto = []\n"
        ),
    )
    .unwrap();
    let spec = world.dir.join("project/app.toml");
    std::fs::write(
        &spec,
        "schema = 1\nname = \"app\"\nversion = 1\n\n[secrets]\nnames = [\"NPM_TOKEN\"]\n",
    )
    .unwrap();
    let spec = spec.display().to_string();
    let added = world.run(&["env", "approval", "add", "NPM_TOKEN", "--spec", &spec]);
    assert!(added.status.success(), "{}", text(&added.stderr));
    let listed = world.run(&["env", "approval", "list", "--json"]);
    assert!(listed.status.success(), "{}", text(&listed.stderr));
    link_test_kit::ids::assert_named_output(&text(&listed.stdout));
    let json: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    let approvals = json["approvals"].as_array().unwrap();
    assert_eq!(approvals.len(), 1, "{json}");
    assert_eq!(approvals[0]["resource"], format!("secret:{id}"));
    assert_eq!(approvals[0]["name"], "app");
    assert_eq!(approvals[0]["asks"], "NPM_TOKEN");
    // Kept in the Safe, the secret by its ID and never its value.
    let kept = world.read("home/.lnk/safe/envs/approvals.toml");
    assert!(kept.contains(&format!("secret:{id}")), "{kept}");
    assert!(!kept.contains("npm_value"), "{kept}");
    // One the file doesn't ask for, and one not kept, are refused.
    let other = world.dir.join("project/other.toml");
    std::fs::write(&other, "schema = 1\nname = \"other\"\nversion = 1\n").unwrap();
    let other = other.display().to_string();
    let unasked = world.run(&["env", "approval", "add", "NPM_TOKEN", "--spec", &other]);
    assert!(!unasked.status.success());
    assert!(
        text(&unasked.stderr).contains("doesn't ask for"),
        "{}",
        text(&unasked.stderr)
    );
    let unknown = world.run(&["env", "approval", "add", "QA_TOKEN", "--spec", &spec]);
    assert!(!unknown.status.success());
    let shown = world.run(&[
        "env",
        "approval",
        "show",
        "NPM_TOKEN",
        "--spec",
        &spec,
        "--json",
    ]);
    let json: serde_json::Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(json["approval"]["resource"], format!("secret:{id}"));
    let removed = world.run(&["env", "approval", "remove", "NPM_TOKEN", "--spec", &spec]);
    assert!(removed.status.success(), "{}", text(&removed.stderr));
    let listed = world.run(&["env", "approval", "list", "--json"]);
    let json: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(json["approvals"], serde_json::json!([]));
}

/// A secret deleted takes its approvals with it: the environment module
/// reads only those whose secret is still kept.
#[test]
fn a_deleted_secrets_approval_is_gone() {
    let world = World::new("approvals-deleted");
    let id = link_id::candidate().unwrap();
    let store = world.dir.join("home/.lnk/safe/secrets/secrets.toml");
    std::fs::create_dir_all(store.parent().unwrap()).unwrap();
    std::fs::write(
        &store,
        format!(
            "[entry.{id}]\nkind = \"named\"\nname = \"NPM_TOKEN\"\nvalue = \"npm_value\"\nto = []\n"
        ),
    )
    .unwrap();
    let spec = world.dir.join("project/app.toml");
    std::fs::write(
        &spec,
        "schema = 1\nname = \"app\"\nversion = 1\n\n[secrets]\nnames = [\"NPM_TOKEN\"]\n",
    )
    .unwrap();
    let spec = spec.display().to_string();
    let added = world.run(&["env", "approval", "add", "NPM_TOKEN", "--spec", &spec]);
    assert!(added.status.success(), "{}", text(&added.stderr));
    std::fs::write(&store, format!("retired = [\"{id}\"]\n")).unwrap();
    let listed = world.run(&["env", "approval", "list", "--json"]);
    assert!(listed.status.success(), "{}", text(&listed.stderr));
    let json: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(json["approvals"], serde_json::json!([]));
}
