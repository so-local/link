//! Who holds your secrets ([`Holder`]): files of your own user ([`Store`]),
//! or, on Linux, the keeper, a service run as a user of its own
//! ([`KEEPER_USER`]) that keeps each user's secrets in a store of theirs
//! and answers on a socket outside every sandbox ([`SOCKET`]). Every call
//! is a [`Request`], answered with a [`Reply`] by the same code either
//! way ([`Store::answer`]): the keeper runs it on the store of the user
//! at the socket's other end, so a caller reaches only its own user's
//! secrets, and a grant is checked where the value is.
//!
//! On the wire, each connection is one call: the caller writes its
//! request as one JSON line, the keeper its reply as another.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use super::{
    Added, Called, Copy, ForService, Grantee, Listed, Removal, Removing, Sealing, Secret, Store,
    Use, Which, Yours, add_in, copied_in, forget_in, gone_from_in, grant_in, held_in, keep_own_in,
    reached, remove_here,
};
use crate::uninstall::Held;

mod answering;
mod calling;
mod routing;
mod serving;
use calling::{call, peer_uid, read_line, socket};
pub use routing::{RouteRevoker, Routes, protect_routes, protect_routes_for, unbind_route};
pub use serving::{serve, serve_with_routes, user_store};

/// The user the keeper runs as: it holds secrets and runs nothing else.
pub const KEEPER_USER: &str = "lnk-secrets";

/// Where the keeper answers.
pub const SOCKET: &str = "/run/lnk-secrets/keeper.sock";
pub const ROUTE_SOCKET: &str = "/run/lnk-secrets/secret.sock";

/// The keeper's socket in place of [`SOCKET`], for tests, whether or not
/// there's a [`KEEPER_USER`]: one the test's own keeper serves. Read by a
/// debug build only ([`crate::test_var`]).
pub const TEST_KEEPER: &str = "LNK_TEST_KEEPER";

/// The most a request is: a secret is one line.
const REQUEST_MAX: u64 = 1 << 20;

/// The keeper protocol, as `keeper/protocol.toml` declares it: what a
/// request ([`Asked`]) and its reply ([`Answered`]) each state.
pub static PROTOCOL: std::sync::LazyLock<link_version::Protocol> = std::sync::LazyLock::new(|| {
    link_version::Protocol::shipped(include_str!("keeper/protocol.toml"))
});

/// A request as it's sent, a line of JSON: the caller's protocol and
/// the request.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Asked<T> {
    protocol: u32,
    request: T,
}

/// A reply as it's sent, a line of JSON: the keeper's protocol and the
/// reply.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Answered<T> {
    protocol: u32,
    reply: T,
}

/// The most a reply is.
const REPLY_MAX: u64 = 16 << 20;

/// How long a call may take to be written or answered.
const WAIT: Duration = Duration::from_secs(30);

/// How long a call past its user's share waits for room: well under
/// [`WAIT`], so the refusal that ends a wait reaches its caller in time.
const QUEUED: Duration = Duration::from_secs(20);

/// The most calls the keeper answers at once for users other than root;
/// more wait for one to end.
const AT_ONCE: usize = 32;

/// The most calls the keeper answers at once for one user; more wait.
const PER_USER: usize = 4;

/// The most calls of one user that wait to be answered; past these, a
/// call is refused as busy, by a reply.
const WAITING_PER_USER: usize = 64;

/// A call to whoever holds your secrets.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "call", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    ProtectRoute {
        #[serde(default)]
        recipient_uid: Option<u32>,
        #[serde(with = "link_secretive::bare")]
        given: Secret,
        binding: crate::bridge::Binding,
    },
    UnbindRoute {
        workload: String,
        #[serde(with = "link_secretive::bare")]
        key: Option<Secret>,
    },
    List,
    ForService {
        id: String,
        which: Which,
    },
    /// Your own secrets of those IDs, read as their holder.
    ForYou {
        ids: Vec<String>,
    },
    Add {
        name: String,
        #[serde(with = "link_secretive::bare")]
        value: Secret,
        sealed: Option<Sealing>,
        to: Vec<Grantee>,
    },
    /// Grants added to a secret kept, and grants taken from it.
    Grant {
        called: Called,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        add: Vec<Grantee>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        remove: Vec<Grantee>,
    },
    /// A service's own secrets: `id` its ID, `workload` what acts as it,
    /// `<kind>:<id>`.
    KeepOwn {
        id: String,
        workload: String,
        name: String,
        own: Vec<Own>,
    },
    Forget {
        id: String,
    },
    RetainOwn {
        id: String,
        uses: Vec<Use>,
    },
    Copied {
        id: String,
        to: super::Place,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        without: Vec<Use>,
    },
    GoneFrom {
        id: String,
        from: super::Place,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        without: Vec<Use>,
    },
    Remove {
        what: Removal,
    },
    Reached {
        id: String,
        reached: Vec<Copy>,
    },
    Held,
    /// Every secret kept for you gone, the store with them (`lnk
    /// uninstall`).
    Clear,
}

/// One of a service's own secrets, as [`super::keep_own`] keeps it: none,
/// as it's kept, its value not read.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Own {
    pub r#as: Use,
    #[serde(with = "link_secretive::bare")]
    pub value: Option<Secret>,
}

/// The answer to a [`Request`].
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reply {
    Protected {
        #[serde(with = "link_secretive::bare")]
        given: Secret,
    },
    Done,
    Listed {
        listed: Vec<Listed>,
    },
    ForService {
        given: Vec<ForService>,
    },
    ForYou {
        given: Vec<Yours>,
    },
    /// A secret added: its ID, what it replaced, and a Keychain item
    /// left.
    Replaced {
        added: Box<Added>,
    },
    /// Whether a grant changed.
    Granted {
        changed: bool,
    },
    Removing {
        removing: Removing,
    },
    Held {
        held: Vec<Held>,
    },
    /// Refused, saying why: never a secret's value.
    Refused {
        error: String,
    },
}

impl Reply {
    fn name(&self) -> &'static str {
        match self {
            Reply::Protected { .. } => "protected",
            Reply::Done => "done",
            Reply::Listed { .. } => "listed",
            Reply::ForService { .. } => "for_service",
            Reply::ForYou { .. } => "for_you",
            Reply::Replaced { .. } => "replaced",
            Reply::Granted { .. } => "granted",
            Reply::Removing { .. } => "removing",
            Reply::Held { .. } => "held",
            Reply::Refused { .. } => "refused",
        }
    }

    fn other(self, wanted: &str) -> anyhow::Error {
        anyhow::anyhow!(
            "your secrets' keeper answered {} to a call answered {wanted}",
            self.name()
        )
    }

    pub fn done(self) -> anyhow::Result<()> {
        match self {
            Reply::Done => Ok(()),
            other => Err(other.other("done")),
        }
    }

    pub fn listed(self) -> anyhow::Result<Vec<Listed>> {
        match self {
            Reply::Listed { listed } => Ok(listed),
            other => Err(other.other("listed")),
        }
    }

    pub fn for_you(self) -> anyhow::Result<Vec<Yours>> {
        match self {
            Reply::ForYou { given } => Ok(given),
            other => Err(other.other("for_you")),
        }
    }

    pub fn for_service(self) -> anyhow::Result<Vec<ForService>> {
        match self {
            Reply::ForService { given } => Ok(given),
            other => Err(other.other("for_service")),
        }
    }

    pub fn replaced(self) -> anyhow::Result<Added> {
        match self {
            Reply::Replaced { added } => Ok(*added),
            other => Err(other.other("replaced")),
        }
    }

    pub fn granted(self) -> anyhow::Result<bool> {
        match self {
            Reply::Granted { changed } => Ok(changed),
            other => Err(other.other("granted")),
        }
    }

    pub fn removing(self) -> anyhow::Result<Removing> {
        match self {
            Reply::Removing { removing } => Ok(removing),
            other => Err(other.other("removing")),
        }
    }

    pub fn held(self) -> anyhow::Result<Vec<Held>> {
        match self {
            Reply::Held { held } => Ok(held),
            other => Err(other.other("held")),
        }
    }
}

/// Who holds your secrets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Holder {
    /// Files of your own user.
    Files(Store),
    /// The keeper, at its socket.
    Keeper(PathBuf),
}

impl Holder {
    /// `request`, answered by whoever holds them.
    pub fn ask(&self, request: Request) -> anyhow::Result<Reply> {
        let reply = match self {
            Holder::Files(store) => store.answer(request, &self.kept_by())?,
            Holder::Keeper(socket) => call(socket, &request)?,
        };
        match reply {
            Reply::Refused { error } => Err(anyhow::anyhow!(error)),
            reply => Ok(reply),
        }
    }

    /// Where a value it holds is kept, as `lnk secret list` says it.
    pub fn kept_by(&self) -> String {
        match self {
            Holder::Files(store) => format!("{} (only yours)", store.file().display()),
            Holder::Keeper(_) => format!("the keeper's store ({KEEPER_USER}'s alone)"),
        }
    }
}

/// Who holds your secrets here: the keeper where there's one (a
/// [`KEEPER_USER`] on this machine), else files of your own. A keeper
/// never takes your file: while it holds a secret, the keeper is refused
/// ([`yours_held`]).
pub(super) fn holder() -> anyhow::Result<Holder> {
    let Some(socket) = socket() else {
        return Ok(Holder::Files(Store::yours()));
    };
    yours_held()?;
    Ok(Holder::Keeper(socket))
}

/// An error while your own file holds a secret, or can't be read: a
/// keeper here keeps its own store, and your file's secrets are added to
/// it again, never taken.
pub fn yours_held() -> anyhow::Result<()> {
    let yours = Store::yours();
    let held = match yours.load() {
        Ok(secrets) => !secrets.entry.is_empty() || !secrets.left.is_empty(),
        Err(_) => true,
    };
    if held {
        bail!(
            "{} keeps secrets of yours, which the keeper, {KEEPER_USER}, never takes: remove \
             the file, then add them again with lnk secret create",
            yours.file().display()
        );
    }
    Ok(())
}

/// Whether there's a keeper here: a [`KEEPER_USER`] on this machine (or
/// a test's keeper).
pub fn here() -> bool {
    socket().is_some()
}

/// Whether this process runs as [`KEEPER_USER`].
pub fn running_as_keeper() -> bool {
    crate::machine::user_name().as_deref() == Some(KEEPER_USER)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A keeper on a socket of its own, its stores under `root`.
    fn keeper(dir: &Path) -> PathBuf {
        let socket = dir.join("run").join("keeper.sock");
        std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
        let listener = crate::socket::listen_open(&socket).unwrap();
        let root = dir.join("lib");
        std::thread::spawn(move || serve(listener, &root));
        socket
    }

    #[test]
    fn the_keeper_keeps_each_users_in_a_store_of_theirs() {
        let dir = link_test_kit::TempDir::short("keeper-stores");
        let socket = keeper(&dir);
        let holder = Holder::Keeper(socket);
        let id = link_id::candidate().unwrap();
        let grant = super::super::Grantee::new(
            &format!("run:{}", link_id::candidate().unwrap()),
            "main",
            &id,
        )
        .unwrap();
        holder
            .ask(Request::Add {
                name: "NPM_TOKEN".into(),
                value: Secret::from("npm_hunter2".to_string()),
                sealed: Some(Sealing::new("registry.npmjs.org", None, None, None).unwrap()),
                to: vec![grant],
            })
            .unwrap()
            .replaced()
            .unwrap();
        // Kept in this user's own store, the keeper's alone.
        // SAFETY: getuid has no failure and touches no memory.
        let uid = unsafe { libc::getuid() };
        let kept = dir.join("lib").join(uid.to_string()).join("secrets.toml");
        assert!(
            std::fs::read_to_string(&kept)
                .unwrap()
                .contains("npm_hunter2")
        );
        // Given where it's granted, and to no one else.
        let given = |id: &str| {
            holder
                .ask(Request::ForService {
                    id: id.into(),
                    which: Which::Variables,
                })
                .unwrap()
                .for_service()
                .unwrap()
        };
        assert_eq!(given(&id)[0].value.expose(), "npm_hunter2");
        assert!(given(&link_id::candidate().unwrap()).is_empty());
        // Listed, never with its value.
        let listed = holder.ask(Request::List).unwrap().listed().unwrap();
        assert_eq!(listed.len(), 1);
        let held = holder.ask(Request::Held).unwrap().held().unwrap();
        assert!(held[0].kept.contains(KEEPER_USER), "{held:?}");
    }

    #[test]
    fn parked_models_stay_in_the_keeper_without_a_grant_and_removal_revokes_them() {
        let dir = link_test_kit::TempDir::short("keeper-parked-models");
        let holder = Holder::Keeper(keeper(&dir));
        let id = link_id::candidate().unwrap();
        let own = |value| Request::KeepOwn {
            id: id.clone(),
            workload: format!("run:{}", link_id::candidate().unwrap()),
            name: "main".into(),
            own: vec![Own {
                r#as: Use::own("model").unwrap(),
                value,
            }],
        };
        holder
            .ask(own(Some(Secret::from("sk-source".to_string()))))
            .unwrap()
            .done()
            .unwrap();
        holder
            .ask(Request::Copied {
                id: id.clone(),
                to: super::super::Place::of_account("host", None),
                without: vec![Use::own("model").unwrap(), Use::own("fallback").unwrap()],
            })
            .unwrap()
            .done()
            .unwrap();
        holder
            .ask(Request::RetainOwn {
                id: id.clone(),
                uses: vec![Use::own("model").unwrap()],
            })
            .unwrap()
            .done()
            .unwrap();
        holder
            .ask(Request::Forget { id: id.clone() })
            .unwrap()
            .done()
            .unwrap();
        let listed = holder.ask(Request::List).unwrap().listed().unwrap();
        assert!(listed[0].kept);
        assert!(listed[0].to.is_empty());
        assert!(listed[0].copies.is_empty());
        let given = || {
            holder
                .ask(Request::ForService {
                    id: id.clone(),
                    which: Which::Own,
                })
                .unwrap()
                .for_service()
                .unwrap()
        };
        assert!(given().is_empty());
        // Another service's own model key: none here.
        let other = super::super::Owner {
            principal: super::super::service_principal(&link_id::candidate().unwrap()).unwrap(),
            r#as: Use::own("model").unwrap(),
        };
        let refused = holder
            .ask(Request::Remove {
                what: Removal::Own(other),
            })
            .unwrap()
            .removing()
            .unwrap();
        assert!(!refused.removed.here);
        // Another workload named main takes nothing of it: it's kept by ID.
        let another = link_id::candidate().unwrap();
        holder
            .ask(Request::KeepOwn {
                id: another.clone(),
                workload: format!("run:{}", link_id::candidate().unwrap()),
                name: "main".into(),
                own: vec![Own {
                    r#as: Use::own("model").unwrap(),
                    value: None,
                }],
            })
            .unwrap()
            .done()
            .unwrap();
        let of_another = holder
            .ask(Request::ForService {
                id: another,
                which: Which::Own,
            })
            .unwrap()
            .for_service()
            .unwrap();
        assert!(of_another.is_empty());
        holder.ask(own(None)).unwrap().done().unwrap();
        assert_eq!(given()[0].value.expose(), "sk-source");
        holder
            .ask(Request::RetainOwn {
                id: id.clone(),
                uses: vec![Use::own("model").unwrap()],
            })
            .unwrap()
            .done()
            .unwrap();
        holder
            .ask(Request::Forget { id: id.clone() })
            .unwrap()
            .done()
            .unwrap();
        let id_of = listed[0].id.clone();
        holder
            .ask(Request::Remove {
                what: Removal::Secret(Called::Id(id_of)),
            })
            .unwrap()
            .removing()
            .unwrap();
        holder.ask(own(None)).unwrap().done().unwrap();
        assert!(given().is_empty());
        assert!(
            holder
                .ask(Request::List)
                .unwrap()
                .listed()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn the_keeper_clears_what_it_keeps_for_you() {
        let dir = link_test_kit::TempDir::short("keeper-clear");
        let holder = Holder::Keeper(keeper(&dir));
        let id = holder
            .ask(Request::Add {
                name: "A".into(),
                value: Secret::from("one".to_string()),
                sealed: None,
                to: Vec::new(),
            })
            .unwrap()
            .replaced()
            .unwrap()
            .id;
        let listed = holder.ask(Request::List).unwrap().listed().unwrap();
        assert_eq!(
            listed.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
            [id.as_str()]
        );
        // Cleared, as `lnk uninstall` asks: nothing kept.
        holder.ask(Request::Clear).unwrap().done().unwrap();
        assert!(
            holder
                .ask(Request::List)
                .unwrap()
                .listed()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_call_that_isnt_one_is_refused_and_a_socket_others_write_is_never_called() {
        let dir = link_test_kit::TempDir::short("keeper-bindings");
        let socket = keeper(&dir);
        let mut stream = UnixStream::connect(&socket).unwrap();
        stream.write_all(b"{\"call\": \"steal\"}\n").unwrap();
        let reply = read_line(&mut stream, REPLY_MAX).unwrap();
        let reply: Answered<Reply> = serde_json::from_slice(reply.expose()).unwrap();
        assert_eq!(reply.protocol, PROTOCOL.current);
        assert!(
            matches!(reply.reply, Reply::Refused { .. }),
            "{:?}",
            reply.reply
        );
        // A caller of an older protocol is refused by name.
        let mut stream = UnixStream::connect(&socket).unwrap();
        let older = format!(
            "{{\"protocol\": {}, \"request\": {{\"call\": \"list\"}}}}\n",
            PROTOCOL.oldest - 1
        );
        stream.write_all(older.as_bytes()).unwrap();
        let reply = read_line(&mut stream, REPLY_MAX).unwrap();
        let reply: Answered<Reply> = serde_json::from_slice(reply.expose()).unwrap();
        let Reply::Refused { error } = reply.reply else {
            panic!("an older caller's request was taken");
        };
        assert!(
            error.contains("the caller speaks the secret keeper protocol 0"),
            "{error}"
        );
        // A folder others may write: a socket put there isn't the keeper's.
        use std::os::unix::fs::PermissionsExt;
        let run = socket.parent().unwrap();
        std::fs::set_permissions(run, std::fs::Permissions::from_mode(0o777)).unwrap();
        let refused = call(&socket, &Request::List).unwrap_err();
        assert!(
            format!("{refused:#}").contains("written by others"),
            "{refused:#}"
        );
    }
}
