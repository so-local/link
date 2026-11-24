use super::*;
use std::time::Duration;

/// A config for profile `profile`, its sandbox `sandbox`.
fn config(sandbox: Vec<String>, profile: PathBuf) -> Config {
    Config {
        sandbox,
        chromium: None,
        profile,
        internet_hosts: Vec::new(),
        private_hosts: Vec::new(),
        floor: Standing::Owner,
    }
}

#[test]
fn a_browser_startup_failure_keeps_its_diagnostic_and_names_the_next_commands() {
    let home = link_test_kit::TempDir::new("browser-startup-guidance");
    let executable = home.join("failed-browser");
    link_test_kit::write_program(&executable, "#!/bin/sh\nprintf '%s\\n' 'Library not loaded: Chrome Framework; Operation not permitted' >&2\nexit 1\n").unwrap();
    let mut browser = Browser::new(Config {
        chromium: Some(executable.clone()),
        ..config(vec![executable.display().to_string()], home.join("Browser"))
    })
    .unwrap();
    let reply = crate::mcp::answer(&mut browser, &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"open","arguments":{"url":"https://example.org"},"_meta":{"link.local/thread":"t"}}}).to_string()).unwrap();
    assert_eq!(reply["result"]["isError"], true);
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("Library not loaded: Chrome Framework; Operation not permitted"),
        "{text}"
    );
    assert!(
        text.contains("lnk agent restart") && text.contains("lnk agent log show"),
        "{text}"
    );
    assert!(text.contains("No page was opened"), "{text}");
}

#[test]
fn a_mac_browser_can_read_its_frameworks_without_other_apps() {
    let browser = browser(&[]);
    let executable = Path::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
    let args = browser.argv(executable);
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--files.read", "/Applications/Google Chrome.app"])
    );
    assert!(
        !args
            .windows(2)
            .any(|pair| pair == ["--files.read", "/Applications"])
    );
    assert!(
        !args
            .windows(2)
            .any(|pair| pair == ["--files.write", "/Applications/Google Chrome.app"])
    );
    let helper = Path::new(
        "/Applications/Google Chrome.app/Contents/Frameworks/Chrome Framework.framework/Helpers/Chrome Helper.app/Contents/MacOS/Chrome Helper",
    );
    assert_eq!(
        program_folder(helper),
        Some(Path::new("/Applications/Google Chrome.app"))
    );
    assert_eq!(
        program_folder(Path::new("/opt/chromium/chrome")),
        Some(Path::new("/opt/chromium"))
    );
}

#[test]
fn a_private_host_is_a_port_here_or_a_host_of_your_network() {
    assert_eq!(
        Private::parse("localhost:8080").unwrap(),
        Private::Port(8080)
    );
    assert_eq!(
        Private::parse("127.0.0.1:3000").unwrap(),
        Private::Port(3000)
    );
    assert_eq!(Private::parse("[::1]:3000").unwrap(), Private::Port(3000));
    assert_eq!(
        Private::parse("nas.local").unwrap(),
        Private::Lan("nas.local".into())
    );
    assert_eq!(
        Private::parse("192.168.1.20").unwrap(),
        Private::Lan("192.168.1.20".into())
    );
    assert!(Private::parse("localhost").is_err());
    assert!(Private::parse("localhost:0").is_err());
    assert!(Private::parse("127.0.0.1").is_err());
}

fn browser(private: &[&str]) -> Browser {
    Browser::new(Config {
        private_hosts: private.iter().map(|h| h.to_string()).collect(),
        ..config(
            vec!["lnk".into(), "sandbox".into(), "run".into()],
            PathBuf::from("/nowhere/Browser"),
        )
    })
    .unwrap()
}

/// Whether `browser` may open `url`.
fn opens(browser: &Browser, url: &str) -> bool {
    guard::allowed(url, &browser.private).is_ok()
}

#[test]
fn only_the_webs_pages_open_and_private_ones_only_named() {
    let none = browser(&[]);
    assert!(opens(&none, "https://example.com/a"));
    for refused in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "chrome://settings",
        "http://127.0.0.1:8080/",
        "http://localhost/",
        "http://169.254.169.254/latest",
        "http://192.168.1.20/",
        "http://[::1]:80/",
        "http://localhost./",
        "http://app.localhost/",
        "http://APP.LOCALHOST.:8080/",
        "not a url",
    ] {
        assert!(!opens(&none, refused), "{refused}");
    }
    let named = browser(&["localhost:8080", "192.168.1.20"]);
    assert!(opens(&named, "http://127.0.0.1:8080/"));
    assert!(opens(&named, "http://localhost:8080/x"));
    assert!(opens(&named, "http://app.localhost:8080/x"));
    assert!(!opens(&named, "http://127.0.0.1:8081/"));
    assert!(!opens(&named, "http://app.localhost:8081/"));
    assert!(opens(&named, "http://192.168.1.20/"));
    assert!(!opens(&named, "http://192.168.1.21/"));
}

/// A range named opens the addresses in it alone, never any private
/// address or name.
#[test]
fn a_private_range_opens_only_its_own_addresses() {
    let range = browser(&["192.168.1.0/24", "fd00::/8"]);
    assert!(opens(&range, "http://192.168.1.77/"));
    assert!(opens(&range, "http://[fd12::1]/"));
    for refused in [
        "http://192.168.2.1/",
        "http://10.0.0.1/",
        "http://127.0.0.1:8080/",
        "http://localhost/",
        "http://169.254.169.254/latest",
        "http://[fe80::1]/",
    ] {
        assert!(!opens(&range, refused), "{refused}");
    }
}

#[test]
fn your_own_browsers_profile_is_refused() {
    let home = std::env::var("HOME").unwrap();
    for own in [
        ".config/google-chrome",
        ".config/chromium/Default",
        ".config/microsoft-edge-beta",
        ".var/app/com.google.Chrome/config",
        "Library/Application Support/Google/Chrome Canary",
    ] {
        let config = config(vec!["lnk".into()], PathBuf::from(&home).join(own));
        assert!(Browser::new(config).is_err(), "{own}");
    }
    let config = config(Vec::new(), PathBuf::from("/nowhere/Browser"));
    assert!(Browser::new(config).is_err(), "no sandbox");
}

#[test]
fn a_page_is_read_a_part_at_a_time() {
    let text = "a\n".repeat(100);
    let (first, next) = part(&text, 0, 51);
    assert_eq!(first.len(), 50);
    assert_eq!(next, Some(50));
    let (rest, next) = part(&text, 50, 1000);
    assert_eq!(rest.len(), 150);
    assert_eq!(next, None);
}

#[test]
fn a_distribution_launcher_selects_its_engine() {
    let Some((launcher, engine)) = constants::PACKAGED_CHROMIUMS
        .iter()
        .find(|(launcher, engine)| Path::new(launcher).is_file() && Path::new(engine).is_file())
    else {
        link_test_kit::gate::lacks(
            "a packaged Chromium launcher",
            "this host has no installed distribution launcher and engine pair",
            None,
        );
        return;
    };
    assert_eq!(
        chromium_program(PathBuf::from(launcher)),
        PathBuf::from(engine)
    );
}

#[test]
fn another_browser_program_keeps_its_path() {
    let custom = PathBuf::from("my-browser-program");
    assert_eq!(chromium_program(custom.clone()), custom);
}

/// A program given (an agent's `[tool.browser] program`) is the one
/// started, whatever this machine has on PATH; none given, the one found
/// here, as before.
#[test]
fn a_given_program_is_started_and_none_keeps_the_search() {
    let custom = PathBuf::from("/opt/custom/chrome");
    assert_eq!(chosen(Some(custom.clone())).unwrap(), custom);
    match find_chromium() {
        Some(found) => assert_eq!(chosen(None).unwrap(), found),
        None => {
            let refused = format!("{:#}", chosen(None).unwrap_err());
            assert!(refused.contains("[tool.browser] program"), "{refused}");
        }
    }
}

/// On Linux the browser never starts its own sandbox, whoever runs it:
/// Link's refuses the user namespace it needs, and Link's is the wall.
#[cfg(target_os = "linux")]
#[test]
fn a_linux_browser_runs_without_its_own_sandbox_inside_links() {
    let browser = browser(&[]);
    let chromium = Path::new("/usr/lib/chromium/chromium");
    let args = browser.argv(chromium);
    assert!(args.iter().any(|a| a == "--no-sandbox"), "{args:?}");
}

/// The browser keeps no extension and no system keychain: its saved
/// sign-ins stay in its profile, which its sandbox writes.
#[test]
fn the_browser_runs_without_extensions_or_the_systems_keychain() {
    let args = browser(&[]).argv(Path::new("/opt/chromium/chrome"));
    for flag in [
        "--disable-extensions",
        "--use-mock-keychain",
        "--password-store=basic",
    ] {
        assert!(args.iter().any(|a| a == flag), "{flag}: {args:?}");
    }
}

/// The browser reaches the internet hosts the agent does, and none when
/// it reaches none: never every host.
#[test]
fn the_browser_reaches_the_agents_internet_hosts_alone() {
    let hosts = |given: &[&str]| {
        let browser = Browser::new(Config {
            internet_hosts: given.iter().map(|h| h.to_string()).collect(),
            ..config(vec!["lnk".into()], PathBuf::from("/nowhere/Browser"))
        })
        .unwrap();
        let args = browser.argv(Path::new("/opt/chromium/chrome"));
        args.windows(2)
            .filter(|pair| pair[0] == "--internet.hosts")
            .map(|pair| pair[1].clone())
            .collect::<Vec<_>>()
    };
    assert!(hosts(&[]).is_empty());
    assert_eq!(
        hosts(&["example.com", "*.wikipedia.org"]),
        ["example.com", "*.wikipedia.org"]
    );
    assert_eq!(hosts(&["*"]), ["*"]);
}

/// The browser's program is granted its own folder alone, never the
/// folder a link to it sits in.
#[test]
fn only_the_engines_folder_is_granted() {
    let args = browser(&[]).argv(Path::new("/opt/chromium/chrome"));
    let granted: Vec<_> = args
        .windows(2)
        .filter(|pair| pair[0] == "--files.read" || pair[0] == "--commands.allow")
        .map(|pair| pair[1].as_str())
        .collect();
    assert_eq!(
        granted,
        [
            "/opt/chromium",
            "/bin/sh",
            "/opt/chromium/chrome",
            "/opt/chromium/"
        ]
    );
}

/// A Chromium below the oldest major version supported is refused by
/// name.
#[test]
fn an_old_chromium_is_refused() {
    let least = settings::get().minimum_major;
    let program = Path::new("/opt/chromium/chrome");
    assert!(supported(&format!("HeadlessChrome/{least}.0.1.2"), program).is_ok());
    assert!(supported(&format!("Chrome/{}.0", least + 10), program).is_ok());
    let old = supported(&format!("HeadlessChrome/{}.0.1.2", least - 1), program).unwrap_err();
    assert!(
        format!("{old:#}").contains(&format!("Chromium {least} or newer")),
        "{old:#}"
    );
    assert!(supported("", program).is_err());
}

/// Each call's page is its conversation's and standing's: a member's
/// turn in an owner's chat, or a call claiming more than the floor,
/// never stands as the owner.
#[test]
fn a_call_is_its_conversations_at_its_standing() {
    let who = |meta: Value, floor| Who::of(Some(&meta), floor).unwrap();
    let owner = who(
        serde_json::json!({"link.local/thread": "telegram:42", "link.local/standing": "owner"}),
        Standing::Owner,
    );
    assert_eq!(
        (owner.conversation.as_str(), owner.owner),
        ("telegram:42", true)
    );
    let member = who(
        serde_json::json!({"link.local/thread": "telegram:42", "link.local/standing": "member"}),
        Standing::Owner,
    );
    assert!(!member.owner);
    assert_ne!(owner, member);
    // A claim never goes above the floor, and nonsense is a stranger's.
    assert!(
        !who(
            serde_json::json!({"link.local/thread": "t", "link.local/standing": "owner"}),
            Standing::Member
        )
        .owner
    );
    assert!(
        !who(
            serde_json::json!({"link.local/thread": "t", "link.local/standing": "king"}),
            Standing::Owner
        )
        .owner
    );
    // No claim: the floor.
    let thread = serde_json::json!({"link.local/thread": "t"});
    assert!(who(thread.clone(), Standing::Owner).owner);
    assert!(!who(thread, Standing::Member).owner);
}

/// A call that names no conversation is refused, owner floor or not,
/// before the browser starts: under a harness that names none, every
/// conversation would share one page and its sign-ins.
#[test]
fn a_call_naming_no_conversation_is_refused() {
    for meta in [
        None,
        Some(serde_json::json!({"link.local/standing": "owner"})),
        Some(serde_json::json!({"link.local/thread": ""})),
    ] {
        let err = Who::of(meta.as_ref(), Standing::Owner).unwrap_err();
        assert!(
            format!("{err:#}").contains("names no conversation"),
            "{err:#}"
        );
    }
    let home = link_test_kit::TempDir::new("browser-no-conversation");
    let executable = home.join("never-run");
    link_test_kit::write_program(&executable, "#!/bin/sh\nexit 1\n").unwrap();
    let mut browser = Browser::new(Config {
        chromium: Some(executable.clone()),
        ..config(vec![executable.display().to_string()], home.join("Browser"))
    })
    .unwrap();
    let call = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "open", "arguments": {"url": "https://example.org"}}});
    let reply = crate::mcp::answer(&mut browser, &call.to_string()).unwrap();
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("names no conversation"), "{text}");
    assert!(!home.join("Browser").exists(), "the browser started");
}

/// Past `pages_max`, the page used least recently is closed, and any
/// unused for `idle_ms`; the calling conversation's never is.
#[test]
fn idle_and_least_recently_used_pages_are_closed() {
    let settings = settings::get();
    let now = Instant::now() + settings.idle() * 2;
    let who = |n: usize| Who {
        conversation: format!("c{n}"),
        owner: false,
    };
    let second = Duration::from_secs(1);
    // A full set, each used a second after the last, the caller's new.
    let used: Vec<(Who, Instant)> = (0..settings.pages_max)
        .map(|n| (who(n), now - second * (settings.pages_max - n) as u32))
        .collect();
    assert_eq!(evicted(&used, &who(99), now), vec![who(0)]);
    // The caller's own is never closed, however old.
    assert!(evicted(&used, &who(0), now).is_empty());
    // One idle past `idle_ms` is closed with room to spare.
    let idle = vec![(who(1), now - settings.idle()), (who(2), now - second)];
    assert_eq!(evicted(&idle, &who(3), now), vec![who(1)]);
}

/// A call's arguments are held to its declaration before the browser
/// starts: this one would fail to start, and is never run.
#[test]
fn a_call_breaking_its_declaration_is_refused_before_the_browser_starts() {
    let home = link_test_kit::TempDir::new("browser-declared");
    let executable = home.join("never-run");
    link_test_kit::write_program(&executable, "#!/bin/sh\nexit 1\n").unwrap();
    let mut browser = Browser::new(Config {
        chromium: Some(executable.clone()),
        ..config(vec![executable.display().to_string()], home.join("Browser"))
    })
    .unwrap();
    for (arguments, says) in [
        (serde_json::json!({"url": 5}), "invalid type"),
        (
            serde_json::json!({"url": "https://example.org", "tab": 2}),
            "unknown field `tab`",
        ),
    ] {
        let call = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "open", "arguments": arguments}});
        let reply = crate::mcp::answer(&mut browser, &call.to_string()).unwrap();
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains(says), "{text}");
    }
}

/// A browser in a folder of the system's programs is allowed alone,
/// never every program beside it; one in a folder of its own allows its
/// helpers there.
#[test]
fn a_browser_in_a_system_program_folder_allows_only_itself() {
    let browser = browser(&[]);
    let allowed = |program: &str| -> Vec<String> {
        browser
            .argv(Path::new(program))
            .windows(2)
            .filter(|pair| pair[0] == "--commands.allow")
            .map(|pair| pair[1].clone())
            .collect()
    };
    let snap = allowed("/usr/bin/chromium");
    assert!(snap.contains(&"/usr/bin/chromium".to_string()), "{snap:?}");
    assert!(!snap.contains(&"/usr/bin/".to_string()), "{snap:?}");
    let own = allowed("/usr/lib/chromium/chromium");
    assert!(own.contains(&"/usr/lib/chromium/".to_string()), "{own:?}");
}

/// While the last browser's sandbox has its way out paused, no other
/// starts in its place, and the call says why and how to resume.
#[test]
fn a_paused_way_out_starts_no_browser() {
    let home = link_test_kit::TempDir::new("browser-paused");
    let executable = home.join("never-run");
    link_test_kit::write_program(&executable, "#!/bin/sh\nexit 1\n").unwrap();
    let mut browser = Browser::new(Config {
        chromium: Some(executable.clone()),
        ..config(vec![executable.display().to_string()], home.join("Browser"))
    })
    .unwrap();
    let sandbox = "run-browser-paused-test";
    let dir = link_sandbox_client::wire_dir().unwrap();
    let paused = link_sandbox_client::pause_file(&dir, sandbox);
    std::fs::create_dir_all(paused.parent().unwrap()).unwrap();
    std::fs::write(&paused, "it reached for a private address\n").unwrap();
    browser.sandbox = Some(sandbox.into());
    let who = Who {
        conversation: "t".into(),
        owner: false,
    };
    let err = browser.go(&who, "https://example.org").unwrap_err();
    let said = format!("{err:#}");
    assert!(said.contains("it reached for a private address"), "{said}");
    assert!(
        said.contains(&format!("lnk sandbox start {sandbox}")),
        "{said}"
    );
    assert!(!home.join("Browser").exists(), "the browser started");
    std::fs::remove_file(&paused).unwrap();
}

/// A key is pressed as Playwright's `press` names it: a named key, one
/// character, or either after modifiers joined by `+`, each held in turn;
/// what it types is dropped while Control, Alt or Meta is held.
#[test]
fn a_key_is_pressed_as_playwright_names_it() {
    use crate::tab::pressed;
    let names = |press: &crate::tab::Press| -> Vec<String> {
        press.modifiers.iter().map(|m| m.name.clone()).collect()
    };
    let enter = pressed("Enter").unwrap();
    assert_eq!((enter.key.as_str(), enter.text.as_str()), ("Enter", "\r"));
    assert!(enter.modifiers.is_empty());
    let space = pressed("Space").unwrap();
    assert_eq!((space.key.as_str(), space.code.as_str()), (" ", "Space"));
    let a = pressed("a").unwrap();
    assert_eq!(
        (
            a.key.as_str(),
            a.code.as_str(),
            a.virtual_key,
            a.text.as_str()
        ),
        ("a", "KeyA", 65, "a")
    );
    let select_all = pressed("Control+a").unwrap();
    assert_eq!(names(&select_all), ["Control"]);
    assert_eq!(
        (select_all.code.as_str(), select_all.text.as_str()),
        ("KeyA", "")
    );
    let shifted = pressed("Shift+a").unwrap();
    assert_eq!((shifted.key.as_str(), shifted.text.as_str()), ("A", "A"));
    let back = pressed("Shift+Tab").unwrap();
    assert_eq!(
        (names(&back), back.code.as_str()),
        (vec!["Shift".to_string()], "Tab")
    );
    let both = pressed("Control+Shift+ArrowLeft").unwrap();
    assert_eq!(names(&both), ["Control", "Shift"]);
    assert_eq!(pressed("1").unwrap().code, "Digit1");
    let slash = pressed("/").unwrap();
    assert_eq!((slash.text.as_str(), slash.virtual_key), ("/", 0));
    let plus = pressed("+").unwrap();
    assert_eq!((plus.key.as_str(), plus.modifiers.len()), ("+", 0));
    assert_eq!(names(&pressed("Control++").unwrap()), ["Control"]);
    for bad in ["Hyper+a", "Enterr", "Control+", "ab", "\u{7}"] {
        assert!(pressed(bad).is_err(), "{bad}");
    }
    let refused = format!("{:#}", pressed("Super+a").unwrap_err());
    assert!(refused.contains("Control"), "{refused}");
}
