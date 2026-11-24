use super::*;

fn parse(text: &str) -> Spec {
    link_spec::parse(text).unwrap()
}

#[test]
fn an_os_table_replaces_its_keys_but_hide_adds_up() {
    let spec = parse(
        "[files]\nwrite = [\"./a\"]\nhide = [\"./s\"]\n[files.macos]\nwrite = [\"./m\"]\nhide = [\"./t\"]\n\
             [commands.macos]\nallow = [\"node\"]\n",
    );
    let mac = spec.on(Os::MacOs).unwrap();
    let files = mac.files.unwrap();
    assert_eq!(files.write.unwrap(), ["./m"]);
    assert_eq!(files.hide.unwrap(), ["./s", "./t"]);
    assert_eq!(mac.commands.unwrap().allow.unwrap(), ["node"]);
    // Only macOS's table: closed on Linux.
    let linux = spec.on(Os::Linux).unwrap();
    assert_eq!(linux.files.unwrap().write.unwrap(), ["./a"]);
    assert!(linux.commands.is_none());
    let nested = parse("[files.macos.linux]\nwrite = [\"x\"]");
    assert!(nested.on(Os::MacOs).is_err());
    // What a spec made on one OS leaves out is said by name.
    assert_eq!(
        spec.others_than(Os::Linux),
        ["[commands.macos]", "[files.macos]"]
    );
    assert!(spec.others_than(Os::MacOs).is_empty());
}

#[test]
fn a_flag_replaces_a_key_but_never_drops_a_hide() {
    let file = parse(
        "[files]\nwrite = [\"./a\"]\nhide = [\"./secrets\"]\n[internet]\nhosts = [\"a.com\"]",
    );
    let flags = Spec {
        files: Some(Files {
            hide: Some(vec!["./other".into()]),
            ..Default::default()
        }),
        internet: Some(Internet {
            hosts: Some(vec!["b.com".into()]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let both = file.with(&flags);
    let files = both.files.unwrap();
    assert_eq!(files.write.unwrap(), ["./a"]);
    assert_eq!(files.hide.unwrap(), ["./secrets", "./other"]);
    assert_eq!(both.internet.unwrap().hosts.unwrap(), ["b.com"]);
}

#[test]
fn a_spec_prints_and_reads_back_the_same() {
    let text = "name = \"app\"\nversion = 2\n[files]\nwrite = [\"~/code\"]\n[localhost]\nports = [8080]\n\
                    [commands]\nallow = [\"node\"]\n[commands.macos]\nallow = [\"node\", \"git\"]\n";
    let spec = parse(text);
    let printed = link_spec::print(&spec).unwrap();
    assert_eq!(parse(&printed), spec);
}

#[test]
fn grants_join_and_are_taken_back_item_by_item() {
    let agent = parse(
        "[internet]\nhosts = [\"*\"]\ntcp = [\"github.com:22\"]\n[commands]\nallow = [\"git\"]\n\
             [commands.macos]\ndeveloper_tools = true\n",
    );
    let more = parse("[localhost]\nports = [5432]\n[commands]\nallow = [\"git\", \"node\"]\n");
    let both = agent.join(&more);
    assert_eq!(
        both.commands.as_ref().unwrap().allow.as_ref().unwrap(),
        &["git", "node"]
    );
    assert_eq!(
        both.localhost.as_ref().unwrap().ports.as_ref().unwrap(),
        &[Port::Number(5432)]
    );
    assert!(both.covers(&more, Os::Linux).unwrap());
    assert!(both.covers(&agent, Os::MacOs).unwrap());
    assert!(!agent.covers(&more, Os::Linux).unwrap());
    // `*` covers any host; a narrower list never covers a wider one.
    let one = parse("[internet]\nhosts = [\"a.com\"]\n");
    assert!(agent.covers(&one, Os::Linux).unwrap());
    assert!(!one.covers(&agent, Os::Linux).unwrap());
    let host = parse("[network]\nmode = \"host\"\n");
    assert!(!agent.covers(&host, Os::Linux).unwrap());
    // Taken back: the hosts go, and `tcp` with them; a value given goes.
    let less = parse("[internet]\nhosts = [\"*\"]\n[commands]\ndeveloper_tools = true\n");
    let after = both.without(&less);
    assert!(after.internet.is_none());
    let commands = after.commands.unwrap();
    assert_eq!(commands.allow.unwrap(), ["git", "node"]);
    assert!(commands.macos.is_none());
    // The last item of a section takes the section.
    let none = more.without(&parse("[localhost]\nports = [5432]\n"));
    assert!(none.localhost.is_none());
}

#[test]
fn taking_away_never_widens() {
    let any =
        parse("[internet]\nhosts = [\"*\"]\nper_minute = 60\n[browser]\nhosts = [\"a.com\"]\n");
    // Its last host taken, `[browser]` closes: an empty list is never
    // written, which would read as any.
    let closed = any.without(&parse("[browser]\nhosts = [\"a.com\"]\n"));
    assert!(closed.browser.is_none());
    assert_eq!(
        any.widened_by_taking(&parse("[browser]\nhosts = [\"a.com\"]\n"))
            .unwrap(),
        None
    );
    // One host of any, or a limit lifted: refused.
    for widening in [
        "[internet]\nhosts = [\"a.com\"]\n",
        "[internet]\nhosts = [\"*\"]\nper_minute = 60\n",
    ] {
        assert!(
            any.widened_by_taking(&parse(widening)).unwrap().is_some(),
            "{widening}"
        );
    }
    assert_eq!(
        any.widened_by_taking(&parse("[internet]\nhosts = [\"*\"]\n"))
            .unwrap(),
        None
    );
}

/// An OS's table left without a key would fall back to the section's
/// own, which opens more there: refused on either OS, whichever runs.
#[test]
fn a_deny_never_widens_through_an_os_table() {
    for (spec, less) in [
        (
            "[internet]\nhosts = [\"*\"]\n[internet.linux]\nhosts = [\"a.com\"]\n",
            "[internet]\nhosts = [\"a.com\"]\n",
        ),
        (
            "[commands]\nallow = [\"sh\", \"git\"]\n[commands.linux]\nallow = [\"git\"]\n",
            "[commands]\nallow = [\"git\"]\n",
        ),
        (
            "[commands]\nallow = [\"sh\", \"git\"]\n[commands.macos]\nallow = [\"git\"]\n",
            "[commands]\nallow = [\"git\"]\n",
        ),
        (
            "[internet]\nhosts = [\"a.com\"]\n[internet.macos]\nhosts = [\"b.com\"]\n\
                 tcp = [\"c.com:22\"]\n",
            "[internet]\nhosts = [\"b.com\"]\n",
        ),
    ] {
        let (spec, less) = (parse(spec), parse(less));
        let why = spec.widened_by_taking(&less).unwrap();
        assert!(why.is_some(), "{spec:?} without {less:?}");
    }
    // Taken from the section and its table alike, it narrows: done.
    let spec = parse(
        "[commands]\nallow = [\"sh\", \"git\"]\n[commands.linux]\nallow = [\"git\", \"node\"]\n",
    );
    let less = parse("[commands]\nallow = [\"git\"]\n");
    assert_eq!(spec.widened_by_taking(&less).unwrap(), None);
    let after = spec.without(&less);
    assert_eq!(
        after
            .on(Os::Linux)
            .unwrap()
            .commands
            .unwrap()
            .allow
            .unwrap(),
        ["node"]
    );
    // A folder no longer hidden would show: refused.
    let hidden = parse("[files]\nwrite = [\"/w\"]\nhide = [\"/w/s\"]\n");
    assert!(
        hidden
            .widened_by_taking(&parse("[files]\nhide = [\"/w/s\"]\n"))
            .unwrap()
            .is_some()
    );
}

/// `"*"` is the whole of a list it's in: joined with more, it stays
/// alone, as a sandbox takes it.
#[test]
fn any_absorbs_the_rest() {
    for (a, b) in [
        (
            "[internet]\nhosts = [\"*\"]\n",
            "[internet]\nhosts = [\"api.openai.com\"]\n",
        ),
        (
            "[lan]\nhosts = [\"printer.local\"]\n",
            "[lan]\nhosts = [\"*\"]\n",
        ),
        (
            "[browser]\nhosts = [\"*\"]\n",
            "[browser]\nhosts = [\"a.com\"]\n",
        ),
    ] {
        let joined = parse(a).join(&parse(b));
        joined.check_any_alone().unwrap();
        assert_eq!(joined, parse(b).join(&parse(a)), "{a} {b}");
        assert!(link_spec::print(&joined).unwrap().contains("[\"*\"]"));
    }
    let ports =
        parse("[localhost]\nports = [\"*\"]\n").join(&parse("[localhost]\nports = [5432]\n"));
    assert_eq!(
        ports.localhost.unwrap().ports.unwrap(),
        [Port::Word("*".into())]
    );
    for beside in [
        "[internet]\nhosts = [\"*\", \"a.com\"]\n",
        "[commands.linux]\nallow = [\"git\", \"*\"]\n",
    ] {
        assert!(parse(beside).check_any_alone().is_err(), "{beside}");
    }
    parse("[commands]\nallow = [\"*\"]\n")
        .check_any_alone()
        .unwrap();
}

/// A team's folders are grants as every other is: joined, taken away
/// and covered folder by folder, each an item of its own.
#[test]
fn a_teams_folders_are_grants_like_any_other() {
    let have = parse("[teams.research]\nwrite = [\"drafts\"]\nread = [\"sources\"]\n");
    let more = parse("[teams.research]\nwrite = [\"sources\"]\n[teams.ops]\nread = [\".\"]\n");
    let joined = have.join(&more);
    assert_eq!(joined.teams["research"].written(), ["drafts", "sources"]);
    assert!(
        joined.teams["research"].only_read().is_empty(),
        "written, so not only read"
    );
    assert_eq!(joined.teams["ops"].only_read(), ["."]);
    let left = joined.without(&parse("[teams.ops]\nread = [\".\"]\n"));
    assert!(
        !left.teams.contains_key("ops"),
        "a team reaching nothing goes"
    );
    assert!(
        joined
            .covers(&parse("[teams.ops]\nread = [\"notes\"]\n"), Os::Linux)
            .unwrap()
    );
    assert!(
        !have
            .covers(
                &parse("[teams.research]\nwrite = [\"sources\"]\n"),
                Os::Linux
            )
            .unwrap()
    );
    assert_eq!(have.items().len(), 2);
}

/// `[tools]` names your own MCP tools (`lnk sandbox tool add`), never
/// Link's tool modules, so no name there follows history: a release that
/// renamed a Link tool of the same name grants no other of yours.
#[test]
fn your_tool_names_are_read_as_written() {
    let shape = link_spec::schema::<Spec>();
    let sites = link_spec::follow::sites(&shape);
    assert!(sites.is_empty(), "{sites:?}");
    let core = link_spec::history::History::parse(
        "[[renamed]]\nmodule = 'gh-tool'\nnow = 'git-tool'\nsince = '1.1.0'\nuntil = '2.0.0'\n",
    )
    .unwrap();
    let release = link_spec::follow::References::from(core.release_wire([]));
    let read = <Spec as link_spec::Specification>::format()
        .read_following::<Spec>(
            "schema = 1\nkind = 'sandbox'\n[tools]\nnames = ['gh']\n",
            &sites,
            &release,
        )
        .unwrap();
    assert_eq!(read.spec.tools.unwrap().names, Some(vec!["gh".to_string()]));
}

/// The sandbox's sections each name themselves for each platform's
/// overlay: a name marked in one is found at the section and each
/// overlay, three places, never one per nesting the walk could reach.
#[test]
fn a_marked_name_in_a_section_has_three_places() {
    let mut shape = link_spec::schema::<Spec>();
    let names = shape
        .pointer_mut("/$defs/sandbox.tools/properties/names/items")
        .and_then(serde_json::Value::as_object_mut)
        .expect("the tools section names its tools");
    names.insert("x-link-ref".into(), "tool".into());
    let mut shown: Vec<_> = link_spec::follow::sites(&shape)
        .iter()
        .map(|site| site.shown())
        .collect();
    shown.sort();
    assert_eq!(
        shown,
        ["tools.linux.names", "tools.macos.names", "tools.names"]
    );
}
