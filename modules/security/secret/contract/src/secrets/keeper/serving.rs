//! The keeper serving its socket: each connection one call, admitted per
//! user, answered on the store of the user at its other end.

use super::*;

/// Serves the keeper on `listener` for ever: each connection one call,
/// answered on the store of the user at its other end, under `root`
/// (`<root>/<uid>/secrets.toml`, each folder only the keeper's).
pub fn serve(listener: UnixListener, root: &Path) -> anyhow::Result<()> {
    serve_with_routes(listener, root, std::sync::Arc::new(Routes::default()))
}

#[derive(Default)]
struct Management {
    counts: std::sync::Mutex<Counts>,
    /// Signalled whenever a call ends, for the calls waiting.
    ended: std::sync::Condvar,
}

/// Each user's calls being answered, and waiting to be.
#[derive(Default)]
struct Counts {
    answering: std::collections::BTreeMap<u32, usize>,
    waiting: std::collections::BTreeMap<u32, usize>,
}

impl Counts {
    /// Whether a call of `uid` may be answered now.
    fn room(&self, uid: u32) -> bool {
        let others: usize = self
            .answering
            .iter()
            .filter(|(peer, _)| **peer != 0)
            .map(|(_, n)| n)
            .sum();
        uid == 0 || (count(&self.answering, uid) < PER_USER && others < AT_ONCE)
    }
}

fn count(counts: &std::collections::BTreeMap<u32, usize>, uid: u32) -> usize {
    counts.get(&uid).copied().unwrap_or(0)
}

fn take(counts: &mut std::collections::BTreeMap<u32, usize>, uid: u32) {
    if let Some(n) = counts.get_mut(&uid) {
        *n -= 1;
        if *n == 0 {
            counts.remove(&uid);
        }
    }
}

struct Admission {
    management: std::sync::Arc<Management>,
    uid: u32,
}

/// What a call is let do on arrival.
enum Arrival {
    /// Be answered now.
    Answered(Admission),
    /// Wait, in its own thread, for one of its user's calls to end.
    Waiting(u32),
    /// Be closed: its user has too many waiting already.
    Busy,
}

impl Management {
    /// A call of `uid`, admitted now, or set to wait, or refused.
    fn arrive(self: &std::sync::Arc<Self>, uid: u32) -> Arrival {
        let mut counts = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        if counts.room(uid) {
            *counts.answering.entry(uid).or_default() += 1;
            return Arrival::Answered(self.admitted(uid));
        }
        if count(&counts.waiting, uid) >= WAITING_PER_USER {
            return Arrival::Busy;
        }
        *counts.waiting.entry(uid).or_default() += 1;
        Arrival::Waiting(uid)
    }

    /// The waiting call of `uid`, admitted when there's room, or `None`
    /// when there was none within `wait`.
    fn wait(self: &std::sync::Arc<Self>, uid: u32, wait: Duration) -> Option<Admission> {
        let deadline = std::time::Instant::now() + wait;
        let mut counts = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        let admitted = loop {
            if counts.room(uid) {
                break true;
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                break false;
            }
            counts = self
                .ended
                .wait_timeout(counts, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        };
        take(&mut counts.waiting, uid);
        admitted.then(|| {
            *counts.answering.entry(uid).or_default() += 1;
            self.admitted(uid)
        })
    }

    fn admitted(self: &std::sync::Arc<Self>, uid: u32) -> Admission {
        Admission {
            management: self.clone(),
            uid,
        }
    }
}

impl Drop for Admission {
    fn drop(&mut self) {
        let mut counts = self
            .management
            .counts
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        take(&mut counts.answering, self.uid);
        self.management.ended.notify_all();
    }
}

/// The keeper's management calls and its secret bridge share this state.
pub fn serve_with_routes(
    listener: UnixListener,
    root: &Path,
    routes: std::sync::Arc<Routes>,
) -> anyhow::Result<()> {
    // One working UID cannot starve another's calls: past its share, its
    // calls wait for its own to end. Root has reserved management
    // capacity so a helper cannot prevent its revocation. A call waits
    // rather than being closed unanswered; only past its user's waiting
    // calls, which bound the threads one user holds, is it closed, and its
    // caller is told the keeper was busy.
    let management = std::sync::Arc::new(Management::default());
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            continue;
        };
        let Ok(uid) = peer_uid(&stream) else {
            continue;
        };
        let arrival = management.arrive(uid);
        if let Arrival::Busy = arrival {
            continue;
        }
        let (root, routes, management) = (root.to_path_buf(), routes.clone(), management.clone());
        std::thread::spawn(move || {
            let admission = match arrival {
                Arrival::Answered(admission) => Some(admission),
                Arrival::Waiting(uid) => management.wait(uid, QUEUED),
                Arrival::Busy => None,
            };
            let result = match admission {
                Some(_admission) => answer(stream, &root, &routes),
                None => refuse(stream, BUSY),
            };
            if let Err(err) = result {
                link_format::daemon::warn!("a call was refused: {err:#}");
            }
        });
    }
    Ok(())
}

/// Why a call that waited its whole time for room is refused.
const BUSY: &str = "your secrets' keeper was busy with your other calls for the whole wait: \
                    try again";

/// The file that answers is read from: `<root>/<uid>/user`, its name.
const USER: &str = "user";

/// The store the keeper keeps under `root` for the user whose ID is
/// `uid`, and whose it is: the user's name, noted at its first call, is
/// the user's still. A user removed leaves their store, and a user made
/// later can be given their ID: that user is refused the store, whose
/// secrets were another's, until root removes `<root>/<uid>`.
pub fn user_store(root: &Path, uid: u32) -> anyhow::Result<Store> {
    let dir = root.join(uid.to_string());
    let name = link_machine::machine::user_name_of(uid)
        .with_context(|| format!("user ID {uid} names no user here"))?;
    let noted = dir.join(USER);
    match std::fs::read_to_string(&noted) {
        Ok(was) if was.trim_end() == name => {}
        Ok(_) => anyhow::bail!(
            "the secrets kept under user ID {uid} were another user's, removed since {name} was \
             given the ID: root removes {} before {name}'s are kept",
            dir.display()
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            link_files::write_private(&noted, format!("{name}\n").as_bytes())
                .with_context(|| format!("noting whose {} is", dir.display()))?;
        }
        Err(err) => return Err(err).with_context(|| format!("reading {}", noted.display())),
    }
    Ok(Store::at(dir.join("secrets.toml")))
}

/// One connection's call, refused unasked with `why`: its request is
/// read first, so that the reply reaches the caller before the close.
fn refuse(mut stream: UnixStream, why: &str) -> anyhow::Result<()> {
    stream.set_read_timeout(Some(WAIT))?;
    stream.set_write_timeout(Some(WAIT))?;
    read_line(&mut stream, REQUEST_MAX)?;
    reply(
        &mut stream,
        Reply::Refused {
            error: why.to_string(),
        },
    )
}

/// `reply`, written to `stream` as its one line.
fn reply(stream: &mut UnixStream, reply: Reply) -> anyhow::Result<()> {
    let answered = Answered {
        protocol: PROTOCOL.current,
        reply,
    };
    let mut out = Secret::new(serde_json::to_vec(&answered)?);
    out.expose_mut().push(b'\n');
    stream.write_all(out.expose())?;
    Ok(())
}

/// One connection's call, answered.
pub(super) fn answer(mut stream: UnixStream, root: &Path, routes: &Routes) -> anyhow::Result<()> {
    stream.set_read_timeout(Some(WAIT))?;
    stream.set_write_timeout(Some(WAIT))?;
    let uid = peer_uid(&stream)?;
    let line = read_line(&mut stream, REQUEST_MAX)?;
    let asked = serde_json::from_slice::<Asked<Request>>(line.expose())
        .map_err(|_| anyhow::anyhow!("not a call your secrets' keeper takes"))
        .and_then(|asked| {
            PROTOCOL.accept("the caller", asked.protocol)?;
            Ok(asked.request)
        });
    let replied = match asked {
        Ok(Request::ProtectRoute {
            recipient_uid,
            given,
            binding,
        }) => routes.protect(uid, recipient_uid, given, binding),
        Ok(Request::UnbindRoute { workload, key }) => {
            routes.unbind(uid, &workload, key.as_ref());
            Ok(Reply::Done)
        }
        Ok(request) => user_store(root, uid)
            .and_then(|store| store.answer(request, &Holder::Keeper(PathBuf::new()).kept_by())),
        Err(err) => Err(err),
    }
    .unwrap_or_else(|err| Reply::Refused {
        error: format!("{err:#}"),
    });
    reply(&mut stream, replied)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store is its user's: noted at the first call, and refused to a
    /// user given the ID of one removed, who never reads their secrets.
    #[test]
    fn a_user_given_a_removed_users_id_is_refused_their_secrets() {
        let root = link_test_kit::TempDir::short("keeper-owner");
        // SAFETY: getuid has no failure and touches no memory.
        let uid = unsafe { libc::getuid() };
        let Some(name) = link_machine::machine::user_name_of(uid) else {
            link_test_kit::gate::lacks("a named user", "this user has no entry here", None);
            return;
        };
        let store = user_store(&root, uid).unwrap();
        let noted = root.join(uid.to_string()).join(USER);
        assert_eq!(
            std::fs::read_to_string(&noted).unwrap(),
            format!("{name}\n")
        );
        drop(store);
        // Noted once, its again at every call.
        user_store(&root, uid).unwrap();
        // The secrets of a user removed, under the ID this user has now.
        link_files::write_private(&noted, b"removed-user\n").unwrap();
        let refused = format!("{:#}", user_store(&root, uid).unwrap_err());
        assert!(refused.contains("another user's"), "{refused}");
        assert!(refused.contains("root removes"), "{refused}");
    }

    fn answered(arrival: Arrival) -> Admission {
        match arrival {
            Arrival::Answered(admission) => admission,
            _ => panic!("a call with room waited"),
        }
    }

    #[test]
    fn working_users_cannot_exhaust_root_revocation_capacity() {
        let management = std::sync::Arc::new(Management::default());
        let mut held = Vec::new();
        for uid in 100..108 {
            for _ in 0..PER_USER {
                held.push(answered(management.arrive(uid)));
            }
            assert!(matches!(management.arrive(uid), Arrival::Waiting(_)));
        }
        assert!(matches!(management.arrive(109), Arrival::Waiting(_)));
        let roots: Vec<_> = (0..8).map(|_| answered(management.arrive(0))).collect();
        drop(roots);
        held.pop();
        // The call that waited for room is admitted once a call ends.
        assert!(management.wait(109, Duration::ZERO).is_some());
    }

    /// A user's call past its share waits for one of its own to end, and
    /// is answered then; past its waiting calls, it's closed.
    #[test]
    fn a_users_call_past_its_share_waits_for_room() {
        let management = std::sync::Arc::new(Management::default());
        let held: Vec<_> = (0..PER_USER)
            .map(|_| answered(management.arrive(100)))
            .collect();
        let Arrival::Waiting(uid) = management.arrive(100) else {
            panic!("a call past its user's share wasn't set to wait");
        };
        assert!(management.wait(uid, Duration::ZERO).is_none());
        let Arrival::Waiting(uid) = management.arrive(100) else {
            panic!("a call past its user's share wasn't set to wait");
        };
        let waiting = {
            let management = management.clone();
            std::thread::spawn(move || management.wait(uid, WAIT).is_some())
        };
        drop(held);
        assert!(waiting.join().unwrap(), "the waiting call wasn't admitted");
        for _ in 0..PER_USER {
            std::mem::forget(answered(management.arrive(101)));
        }
        for _ in 0..WAITING_PER_USER {
            assert!(matches!(management.arrive(101), Arrival::Waiting(_)));
        }
        assert!(matches!(management.arrive(101), Arrival::Busy));
    }
}
