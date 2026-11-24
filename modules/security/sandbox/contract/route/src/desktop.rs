//! Your desktop session, as what runs outside a sandbox (its proxy, a
//! route's bridge) reaches you: a notification, and the variables a
//! dialog or your browser needs.

/// What a dialog, a notification or your browser needs of your desktop
/// session (Linux): a proxy runs with its program's environment, and a
/// bridge with one of its own, neither carrying them (a harness's HOME is
/// its own folder), so they're handed: `sandbox proxy --desktop`, and a
/// route's given.
pub const DESKTOP_VARS: &[&str] = &[
    "HOME",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
];

/// Where the proxy finds the programs it runs outside the sandbox (a
/// dialog, a notification, your browser): the system's folders only.
/// Never its own `PATH`, which is its program's and may name a folder the
/// sandbox writes (a harness's shims, `.`), where a program of the same
/// name would run outside.
pub const SYSTEM_PATH: &str = "/usr/bin:/bin:/usr/local/bin";

static DESKTOP: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();

/// Sets the desktop session's variables for this process's dialogs and
/// notifications (only [`DESKTOP_VARS`]).
pub fn set_desktop(pairs: Vec<(String, String)>) {
    let pairs = pairs
        .into_iter()
        .filter(|(k, _)| DESKTOP_VARS.contains(&k.as_str()))
        .collect();
    let _ = DESKTOP.set(pairs);
}

/// The desktop session's variables: those set, or this process's own
/// (only [`DESKTOP_VARS`], all of them [`only_desktop`]).
pub fn desktop() -> Vec<(String, String)> {
    match DESKTOP.get() {
        Some(pairs) => pairs.clone(),
        None => DESKTOP_VARS
            .iter()
            .filter_map(|k| Some((k.to_string(), std::env::var(k).ok()?)))
            .collect(),
    }
}

/// Tells the person at this computer, best effort: a notification where
/// there's a desktop to show it (macOS; Linux with `notify-send`).
pub fn notify(title: &str, text: &str) {
    notify_on(&desktop(), title, text)
}

/// [`notify`], on the desktop session of `desktop`'s variables.
pub fn notify_on(desktop: &[(String, String)], title: &str, text: &str) {
    if cfg!(test) {
        return;
    }
    let clean = |t: &str| link_format::clean(t).replace(['"', '\\'], "'");
    let (title, text) = (clean(title), clean(text));
    let mut command = if cfg!(target_os = "macos") {
        let mut command = link_secretive::process::command("/usr/bin/osascript");
        command.args([
            "-e",
            &format!("display notification \"{text}\" with title \"{title}\""),
        ]);
        command
    } else {
        let mut command = link_secretive::process::command("notify-send");
        command
            .env("PATH", SYSTEM_PATH)
            .envs(desktop.iter().cloned())
            .args([title.as_str(), text.as_str()]);
        command
    };
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // Waited for on a thread of its own: never the proxy's.
    std::thread::spawn(move || {
        let _ = command.status();
    });
}

/// Of `pairs`, only the desktop session's variables ([`DESKTOP_VARS`]).
pub fn only_desktop(pairs: &[(String, String)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .filter(|(k, _)| DESKTOP_VARS.contains(&k.as_str()))
        .cloned()
        .collect()
}
