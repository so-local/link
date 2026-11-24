//! Background services, as `lnk`'s modules run them: a launchd
//! LaunchAgent on macOS, a systemd user service on native Linux. Either way it
//! survives closing the terminal, restarts if it dies, and starts at
//! login. A module names its own (`link.<...>` labels, `lnk-<...>.service`
//! units) and keeps what it runs; this is the part every one shares.
//! An explicitly configured Linux guest uses the same API with a durable
//! process guardian, without native login startup or a systemd dependency.

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, bail};

use link_format::{Note, Report, aside, cmd, sentence};

mod states;
pub use states::{Status, running, running_each, status, status_each, uptime};
#[cfg(all(test, target_os = "linux"))]
mod native_tests;
#[cfg(all(test, target_os = "linux"))]
mod query_tests;

/// What a service runs.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub workdir: PathBuf,
    pub log: PathBuf,
}

/// Whether launchd or systemd is asked anything, and a model runtime
/// started: not with `LNK_SERVICES=off`, which tests set. A service's
/// name doesn't depend on `HOME`, so a test's agent would stop or
/// replace yours.
/// `systemd-run`'s `--setenv` for each variable of this process a unit of
/// Link's started with it is handed: `HOME`, `PATH`, `LANG`,
/// `XDG_CONFIG_HOME` and Link's own (`LNK_*`), but for one naming a
/// secret (a token, key, password). By name only, so systemd-run takes
/// each value from its own environment and none is on its command line
/// for `ps` to show.
pub fn setenv_args() -> Vec<String> {
    std::env::vars_os()
        .filter_map(|(key, _)| key.into_string().ok())
        .filter(|key| handed_on(key))
        .map(|key| format!("--setenv={key}"))
        .collect()
}

/// Whether variable `key` is handed to a unit ([`setenv_args`]).
fn handed_on(key: &str) -> bool {
    let secret = ["TOKEN", "SECRET", "KEY", "PASS"]
        .iter()
        .any(|word| key.contains(word));
    matches!(key, "HOME" | "PATH" | "LANG" | "XDG_CONFIG_HOME")
        || (key.starts_with("LNK_") && !secret)
}

pub fn managed() -> bool {
    std::env::var("LNK_SERVICES").map_or(true, |v| v != "off" && v != "0")
}

/// Installs the service `label` (launchd) or `unit_name` (systemd) for
/// `job` and (re)starts it.
pub fn start(label: &str, unit_name: &str, job: &Job) -> anyhow::Result<()> {
    start_all(&[(label, unit_name, job)])
}

/// [`start`] for several services at once: on Linux, one reload of
/// systemd for all of them.
pub fn start_all(services: &[(&str, &str, &Job)]) -> anyhow::Result<()> {
    for (_, _, job) in services {
        check(job)?;
        open_log(&job.log)?;
    }
    #[cfg(target_os = "linux")]
    if let Some(driver) = crate::guest::selected()? {
        for (_, unit_name, job) in services {
            driver.validate(unit_name, job)?;
            write_file(&unit_path_of(unit_name), &unit(job))?;
            driver.start(unit_name, job, managed())?;
        }
        return Ok(());
    }
    if cfg!(target_os = "macos") {
        for (label, _, job) in services {
            start_launchd(label, job)?;
        }
        return Ok(());
    }
    start_systemd(services)
}

fn start_systemd(services: &[(&str, &str, &Job)]) -> anyhow::Result<()> {
    let units: Vec<&str> = services.iter().map(|(_, unit, _)| *unit).collect();
    let files: Vec<_> = units.iter().map(|name| unit_path_of(name)).collect();
    for ((_, _, job), file) in services.iter().zip(&files) {
        write_file(file, &unit(job))?;
    }
    if units.is_empty() || !managed() {
        return Ok(());
    }
    systemctl_args(
        [OsStr::new("enable"), OsStr::new("--no-reload")]
            .into_iter()
            .chain(files.iter().map(|file| file.as_os_str())),
    )?;
    systemctl(&["daemon-reload"])?;
    systemctl(&[&["restart"], &units[..]].concat()).context("starting the service with systemd")?;
    Ok(())
}

/// Loads `label` for `job` into launchd, in place of what it had.
#[expect(
    clippy::disallowed_methods,
    reason = "launchd refuses a bootstrap for a moment after an unload: a pause, then once more"
)]
fn start_launchd(label: &str, job: &Job) -> anyhow::Result<()> {
    let path = plist_path_of(label);
    unload(label);
    write_file(&path, &plist(label, job))?;
    if !managed() {
        return Ok(());
    }
    let path = path.display().to_string();
    run(link_secretive::process::command("plutil").args(["-lint", &path]))
        .context("the service file lnk wrote is invalid")?;
    // A job that was just unloaded can still refuse a bootstrap for a
    // moment ("5: Input/output error"): once more after a pause.
    launchctl(&["bootstrap", &domain(), &path])
        .or_else(|_| {
            unload(label);
            std::thread::sleep(std::time::Duration::from_secs(2));
            launchctl(&["bootstrap", &domain(), &path])
        })
        .context("starting the service with launchd")
}

/// systemd stops a user's services when their last session ends, unless
/// lingering is on for the user: on a server reached over SSH, `what`
/// would stop at logout. Turns it on (systemd lets users do that for
/// themselves on most systems), else says how. Nothing on macOS.
pub fn keep_after_logout(what: &str) {
    #[cfg(target_os = "linux")]
    if !matches!(crate::guest::selected(), Ok(None)) {
        return;
    }
    if cfg!(target_os = "macos") || !managed() {
        return;
    }
    let uid = uid();
    let linger = |uid: &str| {
        link_secretive::process::command("loginctl")
            .args(["show-user", uid, "--property=Linger", "--value"])
            .stderr(std::process::Stdio::null())
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "yes")
    };
    match linger(&uid) {
        Err(_) | Ok(true) => return, // no logind to ask, or already on
        Ok(false) => {}
    }
    let enabled = link_secretive::process::command("loginctl")
        .arg("enable-linger")
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if enabled && linger(&uid).unwrap_or(false) {
        aside(Note(sentence!(
            "{} keeps running when you log out (lingering is on for your user).",
            what
        )));
    } else {
        let user = std::env::var("USER").unwrap_or_else(|_| uid.clone());
        aside(Report::warning(sentence!(
            "{} stops when you log out of this machine; {} keeps it running.",
            what,
            cmd!("sudo loginctl enable-linger {}", user)
        )));
    }
}

/// Refuses a job whose unit or plist could say more than it means: a
/// control character (a newline) in a systemd value starts a new
/// directive.
pub(crate) fn check(job: &Job) -> anyhow::Result<()> {
    let log = job.log.display().to_string();
    let workdir = job.workdir.display().to_string();
    let values = job
        .argv
        .iter()
        .chain(job.env.keys())
        .chain(job.env.values())
        .chain([&log, &workdir]);
    for value in values {
        if value.chars().any(char::is_control) {
            bail!("refusing to run {value:?}: it has a control character");
        }
    }
    Ok(())
}

/// Opens the log for a new start: a regular file, only the user's, in a
/// folder only the user's, which launchd or systemd (outside the sandbox)
/// then append the harness's output to, or a start in a terminal does
/// itself. Records where this start's lines begin (`lnk agent log show`).
pub fn open_log(path: &Path) -> anyhow::Result<std::fs::File> {
    if let Some(dir) = path.parent() {
        link_files::create_private_dir(dir)
            .with_context(|| format!("creating {}", dir.display()))?;
    }
    match path.symlink_metadata() {
        Ok(meta) if !meta.file_type().is_file() => {
            bail!("{} is not a regular file; remove it", path.display())
        }
        _ => {}
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    // Never through a link, even one made since the check above: the file
    // itself, whose permissions are then set through the handle.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let log = options
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    #[cfg(unix)]
    log.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    let start = log.metadata()?.len();
    link_files::write_private(&start_path(path), start.to_string().as_bytes())?;
    Ok(log)
}

/// Where the byte offset of the last start's first line is kept, next to
/// the log (the harness can't write either).
pub fn start_path(log: &Path) -> PathBuf {
    log.with_extension("start")
}

/// Stops the service and removes it, so it doesn't start at login.
pub fn stop(label: &str, unit_name: &str) -> anyhow::Result<()> {
    stop_before_start(label, unit_name)?;
    reload();
    Ok(())
}

/// [`stop`], from the service's own process, by its file (a plist or a
/// unit, named for it): the file removed, so neither a restart nor the
/// next login runs it, and its stop not waited for, since it ends this
/// process. Best effort.
pub fn stop_from_within(file: &Path) {
    let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    #[cfg(target_os = "linux")]
    match crate::guest::selected() {
        Ok(Some(driver)) => {
            if managed() {
                let _ = crate::guest::stop(&driver, name, false);
            }
            let _ = remove(file);
            return;
        }
        Err(_) => return,
        Ok(None) => {}
    }
    if cfg!(target_os = "macos") {
        let _ = remove(file);
        let _ = launchctl(&["bootout", &target_of(name.trim_end_matches(".plist"))]);
    } else {
        let _ = systemctl(&["disable", "--no-reload", name]);
        let _ = remove(file);
        let _ = systemctl(&["stop", "--no-block", name]);
    }
}

/// Has systemd read the units again, after [`stop_before_start`]s with no
/// start to follow; nothing to do with launchd.
pub fn reload() {
    #[cfg(target_os = "linux")]
    if !matches!(crate::guest::selected(), Ok(None)) {
        return;
    }
    if !cfg!(target_os = "macos") {
        let _ = systemctl(&["daemon-reload"]);
    }
}

/// Has systemd forget the units `unit_names`, once [`stop`]ped for good
/// (their owner deleted or moved away): a unit that failed before stays
/// on systemd's list as "not-found failed" after its file goes, until
/// it's reset. Nothing to forget with launchd, or for a guest's driver,
/// which keeps no such list. Best effort: a unit systemd never failed
/// isn't on it.
pub fn forget(unit_names: &[&str]) {
    #[cfg(target_os = "linux")]
    if !matches!(crate::guest::selected(), Ok(None)) {
        return;
    }
    if !cfg!(target_os = "macos") {
        // One at a time: a unit systemd doesn't know refuses the call.
        for unit in unit_names {
            let _ = systemctl(&["reset-failed", unit]);
        }
    }
}

/// [`stop`], leaving systemd's reload to the [`start`] or [`start_all`]
/// that follows (each reload reloads every unit).
pub fn stop_before_start(label: &str, unit_name: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    if let Some(driver) = crate::guest::selected()? {
        if managed() {
            crate::guest::stop(&driver, unit_name, true)?;
        }
        return remove(&unit_path_of(unit_name));
    }
    if cfg!(target_os = "macos") {
        unload(label);
        remove(&plist_path_of(label))
    } else {
        let _ = systemctl(&["disable", "--no-reload", "--now", unit_name]);
        remove(&unit_path_of(unit_name))
    }
}

/// Stops a background service by its launchd label or systemd unit
/// (another program's, before its file is removed). Best effort.
pub fn unload_other(service: &str) {
    #[cfg(target_os = "linux")]
    match crate::guest::selected() {
        Ok(Some(driver)) => {
            if managed() {
                let _ = crate::guest::pause(&driver, service);
            }
            return;
        }
        Err(_) => return,
        Ok(None) => {}
    }
    if cfg!(target_os = "macos") {
        let _ = launchctl(&["bootout", &format!("{}/{service}", domain())]);
    } else {
        let _ = systemctl(&["disable", "--now", service]);
    }
}

/// Starts again a service of another program's whose file is back at
/// `path`, as [`unload_other`] stopped it. Best effort.
pub fn load_other(path: &Path) {
    #[cfg(target_os = "linux")]
    match crate::guest::selected() {
        Ok(Some(driver)) => {
            if managed()
                && let Some(unit) = path.file_name().and_then(|n| n.to_str())
            {
                let _ = crate::guest::resume(&driver, unit);
            }
            return;
        }
        Err(_) => return,
        Ok(None) => {}
    }
    if cfg!(target_os = "macos") {
        let _ = launchctl(&["bootstrap", &domain(), &path.display().to_string()]);
    } else if let Some(unit) = path.file_name().and_then(|n| n.to_str()) {
        let _ = systemctl(&["enable", "--now", unit]);
    }
}

/// Stops the service `label` (launchd) or `unit_name` (systemd) for now,
/// its file kept, so [`resume`] starts it again as it was: an agent
/// stopped for being idle.
pub fn pause(label: &str, unit_name: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    if let Some(driver) = crate::guest::selected()? {
        return if managed() {
            crate::guest::pause(&driver, unit_name)
        } else {
            Ok(())
        };
    }
    match cfg!(target_os = "macos") {
        true => {
            unload(label);
            Ok(())
        }
        false => systemctl(&["stop", unit_name]),
    }
}

/// Starts again a service [`pause`] stopped, from its file.
pub fn resume(label: &str, unit_name: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    if let Some(driver) = crate::guest::selected()? {
        return if managed() {
            crate::guest::resume(&driver, unit_name)
        } else {
            Ok(())
        };
    }
    match cfg!(target_os = "macos") {
        true => {
            let path = plist_path_of(label).display().to_string();
            launchctl(&["bootstrap", &domain(), &path])
                .or_else(|_| launchctl(&["kickstart", &target_of(label)]))
                .context("starting the service with launchd")
        }
        false => systemctl(&["start", unit_name]),
    }
}

/// Unloads the job, waiting (up to 10 s) until launchd has let it go:
/// `bootout` returns before the job is gone.
#[expect(
    clippy::disallowed_methods,
    reason = "launchd says nothing when an unload is done: its state is polled"
)]
fn unload(label: &str) {
    if !managed() || launchctl(&["bootout", &target_of(label)]).is_err() {
        return; // not loaded
    }
    for _ in 0..50 {
        let loaded = link_secretive::process::command("launchctl")
            .args(["print", &target_of(label)])
            .output()
            .is_ok_and(|o| o.status.success());
        if !loaded {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

pub fn domain() -> String {
    format!("gui/{}", uid())
}

fn target_of(label: &str) -> String {
    format!("{}/{label}", domain())
}

fn uid() -> String {
    user_id().unwrap_or_else(|| "501".into())
}

/// This user's id (`id -u`), asked once per process.
pub fn user_id() -> Option<String> {
    static UID: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    UID.get_or_init(|| {
        link_secretive::process::command("id")
            .arg("-u")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|uid| !uid.is_empty())
    })
    .clone()
}

pub fn plist_path_of(label: &str) -> PathBuf {
    link_files::home_dir().join(format!("Library/LaunchAgents/{label}.plist"))
}

pub fn unit_path_of(unit_name: &str) -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| link_files::home_dir().join(".config"))
        .join("systemd/user")
        .join(unit_name)
}

/// `launchctl args`; nothing with [`managed`] off.
pub fn launchctl(args: &[&str]) -> anyhow::Result<()> {
    if !managed() {
        return Ok(());
    }
    run(link_secretive::process::command("launchctl").args(args))
}

/// `systemctl --user args`; nothing with [`managed`] off. From a
/// service's environment, which may lack it, the user's bus is in
/// `/run/user/<uid>`.
pub fn systemctl(args: &[&str]) -> anyhow::Result<()> {
    systemctl_args(args)
}

fn systemctl_args(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> anyhow::Result<()> {
    if !managed() {
        return Ok(());
    }
    run(systemctl_command().args(args)).map_err(no_user_systemd)
}

/// Every manager operation, including status queries, uses the same user bus.
fn systemctl_command() -> Command {
    let mut command = link_secretive::process::command("systemctl");
    if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
        command.env("XDG_RUNTIME_DIR", format!("/run/user/{}", uid()));
    }
    command.arg("--user");
    command
}

/// systemd's error when this user has no systemd of their own (over SSH,
/// in WSL or a container), as what to do about it.
fn no_user_systemd(err: anyhow::Error) -> anyhow::Error {
    match format!("{err:#}").contains("Failed to connect to bus") {
        true => anyhow::anyhow!(
            "no systemd for your user here (SSH, WSL, a container): sudo loginctl \
             enable-linger $USER and log in again, or run it in this terminal instead \
             (--foreground, where the command has it)"
        ),
        false => err,
    }
}

fn run(command: &mut Command) -> anyhow::Result<()> {
    let output = command
        .output()
        .with_context(|| format!("running {command:?}"))?;
    if !output.status.success() {
        bail!(
            "{command:?} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// A unit or plist, only yours (600): its environment may carry what
/// its program's URLs are proved by, for no other user to read.
fn write_file(path: &Path, text: &str) -> anyhow::Result<()> {
    link_files::write_private(path, text.as_bytes())
        .with_context(|| format!("writing {}", path.display()))
}

fn remove(path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            Err(err).with_context(|| format!("removing {}", path.display()))
        }
        _ => Ok(()),
    }
}

/// The LaunchAgent `label` for `job`.
pub fn plist(label: &str, job: &Job) -> String {
    plist_for(label, job, None, 10)
}

pub(crate) fn plist_for(
    label: &str,
    job: &Job,
    user: Option<&str>,
    restart_seconds: u64,
) -> String {
    let string = |s: &str| format!("<string>{}</string>", xml(s));
    let args: String = job
        .argv
        .iter()
        .map(|a| format!("\n\t\t{}", string(a)))
        .collect();
    let env: String = job
        .env
        .iter()
        .map(|(k, v)| format!("\n\t\t<key>{}</key>\n\t\t{}", xml(k), string(v)))
        .collect();
    let log = job.log.display().to_string();
    let authority = user.map(|user| format!("\t<key>UserName</key>\n\t<string>{}</string>\n\t<key>LimitLoadToSessionType</key>\n\t<string>System</string>\n", xml(user))).unwrap_or_default();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<!-- Written by lnk; the lnk command that stops it removes it. -->
<plist version="1.0">
<dict>
{authority}
	<key>Label</key>
	<string>{label}</string>
	<key>ProgramArguments</key>
	<array>{args}
	</array>
	<key>EnvironmentVariables</key>
	<dict>{env}
	</dict>
	<key>WorkingDirectory</key>
	{workdir}
	<key>StandardOutPath</key>
	{log}
	<key>StandardErrorPath</key>
	{log}
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<true/>
	<key>ThrottleInterval</key>
	<integer>{restart_seconds}</integer>
	<key>ProcessType</key>
	<string>Background</string>
</dict>
</plist>
"#,
        workdir = string(&job.workdir.display().to_string()),
        log = string(&log),
    )
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The systemd user unit for `job`.
pub fn unit(job: &Job) -> String {
    unit_for(job, None, 10)
}

pub(crate) fn unit_for(job: &Job, user: Option<&str>, restart_seconds: u64) -> String {
    let exec: Vec<String> = job.argv.iter().map(|a| systemd_quote(a)).collect();
    let env: String = job
        .env
        .iter()
        .map(|(k, v)| format!("Environment={}\n", env_quote(&format!("{k}={v}"))))
        .collect();
    let log = specifiers(&job.log.display().to_string());
    let (ordering, authority, target) = match user {
        Some(user) => (
            "After=network-online.target\nWants=network-online.target\n",
            format!("User={}\n", env_quote(user)),
            "multi-user.target",
        ),
        None => ("", String::new(), "default.target"),
    };
    format!(
        "# Written by lnk; the lnk command that stops it removes it.\n\
         [Unit]\n\
         Description=Link: a background service of lnk\n\
         {ordering}\
         \n\
         [Service]\n\
         {authority}\
         ExecStart={exec}\n\
         {env}\
         WorkingDirectory={workdir}\n\
         StandardOutput=append:{log}\n\
         StandardError=append:{log}\n\
         Restart=always\n\
         RestartSec={restart_seconds}\n\
         \n\
         [Install]\n\
         WantedBy={target}\n",
        exec = exec.join(" "),
        workdir = specifiers(&job.workdir.display().to_string()),
    )
}

/// An argument of `ExecStart=`, where systemd expands `$` and `%`.
fn systemd_quote(text: &str) -> String {
    let escaped = specifiers(&text.replace('\\', "\\\\").replace('"', "\\\"")).replace('$', "$$");
    format!("\"{escaped}\"")
}

/// An `Environment=` assignment: quoted, `%` escaped, and `$` as it is,
/// since systemd expands no variables there (`$$` would stay doubled).
fn env_quote(text: &str) -> String {
    let escaped = specifiers(&text.replace('\\', "\\\\").replace('"', "\\\""));
    format!("\"{escaped}\"")
}

/// A path setting (`WorkingDirectory=`, `append:`): never quoted, which
/// systemd refuses there, with only its specifiers (`%`) escaped.
fn specifiers(text: &str) -> String {
    text.replace('%', "%%")
}

#[cfg(test)]
mod tests {
    /// A unit's file is only yours: its environment is no other user's
    /// to read.
    #[cfg(unix)]
    #[test]
    fn a_unit_file_is_only_yours() {
        use std::os::unix::fs::PermissionsExt;
        let dir = link_test_kit::TempDir::new("unit");
        let path = dir.path().join("user/lnk-agent-main.service");
        super::write_file(&path, "[Service]\nEnvironment=LNK_URL=x\n").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    /// A unit is handed Link's variables but none naming a secret.
    #[test]
    fn a_unit_is_handed_no_secret() {
        for key in ["HOME", "PATH", "LNK_SERVICES", "LNK_CONFIG_DIR"] {
            assert!(super::handed_on(key), "{key}");
        }
        for key in [
            "LNK_TOKEN",
            "LNK_TELEGRAM_TOKEN",
            "LNK_SECRET",
            "LNK_API_KEY",
            "LNK_PASSWORD",
            "AWS_SECRET_ACCESS_KEY",
            "SSH_AUTH_SOCK",
        ] {
            assert!(!super::handed_on(key), "{key}");
        }
        assert!(
            super::setenv_args()
                .iter()
                .all(|a| !a.contains('=') || a.matches('=').count() == 1)
        );
    }

    /// No systemd of the user's own is said with what to do; any other
    /// error is passed on as it is.
    #[test]
    fn no_user_systemd_says_what_to_do() {
        let bus =
            anyhow::anyhow!("systemctl --user daemon-reload failed: Failed to connect to bus");
        let said = super::no_user_systemd(bus).to_string();
        assert!(
            said.contains("enable-linger") && said.contains("--foreground"),
            "{said}"
        );
        let other = super::no_user_systemd(anyhow::anyhow!("Unit not found")).to_string();
        assert_eq!(other, "Unit not found");
    }

    use super::*;

    fn job() -> Job {
        Job {
            argv: vec!["/usr/bin/sandbox-exec".into(), "a&b".into()],
            env: BTreeMap::from([("HOME".into(), "/h".into())]),
            workdir: "/Users/me/Link".into(),
            log: "/tmp/h.log".into(),
        }
    }

    #[test]
    fn the_plist_runs_the_job_and_keeps_it_alive() {
        let plist = plist("link.agent.main", &job());
        assert!(plist.contains("<key>Label</key>\n\t<string>link.agent.main</string>"));
        assert!(plist.contains("<string>a&amp;b</string>"));
        assert!(plist.contains("<key>HOME</key>\n\t\t<string>/h</string>"));
        assert!(plist.contains("<key>KeepAlive</key>\n\t<true/>"));
        assert!(plist.contains("<key>RunAtLoad</key>\n\t<true/>"));
    }

    #[test]
    fn control_characters_are_refused() {
        let mut bad = job();
        bad.argv.push("x\nExecStartPre=/bin/evil".into());
        assert!(check(&bad).is_err());
        let mut bad = job();
        bad.env.insert("A".into(), "1\r".into());
        assert!(check(&bad).is_err());
        assert!(check(&job()).is_ok());
    }

    #[test]
    fn the_log_is_a_private_regular_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = link_test_kit::TempDir::new("log");
        let log = dir.join("logs/h.log");
        open_log(&log).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&log), 0o600);
        assert_eq!(mode(&dir.join("logs")), 0o700);
        assert_eq!(std::fs::read_to_string(start_path(&log)).unwrap(), "0");
        std::fs::write(&log, "old\n").unwrap();
        open_log(&log).unwrap();
        assert_eq!(std::fs::read_to_string(start_path(&log)).unwrap(), "4");
        // A symlink in its place is refused, not followed.
        std::fs::remove_file(&log).unwrap();
        std::os::unix::fs::symlink(dir.join("target"), &log).unwrap();
        assert!(open_log(&log).is_err());
        assert!(!dir.join("target").exists());
        // So is one where the start is kept: replaced, never written through.
        std::fs::remove_file(&log).unwrap();
        std::fs::write(dir.join("victim"), "mine").unwrap();
        std::fs::remove_file(start_path(&log)).unwrap();
        std::os::unix::fs::symlink(dir.join("victim"), start_path(&log)).unwrap();
        open_log(&log).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("victim")).unwrap(), "mine");
        assert!(!start_path(&log).symlink_metadata().unwrap().is_symlink());
    }

    #[test]
    fn the_unit_quotes_what_systemd_would_expand() {
        let mut job = job();
        job.argv.push("100%$x".into());
        let unit = unit(&job);
        assert!(unit.contains(r#"ExecStart="/usr/bin/sandbox-exec" "a&b" "100%%$$x""#));
        assert!(unit.contains(r#"Environment="HOME=/h""#));
        assert!(unit.contains("Restart=always"));
    }

    #[test]
    fn the_unit_writes_paths_and_variables_as_systemd_reads_them() {
        // As systemd 255 read them: a quoted working folder is refused
        // ("not absolute"), and `$$` stays doubled in a variable.
        let mut job = job();
        job.workdir = "/home/me/My Link 100%".into();
        job.log = "/home/me/logs/50%.log".into();
        job.env.insert("KEY".into(), "a$b%c \"q\"".into());
        let unit = unit(&job);
        assert!(
            unit.contains("\nWorkingDirectory=/home/me/My Link 100%%\n"),
            "{unit}"
        );
        assert!(
            unit.contains("StandardOutput=append:/home/me/logs/50%%.log\n"),
            "{unit}"
        );
        assert!(unit.contains(r#"Environment="KEY=a$b%%c \"q\"""#), "{unit}");
    }
}
