//! The generated command line read as the agent's commands, and what they
//! share once read.

use super::*;
use clap::Parser;

/// `line` read as `lnk-agent`'s command line, which never holds its
/// namespace.
fn parsed(line: &[&str]) -> Command {
    Cli::try_parse_from(std::iter::once("lnk-agent").chain(line.iter().copied()))
        .unwrap()
        .command
}

fn refused(line: &[&str]) -> bool {
    Cli::try_parse_from(std::iter::once("lnk-agent").chain(line.iter().copied())).is_err()
}

/// Every command takes the agent it acts on, by its ID: a name is the
/// program's lookup's to turn into one (`naming.rs`), and with none set,
/// as here, it's refused.
#[test]
fn every_command_takes_the_agent_it_acts_on() {
    let id = "agent:k7m2p9q4r8s3";
    let command = parsed(&["restart", "--agent", id]);
    assert!(matches!(command, Command::AgentRestart(_)));
    assert_eq!(command.agent().map(AgentId::as_str), Some(id));
    assert_eq!(
        parsed(&["-a", id, "chat", "list"])
            .agent()
            .map(AgentId::as_str),
        Some(id)
    );
    assert_eq!(parsed(&["start"]).agent(), None);
    assert!(refused(&["restart", "--foreground"]));
    assert!(refused(&["restart", "--agent", "other"]));
}

#[test]
fn a_start_given_nothing_starts_as_ever() {
    let Command::AgentStart(start) = parsed(&["start"]) else {
        panic!("agent start")
    };
    assert_eq!(
        Starting::start(&start, &link_spec::FieldFlags::default()),
        Starting::default()
    );
    let Command::AgentStart(again) = parsed(&["start", "--sandbox"]) else {
        panic!("agent start")
    };
    let again = Starting::start(&again, &link_spec::FieldFlags::default());
    assert_eq!(again.sandbox, Some(PathBuf::new()));
    assert_eq!(again.sandbox_file(), None);
    let Command::AgentStart(file) = parsed(&["start", "--sandbox", "run.toml"]) else {
        panic!("agent start")
    };
    let file = Starting::start(&file, &link_spec::FieldFlags::default());
    assert_eq!(file.sandbox_file(), Some(std::path::Path::new("run.toml")));
    assert!(refused(&["start", "--sandbox", "run.toml", "--no-sandbox"]));
}

#[test]
fn live_chat_and_one_message_have_distinct_commands() {
    let live = Chat::of(&parsed(&["chat", "start"])).unwrap();
    assert!(live.live);
    assert_eq!(live.args(), ["chat", "start"]);
    assert_ne!(live.thread().unwrap(), live.thread().unwrap());
    // Bare, `chat` is `chat start`.
    assert!(Chat::of(&parsed(&["chat"])).unwrap().live);
    let one = Chat::of(&parsed(&["chat", "send", "--message", "hi"])).unwrap();
    assert!(!one.live);
    assert_eq!(one.message().as_deref(), Some("hi"));
    assert_eq!(one.args(), ["chat", "send"]);
    assert_eq!(one.thread().unwrap(), "link:main");
    let selected = Chat::of(&parsed(&["chat", "start", "--chat", "work"])).unwrap();
    assert_eq!(selected.thread().unwrap(), "link:work");
    // Only a live chat continuing a named one needs it to exist: a
    // message in a new name starts that conversation.
    assert!(selected.existing);
    assert!(!live.existing);
    let named = Chat::of(&parsed(&["chat", "send", "--chat", "work", "-m", "hi"])).unwrap();
    assert!(!named.existing);
    // A live chat may open with its first message, which goes to the
    // agent where it runs as an argument: the terminal stays its stdin.
    let seeded = Chat::of(&parsed(&["chat", "-m", "What can you do?"])).unwrap();
    assert!(seeded.live);
    assert_eq!(seeded.message().as_deref(), Some("What can you do?"));
    assert!(!seeded.piped());
    assert_eq!(
        seeded.args(),
        ["chat", "start", "--message", "What can you do?"]
    );
    assert!(one.piped());
    assert!(!live.piped());
    assert!(refused(&[
        "chat",
        "send",
        "--message",
        "hi",
        "--action",
        "x"
    ]));
    assert!(refused(&["chat", "send", "--json", "--stream-json"]));
}

#[test]
fn a_harness_chat_command_is_sent_as_its_message() {
    let ask = Chat::of(&parsed(&["chat", "send", "--aside", "--message", "why?"])).unwrap();
    assert_eq!(ask.message().as_deref(), Some("/chat:ask why?"));
    let queued = Chat::of(&parsed(&["chat", "send", "--queue", "-m", "next"])).unwrap();
    assert_eq!(queued.message().as_deref(), Some("/chat:queue next"));
    let steered = Chat::of(&parsed(&["chat", "send", "--steer", "-m", "now"])).unwrap();
    assert_eq!(steered.message().as_deref(), Some("/chat:steer now"));
    assert!(refused(&["chat", "send", "--aside", "--queue", "-m", "x"]));
    assert!(refused(&["chat", "send", "--steer"]));
    let status = Chat::of(&parsed(&["chat", "show", "link:main", "--status"])).unwrap();
    assert_eq!(status.message().as_deref(), Some("/chat:status"));
    assert!(status.existing, "a status is of a chat the agent has");
    assert!(Chat::of(&parsed(&["chat", "show", "link:main"])).is_none());
    assert!(Chat::of(&parsed(&["chat", "export", "--format", "compact"])).is_none());
    let clear = Chat::of(&parsed(&["chat", "set", "goal", "--clear"])).unwrap();
    assert_eq!(clear.message().as_deref(), Some("/chat:goal --clear"));
    let model = Chat::of(&parsed(&["model", "set", "fast", "--chat", "work"])).unwrap();
    assert_eq!(model.message().as_deref(), Some("/model:set fast"));
    assert!(Chat::of(&parsed(&["model", "set", "fast"])).is_none());
}

/// `chat set` names the value it sets, and sends its harness's command.
#[test]
fn chat_set_sends_the_harness_command_for_its_value() {
    let named = Chat::of(&parsed(&["chat", "set", "name", "Plans"])).unwrap();
    assert_eq!(named.message().as_deref(), Some("/chat:rename Plans"));
    let verbose = Chat::of(&parsed(&["chat", "set", "verbose", "on", "--chat", "work"])).unwrap();
    assert_eq!(verbose.message().as_deref(), Some("/chat:verbose on"));
    assert_eq!(verbose.thread().unwrap(), "link:work");
}

/// `chat set goal` gives the chat a goal and `--clear` takes it away;
/// `chat get goal` says it.
#[test]
fn a_goal_is_set_cleared_and_got() {
    let set = Chat::of(&parsed(&["chat", "set", "goal", "the tests pass"])).unwrap();
    assert_eq!(set.message().as_deref(), Some("/chat:goal the tests pass"));
    let got = Chat::of(&parsed(&["chat", "get", "goal", "--chat", "work"])).unwrap();
    assert_eq!(got.message().as_deref(), Some("/chat:goal"));
    assert_eq!(got.thread().unwrap(), "link:work");
    assert!(refused(&["chat", "set", "goal", "x", "--clear"]));
}

/// `chat get` refuses a value it doesn't name, and one left out.
#[test]
fn chat_get_refuses_a_value_it_doesnt_name() {
    assert!(refused(&["chat", "get", "name"]));
    assert!(refused(&["chat", "get"]));
}

/// `chat set` refuses a value it doesn't name, one without its value, and
/// clearing what only a goal clears.
#[test]
fn chat_set_refuses_a_value_it_doesnt_name() {
    assert!(refused(&["chat", "set", "colour", "on"]));
    assert!(refused(&["chat", "set"]));
    let Command::ChatSet(named) = parsed(&["chat", "set", "name"]) else {
        panic!("chat set")
    };
    assert!(named.check().is_err());
    let Command::ChatSet(cleared) = parsed(&["chat", "set", "name", "--clear"]) else {
        panic!("chat set")
    };
    assert!(cleared.check().is_err());
    let Command::ChatSet(goal) = parsed(&["chat", "set", "goal", "--clear"]) else {
        panic!("chat set")
    };
    assert!(goal.check().is_ok());
}

/// A chat command a harness answers that `chat start`, `chat send` or
/// `chat set` already says is no command of its own: it's typed in a chat.
#[test]
fn chat_commands_another_says_are_typed_in_a_chat_only() {
    // `chat stop` stays a command: it stops an answer from another terminal.
    let stop = Chat::of(&parsed(&["chat", "stop", "--chat", "work"])).unwrap();
    assert_eq!(stop.message().as_deref(), Some("/chat:stop"));
    assert_eq!(stop.thread().unwrap(), "link:work");
    for verb in ["create", "resume", "goal"] {
        assert!(refused(&["chat", verb]), "chat {verb}");
    }
}

#[test]
fn a_team_chat_selects_the_lead_or_an_explicit_member() {
    for member in [None, Some("writer")] {
        let mut line = vec![
            "chat",
            "send",
            "--team",
            "research",
            "--message",
            "Plan the launch",
        ];
        if let Some(member) = member {
            line.extend(["--member", member]);
        }
        let command = parsed(&line);
        assert!(Chat::of(&command).is_none());
        let Command::ChatSend(chat) = command else {
            panic!("chat send --team")
        };
        assert_eq!(chat.team.as_deref(), Some("research"));
        assert_eq!(chat.member.as_deref(), member);
        assert_eq!(
            Chat::team(&chat).message().as_deref(),
            Some("Plan the launch")
        );
    }
    assert!(refused(&["chat", "send", "--member", "writer", "-m", "x"]));
    assert!(refused(&[
        "team",
        "create",
        "--spec",
        "team.toml",
        "--lead",
        "x"
    ]));
}

#[test]
fn send_says_to_whom_and_where() {
    let line = [
        "chat",
        "send",
        "-m",
        "hi",
        "--to",
        "owner",
        "--connection",
        "slack",
    ];
    let Command::ChatSend(owner) = parsed(&line) else {
        panic!("chat send --to")
    };
    assert_eq!(send_to(&owner).unwrap(), "owner");
    assert_eq!(send_thread(&owner).unwrap(), "link:main");
    let line = ["chat", "send", "-m", "hi", "--to", "owner", "--chat", "q"];
    let Command::ChatSend(named) = parsed(&line) else {
        panic!("chat send --to owner --chat")
    };
    assert_eq!(send_thread(&named).unwrap(), "link:q");
    let line = ["chat", "send", "-m", "hi", "--to", "owner", "--chat", "x:1"];
    let Command::ChatSend(unnamed) = parsed(&line) else {
        panic!("chat send --to owner --chat")
    };
    assert!(send_thread(&unnamed).is_err());
    let line = [
        "chat",
        "send",
        "-m",
        "hi",
        "--to",
        "teammate:scout",
        "--chat",
        "c",
    ];
    let Command::ChatSend(teammate) = parsed(&line) else {
        panic!("chat send --to")
    };
    assert_eq!(send_to(&teammate).unwrap(), "scout");
    assert_eq!(send_thread(&teammate).unwrap(), "c");
    assert!(refused(&["chat", "send", "-m", "hi", "--to", "scout"]));
    assert!(refused(&[
        "chat",
        "send",
        "-m",
        "hi",
        "--connection",
        "slack"
    ]));
    assert!(refused(&[
        "chat",
        "send",
        "-m",
        "hi",
        "--to",
        "owner",
        "--chat",
        "c",
        "--connection",
        "slack"
    ]));
}

#[test]
fn a_route_is_one_of_its_four() {
    let route = |args: &[&str]| {
        let mut all = vec!["route"];
        all.extend(args);
        let Command::AgentRoute(given) = parsed(&all) else {
            panic!("agent route")
        };
        upstream(&given)
    };
    assert_eq!(route(&["--direct"]).unwrap(), "direct");
    assert_eq!(route(&["--none"]).unwrap(), "none");
    assert_eq!(
        route(&["--http", "127.0.0.1:3291"]).unwrap(),
        "http://127.0.0.1:3291"
    );
    assert_eq!(
        route(&["--socks", "socks5://127.0.0.1:1080"]).unwrap(),
        "socks5://127.0.0.1:1080"
    );
    assert!(route(&["--direct", "--none"]).is_err());
    // Each flag takes only its own kind, and an HTTP proxy only here.
    assert!(route(&["--socks", "http://127.0.0.1:3291"]).is_err());
    assert!(route(&["--http", "socks5://127.0.0.1:1080"]).is_err());
    assert!(route(&["--http", "10.0.0.1:3128"]).is_err());
}
