//! One agent served over stdio as its machine's bridges reach it
//! (`lnk-memory-tool bridge`): bound as a shared engine binds a session,
//! but in a process of its own, run as that agent's own user, so no other
//! agent's memory is ever in it. Its `memory/` is listed at its start and
//! once a minute. Its index is a cache of the copy beside its Home,
//! fetched at its start and written back once a minute while it changes
//! and when its input closes; or, once, made current and written
//! back with nothing served ([`One::once`]).

use super::*;

/// What `bridge` serves.
pub struct One {
    /// The agent, by its ID.
    pub agent: String,
    /// How its bridges reach it, with its key.
    pub bound: Bound,
    /// A team's memory, served instead of the agent's own.
    pub team: Option<TeamSession>,
    /// The folder its index is fetched into.
    pub cache: PathBuf,
    /// List what changed, write the index back, remove the cache and end,
    /// serving nothing.
    pub once: bool,
}

/// Serves `one` on `input` and `output` until `input` ends, then writes
/// its index back; with [`One::once`], only writes it back, current.
pub fn serve_one(one: One, input: impl BufRead, output: impl Write) -> Result<()> {
    let One {
        agent,
        bound,
        team,
        cache,
        once,
    } = one;
    let owner = bound.owner.clone();
    let table = Arc::new(Table {
        agents: Agents(Default::default()),
        config: Config {
            cache: Some(cache.clone()),
            files_bridge: bound.files.clone(),
            storage_bridge: bound.storage.as_ref().and_then(|s| s.socket.clone()),
            write_every: WRITE_EVERY,
            ..Config::default()
        },
        opened: Mutex::default(),
        bound: Mutex::default(),
        binding: Mutex::default(),
        pool: crate::serve::Pool::new(),
    });
    link_files::create_private_dir(&cache)?;
    let slot = table.bind(&agent, bound, team)?;
    // Its `memory/` listed again: what was pushed while it slept.
    slot.common().changed();
    slot.writer.wake();
    let ended = match once {
        true => Ok(()),
        false => {
            ticking(&table);
            relisting(&slot);
            crate::serve::session(slot.as_ref(), input, output)
        }
    };
    // Written back, current, whatever ended the session.
    let flushed = table.control(Control::Flush {
        owner,
        agent: agent.clone(),
    });
    if once {
        match std::fs::remove_dir_all(&cache) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                return Err(err).with_context(|| format!("removing {}", cache.display()));
            }
            _ => {}
        }
    }
    ended.and(flushed)
}

/// Its `memory/` listed again once a minute while it's served: no other
/// process tells it of a change, so what the agent's file tool wrote
/// there is found within a minute.
#[expect(clippy::disallowed_methods, reason = "the service's own period")]
fn relisting(slot: &BoundSlot) {
    let held = Arc::downgrade(slot);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(WRITE_EVERY);
            match held.upgrade() {
                Some(slot) => {
                    slot.common().changed();
                    slot.writer.wake();
                }
                None => return,
            }
        }
    });
}
