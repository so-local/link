//! What a harness needs beyond the agent's grants: asked once, its own
//! risks named, a widening switch asked, a bad answer refused.

use super::*;

/// Its proxy on this machine's loopback is the last port of its block,
/// `main`'s as every agent's, kept from one start to the next.
#[test]
fn an_unsandboxed_agent_keeps_its_block_and_so_its_proxy_port() {
    let mut world = World::new("own-proxy");
    world.sandboxed = false;
    ok(&world.start("main", &["create", "main", "--harness", "needy"]));
    let saved =
        || serde_json::Value::Object(crate::common::record(&world.home, "main", "here.toml"));
    let first = saved();
    assert!(first["port"].as_u64().is_some(), "{first}");
    assert!(first.get("proxy_port").is_none(), "{first}");
    ok(&world.start("main", &["start"]));
    assert_eq!(saved()["port"], first["port"]);
    assert_eq!(saved().get("block"), first.get("block"));
}

#[test]
fn a_harness_asks_for_what_it_needs_once() {
    let world = World::new("needs");
    world.needs(
        r#"{"needs": [{"spec": {"network": {"mode": "host"}},
                       "why": "its websockets ignore HTTPS_PROXY"}]}"#,
    );
    // No terminal and no --yes: refused, naming --yes, and nothing granted.
    let refused = failed(&world.start("main", &["create", "main", "--harness", "needy"]));
    assert!(
        refused.contains("needy asks for --network.mode host") && refused.contains("--yes"),
        "{refused}"
    );
    assert!(!world.grants("needy").exists(), "granted unasked");

    // Allowed: granted to the harness, with the needs it answered, and
    // said every start.
    let started = ok(&world.start("main", &["start", "--yes"]));
    assert!(
        started.contains("needy asks to do more than Link's defaults allow.")
            && started.contains("--network.mode host")
            && started.contains("needy says: its websockets ignore HTTPS_PROXY")
            && started.contains("Allowed by --yes."),
        "{started}"
    );
    assert!(
        started.contains(
            "needy runs with --network.mode host, beyond Link's defaults (lnk agent harness-permission remove \
             --network.mode host --harness needy)"
        ),
        "{started}"
    );
    let grants = read(&world.grants("needy"));
    assert!(
        grants.contains("[network]\nmode = \"host\"") && grants.contains("[asked.network]"),
        "{grants}"
    );
    // The harness's, not the agent's: its own file has nothing of it.
    let own = read(&world.sandbox("main"));
    assert!(!own.contains("[network]"), "{own}");
    // `needs` read the agent's settings, never its key.
    let handed = read(&world.bin.join("needs.in"));
    assert!(
        handed.contains("\"provider\":\"anthropic\"") && !handed.contains("sk-ant-test-key"),
        "{handed}"
    );
    // Asked before the network as it is was its: behind its proxy then,
    // in the sandbox. With it, no more, and so told as it's configured.
    let configured = read(
        &world
            .home
            .join(".lnk/data/agents/main/harnesses/needy/configure.in"),
    );
    if world.sandboxed {
        assert!(handed.contains("\"proxied\":true"), "{handed}");
        assert!(!configured.contains("proxied"), "{configured}");
    }
    // Nor in its environment.
    let env = read(&world.bin.join("needs.env"));
    assert!(
        env.contains("PATH=") && !env.contains("ANTHROPIC_API_KEY") && !env.contains("sk-ant"),
        "{env}"
    );

    // Answered: the next start asks nothing, and still says it.
    let again = ok(&world.start("main", &["start"]));
    assert!(!again.contains("asks to do more"), "{again}");
    assert!(
        again.contains("needy runs with --network.mode host"),
        "{again}"
    );
    let json = status(&world, "main");
    assert_eq!(
        json["beyond_defaults"],
        serde_json::json!(["--network.mode host"])
    );
    assert_eq!(json["spec"]["network"]["mode"], "host");
    assert_eq!(json["spec"]["internet"]["hosts"], serde_json::json!(["*"]));
    // Shown with where it came from.
    let shown = ok(&world.lnk(&["show", "--permissions"]));
    assert!(
        shown.contains("needy's") && shown.contains("yours"),
        "{shown}"
    );
    // A Mac's own, only Link's default here: not listed elsewhere.
    #[cfg(not(target_os = "macos"))]
    assert!(!shown.contains("--commands.developer_tools"), "{shown}");

    // Taken back: not asked again, and nothing beyond the defaults.
    let denied = ok(&world.lnk(&[
        "harness-permission",
        "remove",
        "--network.mode",
        "host",
        "--harness",
        "needy",
    ]));
    assert!(
        denied.contains("lnk agent harness-permission add --network.mode host --harness needy")
            && denied.contains("lnk agent harness reset --harness needy"),
        "{denied}"
    );
    let denied = ok(&world.start("main", &["start"]));
    assert!(
        !denied.contains("asks to do more") && !denied.contains("beyond Link's defaults"),
        "{denied}"
    );
    let configured = read(
        &world
            .home
            .join(".lnk/data/agents/main/harnesses/needy/configure.in"),
    );
    if world.sandboxed {
        assert!(configured.contains("\"proxied\":true"), "{configured}");
    }
    let shown = ok(&world.lnk(&["harness-permission", "list", "--harness", "needy"]));
    assert!(
        shown.contains("It asked for") && shown.contains("--network.mode host"),
        "{shown}"
    );
    // Given back by its flags: only what it asks for.
    let allowed = ok(&world.lnk(&[
        "harness-permission",
        "add",
        "--network.mode",
        "host",
        "--harness",
        "needy",
    ]));
    assert!(
        allowed.contains("The grants of needy now include --network.mode host"),
        "{allowed}"
    );
    assert!(read(&world.grants("needy")).contains("[network]\nmode = \"host\""));
    let refused = failed(&world.lnk(&[
        "harness-permission",
        "add",
        "--files.read",
        "~",
        "--harness",
        "needy",
    ]));
    assert!(
        refused.contains("needy doesn't ask for --files.read '~'")
            && refused.contains("grant your agent what you choose"),
        "{refused}"
    );
    // A permission's old name is no risk's, nor any flag.
    let refused = failed(&world.lnk(&["harness-permission", "add", "lan", "--harness", "needy"]));
    assert!(refused.contains("isn't a risk's name"), "{refused}");
    // Taken back, then asked again: its next start asks. The old form,
    // the harness first, still works for a release, and says the new.
    ok(&world.lnk(&[
        "harness-permission",
        "remove",
        "--network.mode",
        "host",
        "--harness",
        "needy",
    ]));
    ok(&world.lnk(&["harness", "reset", "--harness", "needy"]));
    let asked = failed(&world.start("main", &["start"]));
    assert!(
        asked.contains("needy asks for --network.mode host"),
        "{asked}"
    );
    ok(&world.start("main", &["start", "--yes"]));

    // Removed, its grants go with it: the next start asks again. The
    // harness is named by --harness.
    let unnamed = failed(&world.lnk(&["harness", "delete", "--yes"]));
    assert!(
        unnamed.contains("names the harness it removes"),
        "{unnamed}"
    );
    ok(&world.lnk(&["harness", "delete", "--harness", "needy", "--yes"]));
    assert!(!world.grants("needy").exists());
    let asked = failed(&world.start("main", &["start"]));
    assert!(asked.contains("--yes"), "{asked}");

    // A need or a field Link doesn't know is refused, not ignored.
    world.needs(r#"{"needs": [{"spec": {"root": true}, "why": "x"}]}"#);
    let unknown = failed(&world.start("main", &["start", "--yes"]));
    assert!(unknown.contains("what lnk-needy needs"), "{unknown}");
    world.needs(r#"{"permissions": [{"name": "lan", "why": "x"}]}"#);
    let field = failed(&world.start("main", &["start", "--yes"]));
    assert!(
        field.contains("what lnk-needy needs") && field.contains("unknown field permissions"),
        "{field}"
    );

    // No sandbox at all is never asked, nor granted: a harness that
    // needs it runs only for an agent started `--no-sandbox`, which
    // --yes never implies, and from then on until `--sandbox`.
    if world.skips() {
        return;
    }
    world.needs(r#"{"unsandboxed": {"why": "it runs ps, which is setuid"}}"#);
    let refused = failed(&world.start("main", &["start", "--yes"]));
    assert!(
        refused.contains("needy runs only with no sandbox.")
            && refused.contains("It says: it runs ps, which is setuid")
            && refused.contains("lnk agent start --agent main --no-sandbox")
            && !refused.contains("asks to do more"),
        "{refused}"
    );
    assert!(!read(&world.grants("needy")).contains("unsandboxed"));
    let open = ok(&world.start("main", &["start", "--no-sandbox"]));
    assert!(
        !open.contains("asks to do more") && open.contains("There is no sandbox."),
        "{open}"
    );
    assert!(!read(&world.grants("needy")).contains("unsandboxed"));
    // Kept: the next start, with no flag, runs without one too, warned.
    let again = ok(&world.start("main", &["start"]));
    assert!(again.contains("There is no sandbox."), "{again}");
    let shown = text(&world.lnk(&["-a", "main", "show"]));
    assert!(shown.contains("lnk agent start --sandbox"), "{shown}");
    assert_eq!(
        status(&world, "main")["beyond_defaults"],
        serde_json::json!([])
    );
    // Never a harness's grant.
    let refused = failed(&world.lnk(&[
        "harness-permission",
        "add",
        "unsandboxed",
        "--harness",
        "needy",
    ]));
    assert!(refused.contains("never a grant"), "{refused}");
    // Back on: then the harness can't start, and says how.
    let refused = failed(&world.start("main", &["start", "--sandbox"]));
    assert!(
        refused.contains("needy runs only with no sandbox"),
        "{refused}"
    );
}

/// A risk the harness names itself is asked for as a permission is, and
/// handed to it allowed; one it runs without is left off without a
/// terminal, and allowed with a yes in one.
#[test]
fn a_harness_names_risks_of_its_own() {
    let world = World::new("risks");
    let configured = || {
        read(
            &world
                .home
                .join(".lnk/data/agents/main/harnesses/needy/configure.in"),
        )
    };
    world.needs(r#"{"risks": [{"name": "bridge", "why": "its bridge asks for no credential"}]}"#);
    let refused = failed(&world.start("main", &["create", "main", "--harness", "needy"]));
    assert!(
        refused.contains("needy asks for bridge") && refused.contains("--yes"),
        "{refused}"
    );
    let started = ok(&world.start("main", &["start", "--yes"]));
    assert!(
        started.contains("needy says: its bridge asks for no credential")
            && started.contains("needy runs with bridge, beyond Link's defaults"),
        "{started}"
    );
    assert!(
        read(&world.grants("needy")).contains("risks = [\"bridge\"]"),
        "{}",
        read(&world.grants("needy"))
    );
    assert!(
        configured().contains(r#""allowed":["bridge"]"#),
        "{}",
        configured()
    );
    // Taken back: not handed to it, and not asked again.
    ok(&world.lnk(&[
        "harness-permission",
        "remove",
        "bridge",
        "--harness",
        "needy",
    ]));
    let denied = ok(&world.start("main", &["start"]));
    assert!(!denied.contains("asks to do more"), "{denied}");
    assert!(!configured().contains("allowed"), "{}", configured());
    // A risk is the harness's: never granted to an agent.
    let refused = failed(&world.lnk(&["permission", "add", "bridge"]));
    assert!(refused.contains("unexpected argument"), "{refused}");
    // The harness's, given back by name; a risk it never named, refused.
    ok(&world.lnk(&["harness-permission", "add", "bridge", "--harness", "needy"]));
    let back = ok(&world.start("main", &["start"]));
    assert!(back.contains("needy runs with bridge"), "{back}");
    assert!(configured().contains(r#""allowed":["bridge"]"#));
    let refused =
        failed(&world.lnk(&["harness-permission", "add", "bridgee", "--harness", "needy"]));
    assert!(
        refused.contains("needy doesn't ask for bridgee"),
        "{refused}"
    );
    // Nor taken back: a risk it never named is refused, naming it.
    let refused = failed(&world.lnk(&[
        "harness-permission",
        "remove",
        "bridgee",
        "--harness",
        "needy",
    ]));
    assert!(
        refused.contains("needy never asks for bridgee"),
        "{refused}"
    );

    // One it runs without: left off without a terminal, and said.
    world.needs(
        r#"{"risks": [{"name": "browser", "why": "its ports", "without": "its browser stays off"}]}"#,
    );
    let off = ok(&world.start("main", &["start", "--yes"]));
    assert!(
        off.contains("needy can also run with this, off unless you allow it.")
            && off.contains("its browser stays off")
            && off.contains(
                "Left off: lnk agent harness-permission add browser --harness needy allows it."
            ),
        "{off}"
    );
    assert!(!configured().contains("browser"), "{}", configured());
    // A yes in a terminal allows it.
    let files = world.home.join("Link/Agents/main/home");
    let start = [
        "start",
        "--foreground",
        "--run.files",
        &files.display().to_string(),
    ]
    .map(String::from);
    let start: Vec<&str> = start.iter().map(String::as_str).collect();
    let mut start = start;
    if !world.sandboxed {
        start.push("--no-sandbox");
    }
    // Its tools, asked once in a terminal: none; then the yes.
    let on = ok(&world.in_terminal(&start, "n\nn\nn\nn\ny\n"));
    assert!(on.contains("Allow this for needy"), "{on}");
    assert!(
        configured().contains(r#""allowed":["bridge","browser"]"#),
        "{}",
        configured()
    );
    // Taken back, then asked again.
    ok(&world.lnk(&[
        "harness-permission",
        "remove",
        "browser",
        "--harness",
        "needy",
    ]));
    let off = ok(&world.start("main", &["start"]));
    assert!(!off.contains("off unless you allow"), "{off}");
    ok(&world.lnk(&["harness", "reset", "--harness", "needy"]));
    let asked = ok(&world.start("main", &["start"]));
    assert!(asked.contains("off unless you allow"), "{asked}");
    // Or allowed by name, no terminal needed.
    ok(&world.lnk(&["harness-permission", "add", "browser", "--harness", "needy"]));
    ok(&world.start("main", &["start"]));
    assert!(
        configured().contains(r#""allowed":["bridge","browser"]"#),
        "{}",
        configured()
    );
}

/// Switching an agent to a harness that needs more than its own is shown,
/// line by line, and asked: refused without a terminal, nothing changed;
/// allowed with `--yes`. What it may do less of is said too. What you
/// grant the agent comes along and asks nothing.
#[test]
fn a_switch_that_widens_what_it_may_do_is_asked() {
    let world = World::new("switch");
    if !world.sandboxed {
        // `use` runs the harness sandboxed, which needs bubblewrap here.
        return;
    }
    ok(&world.start("main", &["create", "main", "--harness", "plain"]));
    ok(&world.lnk(&["permission", "add", "--files.read", "~"]));
    world.needs(
        r#"{"needs": [{"spec": {"network": {"mode": "host"}}, "why": "its sockets"},
                      {"spec": {"files": {"read": ["~"]}}, "why": "it reads"}]}"#,
    );
    let refused = failed(&world.lnk(&["harness", "set", "needy", "--foreground"]));
    assert!(
        refused.contains("needy may do more on this machine than plain.")
            && refused.contains("--network.mode host")
            && !refused.contains("+ --files.read")
            && refused.contains("This holds for main, and every other agent here that runs needy.")
            && refused.contains("switching main to needy gives it --network.mode host")
            && refused.contains("Nothing changed")
            && refused.contains("Still on plain")
            && !refused.contains("again later"),
        "{refused}"
    );
    assert_eq!(status(&world, "main")["harness"], "plain");
    let switched = ok(&world.lnk(&["harness", "set", "needy", "--foreground", "--yes"]));
    assert!(switched.contains("Allowed by --yes."), "{switched}");
    let json = status(&world, "main");
    assert_eq!(json["harness"], "needy");
    assert_eq!(
        json["beyond_defaults"],
        serde_json::json!(["--files.read '~'", "--network.mode host"])
    );
    // Back: less, said, and nothing asked; its own home read stays.
    let back = ok(&world.lnk(&["harness", "set", "plain", "--foreground"]));
    assert!(
        back.contains("plain may do less than needy here: not --network.mode host.")
            && !back.contains("may do more"),
        "{back}"
    );
    assert_eq!(
        status(&world, "main")["beyond_defaults"],
        serde_json::json!(["--files.read '~'"])
    );
}

/// An adapter without `needs`, whatever version it speaks, needs nothing
/// more; one that answers what isn't a need is refused.
#[test]
fn an_adapter_without_needs_needs_nothing_and_a_bad_answer_is_refused() {
    let world = World::new("no-needs");
    // It doesn't know the command: an error exit, as for any it doesn't.
    let started = ok(&world.start("main", &["create", "main", "--harness", "needy"]));
    assert!(
        !started.contains("asks to do more") && !started.contains("beyond Link's defaults"),
        "{started}"
    );
    assert!(!world.grants("needy").exists(), "granted unasked");
    // It answers nothing.
    world.needs("");
    let started = ok(&world.start("main", &["start"]));
    assert!(!started.contains("asks to do more"), "{started}");
    // An answer that isn't one: refused, and nothing granted.
    world.needs("lan, please");
    let refused = failed(&world.start("main", &["start", "--yes"]));
    assert!(
        refused.contains("reading what lnk-needy needs said"),
        "{refused}"
    );
    world.needs(r#"{"needs": "lan"}"#);
    let refused = failed(&world.start("main", &["start", "--yes"]));
    assert!(
        refused.contains("reading what lnk-needy needs said"),
        "{refused}"
    );
    assert!(!world.grants("needy").exists(), "granted on a bad answer");
}

/// A switch to a harness that takes in no conversation says what
/// stays behind and asks: refused without a terminal, nothing changed;
/// agreed with `--yes`. Between two harnesses giving and taking every
/// conversation over ASP, nothing is asked.
#[test]
fn a_switch_says_what_stays_behind() {
    let world = World::new("behind");
    if !world.sandboxed {
        // `use` runs the harness sandboxed, which needs bubblewrap here.
        return;
    }
    crate::common::write_adapter(
        &world.bin.join("lnk-own"),
        "#!/bin/sh\ncase \"$1\" in\n  command) printf '{\"argv\":[\"/bin/true\"],\"ports\":[]}' ;;\n  *) exit 0 ;;\nesac\n",
    )
    .unwrap();
    ok(&world.start("main", &["create", "main", "--harness", "plain"]));
    let refused = failed(&world.lnk(&["harness", "set", "own", "--foreground"]));
    assert!(
        refused.contains("own won't carry plain's conversations")
            && refused.contains("They stay with plain")
            && refused.contains("Switching main to own leaves plain's conversations behind")
            && refused.contains("Still on plain"),
        "{refused}"
    );
    assert_eq!(status(&world, "main")["harness"], "plain");
    let switched = ok(&world.lnk(&["harness", "set", "own", "--foreground", "--yes"]));
    assert!(switched.contains("Agreed by --yes."), "{switched}");
    assert_eq!(status(&world, "main")["harness"], "own");
    // Between two that give and take every conversation over ASP.
    ok(&world.lnk(&["harness", "set", "plain", "--foreground", "--yes"]));
    let quiet = ok(&world.lnk(&["harness", "set", "needy", "--foreground"]));
    assert!(!quiet.contains("won't carry"), "{quiet}");
}

/// A harness asking to run programs says which, beside the need, and
/// one asking for any program is refused: a grant names what it runs.
#[test]
fn a_harness_asking_for_programs_says_so_plainly() {
    // Link's default allows any program; these agents run only their
    // toolchains', so a harness asking for more is asked.
    let world = World::new("programs");
    world.needs(r#"{"needs": [{"spec": {"commands": {"allow": ["*"]}}, "why": "its runtime"}]}"#);
    world.toolchains_only("main");
    let refused = failed(&world.start("main", &["create", "main", "--harness", "needy"]));
    assert!(
        refused.contains(
            "This changes which programs needy may run: '*', beside those [commands] lists"
        ),
        "{refused}"
    );
    world.needs(
        r#"{"needs": [{"spec": {"commands": {"toolchains": ["jvm"]}}, "why": "its runtime"}]}"#,
    );
    world.toolchains_only("third");
    let refused = failed(&world.start("third", &["create", "third", "--harness", "needy"]));
    assert!(
        refused.contains("--commands.toolchains jvm")
            && refused.contains("a toolchain's programs, beyond your [commands]")
            && refused.contains(
                "This changes which programs needy may run: the jvm toolchain's programs"
            ),
        "{refused}"
    );
    world.needs(r#"{"needs": [{"spec": {"commands": {"allow": ["ps"]}}, "why": "it lists its processes"}]}"#);
    world.toolchains_only("other");
    let refused = failed(&world.start("other", &["create", "other", "--harness", "needy"]));
    assert!(
        refused.contains(
            "This changes which programs needy may run: ps, beside those [commands] lists"
        ),
        "{refused}"
    );
}
