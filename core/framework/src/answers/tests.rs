use super::*;
use crate::modules::Listed;

/// A question whose protocol is at 2, accepting nothing older.
const ASKED: Question<String, Vec<u32>> = Question::new(
    "store.folders",
    "name = \"store folders answers\"\ncurrent = 2\noldest = 2\nupgrade = \"lnk upgrade\"\n",
);

#[test]
fn only_installed_modules_that_answer_are_asked() {
    let module = |name: &str, installed: bool, answers: &[&str]| Listed {
        name: name.into(),
        installed,
        answers: answers.iter().map(|q| q.to_string()).collect(),
        ..Default::default()
    };
    let listed = vec![
        module("agent", true, &["store.folders"]),
        module("other", false, &["store.folders"]),
        module("quiet", true, &[]),
        module("../escape", true, &["store.folders"]),
    ];
    let answerers = ASKED.answerers_in(&listed);
    assert_eq!(
        answerers,
        [Answerer {
            module: "agent".into(),
            question: "store.folders".into(),
        }]
    );
    let command = answerers[0].command();
    assert!(
        command
            .get_program()
            .to_string_lossy()
            .ends_with("lnk-agent")
    );
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        ["--link-answer", "store.folders"]
    );
    // It hands its sources, none here, for the answerer to compare.
    let handed = command
        .get_envs()
        .find(|(name, _)| *name == crate::source::ASKER)
        .and_then(|(_, value)| value.map(|v| v.to_os_string()));
    assert_eq!(handed, crate::source::this().map(Into::into));
}

/// A question and its answer cross the wire in their envelopes, each
/// stating the version its side speaks.
#[test]
fn a_question_and_its_answer_round_trip_in_their_envelopes() {
    let input = "main".to_string();
    let line = serde_json::to_string(&ASKED.asked(&input)).unwrap();
    assert_eq!(line, r#"{"protocol":2,"question":"main"}"#);
    let asked: Asked<String> = serde_json::from_str(&line).unwrap();
    assert_eq!(ASKED.question_of(asked).unwrap(), "main");

    let line = serde_json::to_string(&ASKED.answered(&vec![1, 2])).unwrap();
    assert_eq!(line, r#"{"protocol":2,"answer":[1,2]}"#);
    let answered: Answered<Vec<u32>> = serde_json::from_str(&line).unwrap();
    assert_eq!(ASKED.answer_of("agent", answered).unwrap(), [1, 2]);

    // A bare question or answer, without its envelope, isn't one.
    assert!(serde_json::from_str::<Asked<String>>(r#""main""#).is_err());
    assert!(serde_json::from_str::<Answered<Vec<u32>>>("[1,2]").is_err());
    assert!(
        serde_json::from_str::<Asked<String>>(r#"{"protocol":2,"question":"a","more":1}"#).is_err()
    );
}

/// Each side refuses a peer below the oldest version it accepts, naming
/// the peer, the protocol and the upgrade.
#[test]
fn a_peer_below_the_oldest_is_refused_by_name_on_each_side() {
    let refused = ASKED
        .question_of(Asked {
            protocol: 1,
            question: "main".into(),
        })
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("the module asking store.folders")
            && refused.contains("store folders answers protocol 1")
            && refused.contains("lnk upgrade"),
        "{refused}"
    );
    let refused = ASKED
        .answer_of(
            "agent",
            Answered {
                protocol: 1,
                answer: vec![],
            },
        )
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("the agent module")
            && refused.contains("store folders answers protocol 1"),
        "{refused}"
    );
    // A newer peer is the newer side's to judge.
    assert!(
        ASKED
            .question_of(Asked {
                protocol: 3,
                question: "main".into(),
            })
            .is_ok()
    );
}

#[test]
fn the_cores_question_declares_its_protocol() {
    let protocol = crate::owner::ACCOUNT.protocol();
    assert_eq!((protocol.current, protocol.oldest), (1, 1));
}

/// A failed answerer's error ends with what it last said on stderr,
/// cleaned and bounded, so whoever reads it learns why.
#[cfg(unix)]
#[test]
fn a_failed_answer_carries_what_the_answerer_said() {
    use std::os::unix::process::ExitStatusExt;
    let answerer = Answerer {
        module: "agent".into(),
        question: ASKED.name.into(),
    };
    let output = |code: i32, stderr: &str| std::process::Output {
        status: std::process::ExitStatus::from_raw(code << 8),
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    };
    let failed = |stderr: &str| {
        answerer
            .answer_in(&ASKED, &output(1, stderr))
            .unwrap_err()
            .to_string()
    };
    assert_eq!(
        failed("starting\n\nError: no such file: settings.toml\n\n"),
        "the agent module didn't answer store.folders: \
         starting / Error: no such file: settings.toml"
    );
    // Said nothing: the error says only that it didn't answer.
    assert_eq!(
        failed(" \n"),
        "the agent module didn't answer store.folders"
    );
    // Only the last lines, without control or invisible characters.
    let said = Said::shipped();
    let many: String = (1..=said.lines + 2)
        .map(|n| format!("line {n}\n"))
        .collect();
    let error = failed(&format!("{many}\u{1b}[31mred\u{202E}\n"));
    assert!(
        !error.contains(&format!("line {}", said.lines)) && !error.contains('\u{1b}'),
        "{error}"
    );
    assert!(error.ends_with("[31mred"), "{error}");
    // Only the last characters.
    let long = "x".repeat(said.chars * 2);
    let error = failed(&format!("{long}end"));
    assert!(error.ends_with("end"), "{error}");
    assert!(error.chars().count() < said.chars + 80, "{error}");
    // One that answered is no error, whatever it said.
    assert!(answerer.answer_in(&ASKED, &output(0, "warning")).is_ok());
}

/// Of a program's stderr, only the first bytes `said.toml` allows are
/// read, and the rest is drained, so a program saying much neither fills
/// memory nor waits on its pipe.
#[test]
fn only_the_first_bytes_of_stderr_are_read() {
    let said = Said::shipped();
    let long = vec![b'x'; usize::try_from(said.bytes * 2).unwrap()];
    let mut pipe = std::io::Cursor::new(long);
    assert_eq!(said.read(&mut pipe).len() as u64, said.bytes);
    assert_eq!(pipe.position(), said.bytes * 2);
}
