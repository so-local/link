//! Telling the agent how a command ended, and the sweeps that find one
//! whose supervisor is gone.

use super::*;

/// Tells the agent how `record` ended, through its bridge's way in, once:
/// marked told before it's handed over, unmarked only when the way in
/// can't be reached. The caller holds its lock.
pub fn tell(state: &Path, record: &mut Record) -> anyhow::Result<()> {
    if record.told || record.state.running() {
        return Ok(());
    }
    let firing = link_connection_contract::Firing {
        parent: None,
        delivery: None,
        occurrence: None,
        model: None,
        thread: record.thread.clone(),
        from: format!("run:{}", record.id),
        text: record.ending(),
        answer: link_connection_contract::AnswerTo::Chat,
        hops: record.hops.saturating_add(1),
        tools: None,
        // As its sender stands: a background command records no
        // conversation's standing of its own.
        standing: link_connection_contract::access::Standing::of(Some(&format!(
            "run:{}",
            record.id
        ))),
    };
    let socket = match std::os::unix::net::UnixStream::connect(&record.way_in) {
        Ok(socket) => socket,
        Err(err) => {
            return Err(err).with_context(|| {
                format!("reaching the agent's bridge at {}", record.way_in.display())
            });
        }
    };
    let binding = record.caller.binding()?;
    record.told = true;
    save(state, record)?;
    let sent = link_sandbox_binding::call(&socket, &binding, &firing)
        .and_then(|()| Ok(socket.shutdown(std::net::Shutdown::Write)?));
    if let Err(err) = sent {
        // Never taken in: told again later.
        record.told = false;
        save(state, record)?;
        return Err(err).context("handing the ending to the agent's bridge");
    }
    // The bridge answers once the agent did; nobody waits for that.
    Ok(())
}

/// What a sweep did.
#[derive(Debug, Default, PartialEq)]
pub struct Swept {
    /// Commands found without their supervisor, now lost.
    pub lost: Vec<String>,
    /// Commands ended and not yet told.
    pub untold: Vec<String>,
    /// Commands removed, past their keep.
    pub removed: Vec<String>,
}

/// Looks at every record in `state`: a command whose supervisor is gone
/// is marked lost, what's left of it killed; one ended and told past its
/// keep is removed with its output. Tells nobody.
pub fn sweep_quietly(state: &Path) -> Swept {
    let ids: Vec<String> = records(state).into_iter().map(|r| r.id).collect();
    sweep_ids(state, &ids)
}

/// [`sweep_quietly`] of the records `ids` names, each read again once its
/// lock is held: a record read before could be one its supervisor has
/// since ended and told, which a sweep would mark lost over and tell
/// again.
fn sweep_ids(state: &Path, ids: &[String]) -> Swept {
    let mut swept = Swept::default();
    for id in ids {
        let Some(lock) = take_lock(state, id) else {
            // Its supervisor holds it, or another sweep: alive.
            continue;
        };
        let Ok(mut record) = load(state, id) else {
            // Removed meanwhile.
            continue;
        };
        if record.state.running() {
            kill_left(&record);
            record.state = State::Lost;
            record.why = Some("its supervisor ended: the agent or the machine stopped".into());
            record.ended = Some(now());
            if save(state, &record).is_ok() {
                swept.lost.push(record.id.clone());
            }
        }
        if !record.told {
            swept.untold.push(record.id.clone());
        } else if record
            .ended
            .is_some_and(|ended| now().saturating_sub(ended) >= record.keep_secs)
        {
            remove_output(&record.files, &record.id);
            let _ = std::fs::remove_file(record_path(state, &record.id));
            let _ = std::fs::remove_file(stop_path(state, &record.id));
            let _ = std::fs::remove_file(lock_path(state, &record.id));
            swept.removed.push(record.id.clone());
        }
        drop(lock);
    }
    swept
}

/// [`sweep_quietly`], then tells the agent of each command ended and
/// untold, trying again while its bridge can't be reached, for a while.
#[expect(clippy::disallowed_methods, reason = "the sweep's own period")]
pub fn sweep(state: &Path) -> Swept {
    let mut swept = sweep_quietly(state);
    let mut wait = Duration::from_secs(2);
    let until = Instant::now() + link_run_tool_contract::limits().tell_for();
    loop {
        let mut left = Vec::new();
        for id in &swept.untold {
            let Some(lock) = take_lock(state, id) else {
                continue;
            };
            if let Ok(mut record) = load(state, id)
                && tell(state, &mut record).is_err()
            {
                left.push(id.clone());
            }
            drop(lock);
        }
        if left.is_empty() || Instant::now() >= until {
            swept.untold = left;
            return swept;
        }
        std::thread::sleep(wait);
        wait = (wait * 2).min(Duration::from_secs(60));
    }
}

/// Whether a sweep would have someone to tell: a command ended and
/// untold, its lock free.
pub fn has_untold(state: &Path) -> bool {
    records(state)
        .iter()
        .any(|r| !r.told && !r.state.running() && take_lock(state, &r.id).is_some())
}

/// Stops every background command in `state` and removes them all, their
/// output with them: for an agent deleted, or moving away. Each running
/// or not yet told, in words.
pub fn forget(state: &Path) -> Vec<String> {
    let said = stop_all(state, "the agent moved away or was deleted");
    for record in records(state) {
        remove_output(&record.files, &record.id);
        let _ = std::fs::remove_dir(record.files.join(FOLDER));
    }
    let _ = std::fs::remove_dir_all(state);
    said
}

/// Stops every background command in `state` still running, each
/// recorded lost for `why` and told when the agent's back (by a sweep);
/// each running, or ended and not yet told, in words.
pub fn stop_all(state: &Path, why: &str) -> Vec<String> {
    let mut said = Vec::new();
    for record in records(state) {
        let record = match record.state.running() {
            true => stop_for(state, &record.id, why).unwrap_or_else(|_| {
                // Its supervisor didn't answer: what's left is killed.
                kill_left(&record);
                record
            }),
            false => record,
        };
        if !record.told {
            said.push(format!(
                "{} ({}): {}",
                record.id,
                record.line(),
                record.how()
            ));
        }
    }
    said
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A record its supervisor ended and told after a sweep listed it, and
    /// before the sweep took its lock, is read again: never marked lost,
    /// nor told twice.
    #[test]
    fn a_sweep_reads_a_record_again_once_it_holds_its_lock() {
        let dir = link_test_kit::TempDir::new("run-sweep-again");
        let state = dir.join("state");
        std::fs::create_dir_all(&state).unwrap();
        let files = dir.to_path_buf();
        let id = link_id::candidate().unwrap();
        let record: Record = serde_json::from_value(json!({
            "id": id, "command": "sh", "argv": ["sh"], "path": "",
            "files": files, "thread": "t", "hops": 0, "way_in": files, "timeout_ms": 0,
            "output_max": 8, "tail": 8, "keep_secs": 3600, "started": 0, "state": "running"}))
        .unwrap();
        save(&state, &record).unwrap();
        // Listed while it runs.
        let ids: Vec<String> = records(&state).into_iter().map(|r| r.id).collect();
        // Then its supervisor ends it and tells the agent.
        let mut ended = record.clone();
        ended.state = State::Exited;
        ended.exit = Some(0);
        ended.ended = Some(now());
        ended.told = true;
        save(&state, &ended).unwrap();
        let swept = sweep_ids(&state, &ids);
        assert_eq!(swept, Swept::default());
        let kept = load(&state, &id).unwrap();
        assert_eq!((kept.state, kept.told), (State::Exited, true));
    }
}
