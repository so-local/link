use super::*;
use link_spec::{follow::References, history::History};

/// A release whose core renamed `memory-tool` to `notes-tool`, and whose
/// `notes-tool` renamed its call `search` to `find` and removed `links`.
fn renamed() -> References {
    let core = History::parse(
        "[[renamed]]\nmodule = 'memory-tool'\nnow = 'notes-tool'\nsince = '1.1.0'\nuntil = '2.0.0'\n",
    )
    .unwrap();
    let notes = History::parse(
        "[[gone]]\nwas = 'notes-tool search'\nnow = 'lnk notes-tool find'\nsince = '1.1.0'\nuntil = '2.0.0'\n\
         [[gone]]\nwas = 'notes-tool links'\nwhy = 'Read the page.'\nsince = '1.1.0'\nuntil = '2.0.0'\n",
    )
    .unwrap();
    References::from(core.release_wire([("notes-tool", &notes)]))
}

fn unread(_: &Part) -> anyhow::Result<Vec<u8>> {
    anyhow::bail!("not read")
}

fn calling(name: &str) -> Vec<Event> {
    vec![
        Event {
            role: event::USER.into(),
            text: "Where were we?".into(),
            ..Default::default()
        },
        Event {
            role: event::ASSISTANT.into(),
            calls: vec![crate::event::Call {
                id: "1".into(),
                name: name.into(),
                arguments: serde_json::json!({}),
            }],
            ..Default::default()
        },
        Event {
            role: event::TOOL.into(),
            call: Some("1".into()),
            tool: Some(name.into()),
            text: "found".into(),
            ..Default::default()
        },
    ]
}

/// A recorded call is replayed under the name its tool and command have
/// today, its record left as it was; one since removed keeps its name.
#[test]
fn a_replayed_call_is_named_as_it_is_today_and_its_record_is_kept() {
    for (recorded, replayed) in [
        ("memory__search", "notes__find"),
        ("memory__links", "memory__links"),
        ("gh__list_issues", "gh__list_issues"),
    ] {
        let events = calling(recorded);
        let projection = projected_by(
            &events,
            true,
            &unread,
            &|name| renamed().call(name),
            &mut ContentDigest::of,
        );
        let names: Vec<_> = projection
            .messages
            .iter()
            .flat_map(|m| &m.calls)
            .map(|call| call.name.as_str())
            .collect();
        assert_eq!(names, [replayed], "{recorded}");
        assert_eq!(events[1].calls[0].name, recorded);
        // Without the provider's blocks, the call and its answer are
        // written into the text by the same names.
        let text: String = projected_by(
            &events,
            false,
            &unread,
            &|name| renamed().call(name),
            &mut ContentDigest::of,
        )
        .messages
        .iter()
        .map(|m| m.text.clone())
        .collect();
        assert!(text.contains(&format!("[called {replayed} ")), "{text}");
        assert!(text.contains(&format!("({replayed} answered")), "{text}");
    }
}
