//! `lnk agent harness check` and `show`: a harness's sandbox tested,
//! and its home and traces shown.

use std::path::{Path, PathBuf};

use link_agent::{
    adapter::{Adapter, Boxed},
    service,
    settings::{self, Settings},
    traces,
};
use link_format::{
    Done, Fields, Foreign, Name, Note, Out, Path as Where, Report, Table, Text, Value, cmd, prose,
    sentence, states,
};

use crate::confine::{proxy_secrets, sandbox_can_run};
use crate::permissions::granted;
use crate::tilde;

states! {
    /// What a probe of the agent's own found.
    enum Held {
        Blocked("blocked") => Good,
        Leaked("leaked") => Bad,
    }
}

/// `lnk agent harness check`.
pub(crate) fn check_sandbox(
    out: &mut Out,
    settings: &Settings,
    name: &str,
) -> Result<Done, Report> {
    let harness = Name::new("harness", name);
    if !settings.sandbox {
        return Err(Report::error(sentence!(
            "The sandbox is off: {} can reach everything you can.",
            &harness
        ))
        .next(cmd!("lnk agent start --sandbox"), "turn it back on"));
    }
    // Its probes stand in for the harness, which runs its adapter.
    let program = Adapter::find(name)?.program().to_path_buf();
    sandbox_can_run()?;
    let mut boxed = Boxed::new(settings, name)?;
    boxed.ports.extend(settings.model_port());
    out.block(Note(sentence!(
        "Trying, from inside {}'s sandbox, what it shouldn't be able to do.",
        &harness
    )));
    let profile = format!("{}-check", boxed.profile);
    // The sandbox it runs in: behind its proxy, the proxy is its only
    // network (its own sockets' folder, not the running agent's).
    let confinement = match settings.proxy_of(name) {
        Some(upstream) => {
            let mut spec = boxed.spec(true);
            // The check reaches no tool: it calls none; and its own probes
            // aren't counted against it.
            spec.tools = None;
            if let Some(internet) = &mut spec.internet {
                internet.per_minute = None;
            }
            let at = link_sandbox_client::ProxyAt {
                upstream,
                dir: settings.net_dir(&format!("{name}-check")),
                ..Default::default()
            };
            let way = link_sandbox_client::Way::Proxy(at);
            boxed.confinement(spec, &program, &profile, &[], &[], true, way)?
        }
        None => {
            let (spec, way) = boxed.unproxied(&profile);
            boxed.confinement(spec, &program, &profile, &[], &[], true, way)?
        }
    };
    // Its teams' folders it's granted, read; every other team's, not.
    let reads: Vec<PathBuf> = boxed
        .teams
        .write
        .iter()
        .chain(&boxed.teams.read)
        .cloned()
        .collect();
    let mine: Vec<&String> = boxed.teams.teams.iter().map(|(name, _, _)| name).collect();
    let mut unread: Vec<PathBuf> = settings::teams()
        .unwrap_or_default()
        .into_iter()
        .filter(|team| !mine.contains(&&team.name))
        .filter_map(|team| team.shared)
        .collect();
    // Nor where its proxy's secrets are kept: what its placeholders are
    // made with (`placeholders.secret`) and its URLs' secret (`url.secret`).
    unread.push(settings.wires());
    // The sandbox's own table, on this terminal.
    out.hand_over();
    let mut clean = link_sandbox_client::check_reading(&confinement, &reads, &unread)?;
    let home = settings.box_of(name);
    let mut own = Table::new(["RESULT", "WHAT", "NOTE"], sentence!("Nothing was tried."));
    let mut probe = |what: String, secret: Option<&[u8]>, kept: &str| {
        let (state, note) = match secret.and_then(|secret| holding(&home, secret)) {
            Some(file) => {
                clean = false;
                (Held::Leaked, format!("it is in {}", file.display()))
            }
            None => (Held::Blocked, kept.to_string()),
        };
        own.row([
            Value::from(state),
            Value::from(what),
            Value::from(Foreign::new(&note)),
        ]);
    };
    // What its placeholders are made with is nowhere in its home: only
    // the placeholders themselves are.
    if settings.proxy_of(name).is_some() {
        let kept = settings.placeholders()?.kept().keep();
        probe(
            "what its placeholders are made with".into(),
            kept.as_ref().map(|k| k.expose().as_bytes()),
            "kept in Link's settings",
        );
    }
    // With keys on the wire, the real key must be nowhere it can read.
    if let Some(model) = settings.wired_model()? {
        probe(
            "its model key, which its proxy adds".into(),
            Some(model.api_key.expose().as_bytes()),
            "added by its proxy, not in its home",
        );
    }
    for secret in proxy_secrets(settings, name)? {
        probe(
            format!("its {} secret, which its proxy adds", secret.named.name),
            Some(secret.value.expose().as_bytes()),
            "added by its proxy, not in its home",
        );
    }
    out.block(own);
    let granted = granted(settings, name);
    out.block(Fields::new().row(
        "Allowed",
        match granted.is_empty() {
            true => Value::from("nothing beyond its home"),
            false => Value::List(granted.iter().map(Value::from).collect()),
        },
    ));
    if !clean {
        return Err(Report::error(sentence!(
            "Something got through the sandbox that shouldn't have."
        ))
        .why(prose!("It shows as leaked above.")));
    }
    Ok(Done::shown().next(
        cmd!("lnk agent harness-permission list --harness {}", &harness),
        "see what it is allowed",
    ))
}

/// A file under `dir` holding `secret`, if any: links not followed,
/// files over 64 MiB skipped. The harness writes `dir`, even while this
/// runs, so each name is opened from its folder's handle without
/// following a link, and read only if the handle is a regular file, at
/// most 64 MiB of it: a link, pipe or device swapped in after the walk
/// can't make `lnk agent harness check` read elsewhere, hang or run
/// out of memory.
pub(crate) fn holding(dir: &std::path::Path, secret: &[u8]) -> Option<PathBuf> {
    use link_files::nofollow::{Dir, Kind};
    use std::io::Read;
    const LIMIT: u64 = 64 << 20;
    if secret.is_empty() {
        return None;
    }
    // Depth first, each folder open only while it's walked: as many
    // handles as the tree is deep.
    let open = |folder: Dir| {
        let names = folder.names().unwrap_or_default().into_iter();
        (folder, names)
    };
    let mut stack = vec![(open(Dir::open(dir).ok()?), dir.to_path_buf())];
    while let Some(((folder, names), path)) = stack.last_mut() {
        let Some(name) = names.next() else {
            stack.pop();
            continue;
        };
        let path = path.join(&name);
        match folder.kind(&name) {
            Ok(Kind::Folder) => {
                if let Ok(inner) = folder.folder(&name) {
                    stack.push((open(inner), path));
                }
            }
            Ok(Kind::File) => {
                let Ok(file) = folder.open_entry(&name) else {
                    continue;
                };
                if !file
                    .metadata()
                    .is_ok_and(|m| m.is_file() && m.len() <= LIMIT)
                {
                    continue;
                }
                let mut bytes = Vec::new();
                if file.take(LIMIT).read_to_end(&mut bytes).is_ok()
                    && bytes.windows(secret.len()).any(|w| w == secret)
                {
                    return Some(path);
                }
            }
            _ => {}
        }
    }
    None
}

/// `lnk agent harness show`: the home, Link's own files for it, and
/// what's outside.
pub(crate) fn show(out: &mut Out, settings: &Settings, name: &str) -> Result<Done, Report> {
    let adapter = Adapter::find(name)?;
    let dir = settings.box_of(name);
    let active = settings.harness() == name;
    let state = match (active, active && service::running(&settings.name)) {
        (true, true) => "active, running",
        (true, false) => "active, stopped",
        _ => "not active",
    };
    let home = match dir.exists() {
        true => format!(
            "{}  {}",
            tilde(&dir),
            traces::size(&dir).map(traces::human).unwrap_or_default()
        ),
        false => format!("{} (not installed)", tilde(&dir)),
    };
    let granted = granted(settings, name);
    let allowed = match granted.is_empty() {
        true => "nothing more".to_string(),
        false => granted.join(", "),
    };
    let mut fields = Fields::new()
        .row("Harness", format!("{name} ({state})"))
        .row("Its home", Foreign::new(&home))
        .row("Your files", Where::new(settings.files()))
        .row(
            "Sandbox",
            match settings.sandbox {
                true => format!(
                    "on: it writes only its home, your files and /tmp/{name}*; allowed: {allowed}"
                ),
                false => "off: it can read and write your whole home folder".to_string(),
            },
        );
    let own: Vec<Value> = link_files(settings, name)
        .into_iter()
        .filter(|p| p.exists())
        .map(|p| Value::from(Where::new(p)))
        .collect();
    if !own.is_empty() {
        fields = fields.row("Link's own", Value::List(own));
    }
    out.block(fields);
    let outside = outside(&adapter, name);
    if outside.is_empty() {
        out.block(Text(prose!(
            "Nothing of {} is outside its home.",
            Name::new("harness", name)
        )));
    } else {
        let mut found = Table::new(["PATH", "SIZE", "NOTE"], sentence!("Nothing was found."));
        for trace in &outside {
            let size = traces::size(&trace.path)
                .filter(|s| *s >= 1 << 20)
                .map(traces::human)
                .unwrap_or_default();
            found.row([
                Value::from(Where::new(&trace.path)),
                Value::from(size),
                Value::from(guessed(trace)),
            ]);
        }
        out.block(found);
        out.block(Note(sentence!(
            "These sit outside its box: written unboxed, or with the sandbox off."
        )));
    }
    let mut done = Done::shown();
    if !outside.is_empty() {
        done = done.next(
            cmd!(
                "lnk agent harness delete --harness {}",
                Name::new("harness", name)
            ),
            "remove them with its home",
        );
    }
    let off = !settings.sandbox;
    if off {
        done = done.next(
            cmd!("lnk agent start --sandbox"),
            "turn the sandbox back on",
        );
    }
    Ok(done)
}

/// What Link itself keeps for the agent's harness `name`, outside its home.
pub(crate) fn link_files(settings: &Settings, name: &str) -> Vec<PathBuf> {
    let profile = settings.profile(name);
    let log = settings.log_path(name);
    [
        profile.clone(),
        format!("{profile}-setup"),
        format!("{profile}-check"),
    ]
    .iter()
    .flat_map(|sandbox| link_sandbox_client::files(sandbox))
    .chain([service::start_path(&log), log])
    .collect()
}

/// A note after a trace: what the adapter says is there, or that it was
/// found by its name alone.
pub(crate) fn guessed(trace: &traces::Trace) -> String {
    match (trace.known, trace.kind) {
        (true, Some(kind)) => kind.label().to_string(),
        (true, None) => String::new(),
        (false, _) => "by its name only: check it is this harness's".into(),
    }
}

/// Everything of harness `name` found outside its home.
pub(crate) fn outside(adapter: &Adapter, name: &str) -> Vec<traces::Trace> {
    let keep = [settings::root(), link_files::installation_dir()];
    let home = link_files::home_dir();
    let declared = adapter.traces();
    let mut found = traces::outside(name, &declared, &home, Path::new("/tmp"), &keep);
    // Its services' files Link kept off, in `~/.lnk/safe/agents/stopped-services` or beside
    // them as before: its own, though in Link's folder.
    let uid = link_service::user_id().unwrap_or_default();
    for trace in declared
        .iter()
        .filter(|t| t.kind == link_harness_contract::harness::traces::Kind::Service)
    {
        let Some(at) = link_files::Relative::within(&home, &trace.at(&home, &uid)) else {
            continue;
        };
        for path in link_agent::migrating::stopped_places(&home, &at).unwrap_or_default() {
            if path.symlink_metadata().is_ok() && !found.iter().any(|t| t.path == path) {
                found.push(traces::Trace {
                    path,
                    known: true,
                    kind: Some(link_harness_contract::harness::traces::Kind::Service),
                });
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What Link keeps for a harness is every file the sandbox keeps for
    /// each of its sandboxes, the secrets its proxy is handed among them,
    /// so removing the harness removes them too.
    #[test]
    fn a_harness_files_are_every_file_its_sandboxes_keep() {
        // Its files are under HOME, which another test sets once a run.
        let _config = crate::tests::own_config();
        let agent = Settings::new("main");
        let kept = link_files(&agent, "claude");
        let profile = agent.profile("claude");
        for sandbox in [profile.clone(), format!("{profile}-setup")] {
            for file in link_sandbox_client::files(&sandbox) {
                assert!(kept.contains(&file), "{}", file.display());
            }
        }
        let names: Vec<String> = kept
            .iter()
            .filter_map(|f| Some(f.file_name()?.to_str()?.to_string()))
            .collect();
        assert!(
            names.iter().any(|n| n.ends_with(".url-secret")),
            "{names:?}"
        );
    }

    #[test]
    fn holding_reads_only_files_it_opened_as_files() {
        use std::os::unix::ffi::OsStrExt;
        let dir = link_test_kit::TempDir::new("holding");
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("a/b")).unwrap();
        std::fs::write(dir.join("outside"), "the-secret").unwrap();
        // A link to a file holding it, and a pipe nothing writes to: the
        // first isn't followed, the second doesn't hold the check.
        std::os::unix::fs::symlink(dir.join("outside"), home.join("link")).unwrap();
        let fifo = std::ffi::CString::new(home.join("a/pipe").as_os_str().as_bytes()).unwrap();
        // SAFETY: a valid C string.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert_eq!(holding(&home, b"the-secret"), None);
        std::fs::write(home.join("a/b/notes"), "... the-secret ...").unwrap();
        assert_eq!(holding(&home, b"the-secret"), Some(home.join("a/b/notes")));
        assert_eq!(holding(&home, b""), None);
    }
}
