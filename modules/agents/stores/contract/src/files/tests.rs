use super::*;
use link_test_kit::TempDir;

/// Agent `agent` of `me@box`, acting as the service a test's agents do.
fn who(agent: &str) -> Identity {
    Identity {
        agent: agent.into(),
        service: crate::testing::TEST_SERVICE.into(),
        owner: "me@box".into(),
        teams: Vec::new(),
    }
}

/// Each agent's Home its own folder of the stand-in bucket.
struct Folders {
    dir: PathBuf,
    cap: Option<u64>,
    changed: Mutex<Vec<(String, String)>>,
}

impl Homes for Folders {
    fn home(&self, who: &Identity) -> anyhow::Result<Home> {
        Ok(Home {
            store: Arc::new(Crypt::new(
                DirStore::new(&self.dir.join(&who.agent))?,
                &format!("key of {}", who.agent),
            )?),
            cap: self.cap,
            write: vec!["work".into(), "memory".into()],
        })
    }
    fn changed(&self, who: &Identity, path: &str) {
        self.changed
            .lock()
            .unwrap()
            .push((who.agent.clone(), path.into()));
    }
}

fn bridge(dir: &Path, cap: Option<u64>) -> (PathBuf, Arc<Folders>) {
    let socket = dir.join("files.sock");
    let homes = Arc::new(Folders {
        dir: dir.join("bucket"),
        cap,
        changed: Mutex::new(Vec::new()),
    });
    let listener = UnixListener::bind(&socket).unwrap();
    let keys = Arc::new(crate::bridge::Keyring::at(&dir.join("keys")));
    let served = homes.clone();
    std::thread::spawn(move || serve(listener, keys, served));
    (socket, homes)
}

/// A Home in a plain folder, its cap `cap`.
struct Plain {
    store: Arc<DirStore>,
    cap: Option<u64>,
}

impl Homes for Plain {
    fn home(&self, _who: &Identity) -> anyhow::Result<Home> {
        Ok(Home {
            store: self.store.clone(),
            cap: self.cap,
            write: vec!["work".into()],
        })
    }
}

/// A bridge in `dir`, taking calls of the agents bound in `dir/keys`,
/// and its socket.
fn served(dir: &Path, homes: Arc<dyn Homes>) -> (PathBuf, Arc<Bridge>) {
    served_within(dir, homes, LIMITS)
}

/// [`served`], within `limits`.
fn served_within(
    dir: &Path,
    homes: Arc<dyn Homes>,
    limits: crate::bridge::Limits,
) -> (PathBuf, Arc<Bridge>) {
    let socket = dir.join("files.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let keys = Arc::new(crate::bridge::Keyring::at(&dir.join("keys")));
    let bridge = Arc::new(Bridge::new(keys, homes));
    let run = bridge.clone();
    std::thread::spawn(move || serve_bridge(listener, run, &limits));
    (socket, bridge)
}

mod calls;
mod connections;
mod remote;
mod teams;
