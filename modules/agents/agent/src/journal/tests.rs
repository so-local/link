use super::*;

fn plan(agent: &str) -> Journal {
    let rel = |p: &str| Relative::new(p).unwrap();
    Journal::plan(
        agent,
        "openclaw",
        vec![
            Step::Agent,
            Step::Copy {
                from: rel(".openclaw"),
                to: rel(".openclaw"),
                leaving: vec![rel(".openclaw/agents/work")],
            },
            Step::Secret {
                name: "TELEGRAM_BOT_TOKEN".into(),
            },
            Step::Service {
                at: rel(".config/systemd/user/openclaw-gateway.service"),
            },
        ],
    )
}

/// Stopped at any step, the journal holds the plan with the steps done,
/// a run again does only the rest, and clearing undoes what was done,
/// last first, and nothing else.
#[test]
fn a_migration_stopped_at_any_step_resumes_there_or_clears_what_it_did() {
    let _own = crate::settings::tests::own_config();
    for stop in 0..4 {
        let agent = format!("journaled{stop}");
        let mut journal = plan(&agent);
        let mut ran = Vec::new();
        let stopped = journal.run(|step| {
            anyhow::ensure!(ran.len() < stop, "stopped");
            ran.push(step.clone());
            Ok(())
        });
        assert!(stopped.is_err());
        let kept = Journal::load(&agent).unwrap().unwrap();
        assert_eq!(kept.entries.iter().filter(|e| e.done).count(), stop);
        assert!(!kept.finished());
        // Resumed: only what wasn't done.
        let mut resumed = kept.clone();
        let mut again = Vec::new();
        resumed
            .run(|step| {
                again.push(step.clone());
                Ok(())
            })
            .unwrap();
        assert_eq!(again.len(), 4 - stop);
        assert_eq!(again.first(), Some(&kept.entries[stop].step));
        assert!(Journal::load(&agent).unwrap().is_none());
        // Cleared instead: what was done, last first.
        let mut cleared = kept.clone();
        cleared.save().unwrap();
        let mut undone = Vec::new();
        cleared
            .clear(|step| {
                undone.push(step.clone());
                Ok(())
            })
            .unwrap();
        let mut done: Vec<Step> = ran.clone();
        done.reverse();
        assert_eq!(undone, done);
        assert!(Journal::load(&agent).unwrap().is_none());
    }
}

#[test]
fn a_journal_stores_no_absolute_path() {
    let _own = crate::settings::tests::own_config();
    let journal = plan("written");
    journal.save().unwrap();
    let text = std::fs::read_to_string(Journal::path_of("written").unwrap()).unwrap();
    assert!(
        !text.contains(&*link_files::home_dir().to_string_lossy()),
        "{text}"
    );
    assert!(text.contains("from = \".openclaw\""), "{text}");
    let bad = text.replace("from = \".openclaw\"", "from = \"/home/me/.openclaw\"");
    assert!(toml::from_str::<Journal>(&bad).is_err());
}
