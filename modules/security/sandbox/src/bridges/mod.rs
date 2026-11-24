//! `lnk sandbox bridge`: the bridge the sandbox's own routes go to
//! ([`link_sandbox_client::routes`]), one for this machine's user, which
//! the first proxy whose call needs it starts: tools that run outside
//! ([`tools`], and a shared tool's hub, [`shared`]; one at a URL,
//! [`remote`]) and a page opened in your browser ([`open`]), each asking
//! you as it must ([`asks`]). It shares no code with the wall but the
//! contract: each call comes, proved, from a sandbox's proxy.

pub mod asks;
pub mod open;
pub mod remote;
pub mod shared;
pub mod stop;
pub mod tools;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::bail;
use link_sandbox_binding::Identity;
use link_sandbox_binding::Limits;
use link_sandbox_client::grants::Authority;
use link_sandbox_route::Call;
use link_sandbox_route::To;
use link_sandbox_route::bridge::Started;

/// What the bridge takes at once: every running sandbox's tool calls and
/// pages, each waiting its answer, or yours.
pub const LIMITS: Limits = Limits {
    name: "the sandbox's bridge",
    calls: 256,
    per_agent: 32,
    per_owner: 128,
    action_max: 1 << 20,
    action_wait: Duration::from_secs(10),
    answer_wait: Duration::from_secs(10),
};

/// How long it runs with no call: then it's gone, its tools' sessions
/// with it, and the next call starts it again.
pub const IDLE: Duration = Duration::from_secs(30 * 60);

/// What the bridge holds for its calls: each sandbox's tools and their
/// sessions, by the sandbox's name.
struct Held {
    /// Link's `sandbox/` folder: the tools' definitions, the questions
    /// waiting, and each sandbox's wire.
    dir: PathBuf,
    runtime: PathBuf,
    /// The keyring its calls are proved against: who is bound now.
    keys: link_sandbox_binding::Keyring,
    tools: Mutex<HashMap<String, Arc<tools::Tools>>>,
    /// Whose each sandbox's tool calls are, by the sandbox's name: the
    /// ID its calls were proved as.
    callers: Mutex<HashMap<String, String>>,
}

impl Held {
    /// Ends every sandbox's idle tool sessions, and forgets the sandboxes
    /// with none left and no call on them: a call starting its first
    /// session holds its tools, and keeps the session it starts there.
    /// A sandbox whose caller is bound no more (it stopped, its key gone)
    /// has its sessions ended at once: none is left to call them, and a
    /// tool kept running for it (memory's session among them) would keep
    /// what it reaches running too.
    fn sweep(&self) {
        let mut callers = self.callers.lock().unwrap_or_else(|e| e.into_inner());
        callers.retain(|_, caller| self.keys.bound(caller).is_some());
        let mut held = self.tools.lock().unwrap_or_else(|e| e.into_inner());
        held.retain(|sandbox, tools| {
            callers.contains_key(sandbox) && (tools.sweep() || Arc::strong_count(tools) > 1)
        });
    }

    /// Notes that `sandbox`'s tool calls are `who`'s.
    fn called_by(&self, sandbox: &str, who: &Identity) {
        let mut callers = self.callers.lock().unwrap_or_else(|e| e.into_inner());
        callers.insert(sandbox.to_string(), caller_of(who));
    }

    /// Sandbox `sandbox`'s tools, as its route's `given` names them: the
    /// ones held, or new ones (their sessions gone) when they changed.
    fn tools(&self, sandbox: &str, given: link_sandbox_client::routes::Tools) -> Arc<tools::Tools> {
        let mut held = self.tools.lock().unwrap_or_else(|e| e.into_inner());
        let desktop = link_sandbox_route::desktop::only_desktop(&given.desktop);
        if let Some(tools) = held.get(sandbox)
            && tools.named == given.tools
            && tools.desktop == desktop
            && tools.asking == given.asking
        {
            return tools.clone();
        }
        let mut tools = tools::Tools::new(sandbox, self.dir.clone(), given.tools);
        tools.runtime = self.runtime.clone();
        tools.desktop = desktop;
        tools.asking = given.asking;
        let tools = Arc::new(tools);
        held.insert(sandbox.to_string(), tools.clone());
        tools
    }
}

/// Serves the bridge as `started` says, until idle.
#[expect(clippy::disallowed_methods, reason = "the sweep's own period")]
pub async fn serve(started: &Started) -> anyhow::Result<()> {
    let dir = link_files::layout::Hidden::Sandboxes.at(&started.settings);
    link_files::create_private_dir(&dir)?;
    // Its own settings (how long a question waits, how many tools start
    // at once), from Link's settings as it's handed them.
    crate::settings::read_from(&dir);
    let held = Held {
        dir,
        runtime: started.runtime.clone(),
        keys: link_sandbox_binding::Keyring::at(&started.keys),
        tools: Mutex::default(),
        callers: Mutex::default(),
    };
    let held = &held;
    let served = link_sandbox_route::bridge::serve(started, &LIMITS, IDLE, |who, call, stream| {
        handle(held, who, call, stream)
    });
    // Sessions no call came for end, and sandboxes with none left.
    let swept = async {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            held.sweep();
        }
    };
    tokio::select! {
        served = served => served,
        () = swept => Ok(()),
    }
}

/// One call, on its route.
async fn handle(
    held: &Held,
    who: Identity,
    call: Call,
    stream: link_sandbox_binding::stream::Stream,
) -> anyhow::Result<()> {
    let To::Request { head } = &call.to else {
        bail!("the {} route's calls are requests", call.route);
    };
    let head = head.as_bytes();
    // What the caller may reach is asked at each call, never taken from
    // the route alone: a grant taken back since its proxy started is gone.
    let asked = authority(&who, &call.sandbox).await;
    match call.route.as_str() {
        // A tool refused for want of an answer says why, not "not granted".
        "mcp" => match asked {
            Ok(authority) => {
                let given = link_sandbox_route::given(&call.given)?;
                held.called_by(&call.sandbox, &who);
                let tools = held.tools(&call.sandbox, given);
                tools::serve(stream, head, Vec::new(), &tools, &authority).await
            }
            Err(why) => tools::unanswered(stream, head, &why).await,
        },
        "open" => {
            let given = link_sandbox_route::given(&call.given)?;
            let opener = open::Opener::here(&call.sandbox, held.dir.clone(), call.inbound, given);
            // Its login's callback is logged on the sandbox's own wire.
            let wire = link_sandbox_proxy::wire::Wire::new(&call.sandbox, held.dir.clone());
            let authority = asked.unwrap_or_else(|_| Authority::Agent(Default::default()));
            open::serve(stream, head, Vec::new(), &opener, Some(wire), &authority).await
        }
        other => bail!("the sandbox's bridge serves no route {other:?}"),
    }
}

/// The authority of a call proved as `who` from `sandbox`
/// ([`Authority::of`]), or why none could be learned: no keeper answers
/// for the caller, or its keeper failed, as a module built from other
/// sources than this bridge does. Said on stderr too.
async fn authority(who: &Identity, sandbox: &str) -> Result<Authority, String> {
    let (caller, sandbox) = (caller_of(who), sandbox.to_string());
    let asked = tokio::task::spawn_blocking(move || Authority::of(&caller, &sandbox)).await;
    let why = match asked {
        Ok(Ok(authority)) => return Ok(authority),
        Ok(Err(err)) => format!("{err:#}"),
        Err(err) => format!(
            "asking what {} may reach: {err}",
            link_format::clean(&caller_of(who))
        ),
    };
    link_format::daemon::warn!("{why}");
    Err(why)
}

/// The ID `who`'s calls were proved as.
fn caller_of(who: &Identity) -> String {
    who.agent.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bridge's hold on `dir`, its keyring in `dir/keys`.
    fn held(dir: &std::path::Path) -> Held {
        Held {
            dir: dir.to_path_buf(),
            runtime: dir.join("runtime"),
            keys: link_sandbox_binding::Keyring::at(&dir.join("keys")),
            tools: Mutex::default(),
            callers: Mutex::default(),
        }
    }

    fn given() -> link_sandbox_client::routes::Tools {
        link_sandbox_client::routes::Tools {
            tools: vec!["task".into()],
            desktop: Vec::new(),
            asking: None,
        }
    }

    /// Caller `id` of `held`'s keyring, bound.
    fn bound(held: &Held, id: &str) -> Identity {
        let identity = Identity {
            agent: id.into(),
            service: "s1t2u3v4w5x6".into(),
            owner: "me@box".into(),
            ..Identity::default()
        };
        held.keys
            .bind(&identity, &link_secretive::Secret::from("k".repeat(32)))
            .unwrap();
        identity
    }

    /// A sandbox's first call can take longer than a sweep's period to
    /// start its tool: the tools it holds stay held, so the session it
    /// starts is the one its next call finds.
    #[test]
    fn a_sweep_keeps_the_tools_a_call_holds() {
        let dir = link_test_kit::TempDir::new("bridge-sweep");
        let held = held(&dir);
        let who = bound(&held, "a1b2c3d4e5f6");
        held.called_by("main", &who);
        let calling = held.tools("main", given());
        held.sweep();
        assert!(Arc::ptr_eq(&calling, &held.tools("main", given())));
        drop(calling);
        held.sweep();
        assert!(held.tools.lock().unwrap().is_empty());
    }

    /// A caller stopped, its key gone, has its sandbox's tools let go at
    /// the next sweep, their sessions with them; another caller's stay.
    #[test]
    fn a_sweep_lets_go_of_the_tools_of_a_caller_bound_no_more() {
        let dir = link_test_kit::TempDir::new("bridge-unbound");
        let held = held(&dir);
        let gone = bound(&held, "a1b2c3d4e5f6");
        let staying = bound(&held, "b1c2d3e4f5a6");
        held.called_by("main", &gone);
        held.called_by("other", &staying);
        let main = held.tools("main", given());
        let other = held.tools("other", given());
        struct Unrevoked;
        impl link_sandbox_binding::Revoker for Unrevoked {
            fn unbind(&self, _: &str, _: Option<&link_secretive::Secret>) {}
        }
        held.keys.unbind(&caller_of(&gone), &Unrevoked);
        held.sweep();
        let tools = held.tools.lock().unwrap();
        assert!(!tools.contains_key("main"));
        assert!(tools.contains_key("other"));
        drop((main, other));
    }
}
