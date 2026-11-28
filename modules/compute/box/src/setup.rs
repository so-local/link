//! What a new box runs: its first-boot setup (cloud-init, as root), then
//! Link's install over SSH (as `ubuntu`, so you see it).

use anyhow::ensure;
use link_cloud_contract::Arch;
use link_core::release;
use link_format::WithinExt;
use link_machine::installation::Installation;
use link_machine::machine::Kind;
use link_version::Version;

/// The box's first boot (cloud-init): lingering for `ubuntu`, so its
/// services outlive SSH sessions, its SSH host keys on the console, and
/// `/etc/lnk/kind.toml` saying it's a box of `kind`'s OS and
/// processor, root's (644, its folder 755), written as root before
/// anything of the user's runs, so nothing but root changes it, and
/// apt's retries in its configuration ([`apt_settings`]). With
/// `trust`, its sshd trusts that authority's certificates naming that
/// principal, and Link's plain keys (an image's already there) go from
/// `authorized_keys`. What runs on the box later installs what it needs
/// (`lnk agent move` adds the harness and its sandbox). Nothing secret: a
/// cloud's metadata can be read by programs on the machine.
pub fn cloud_init(kind: Kind, trust: Option<&Trust>) -> anyhow::Result<String> {
    ensure!(kind.is_box(), "a box's first boot says it's a box");
    let machine = toml::to_string(&kind)?;
    let content: String = machine.lines().map(|l| format!("      {l}\n")).collect();
    let init = format!(
        "#cloud-config\n\
         # Written by lnk box create.\n\
         # Its SSH host keys on the console, for lnk box create to pin.\n\
         ssh:\n\
         \x20 emit_keys_to_console: true\n\
         # What this machine is, root's: lnk reads it.\n\
         write_files:\n\
         \x20 - path: {path}\n\
         \x20   owner: root:root\n\
         \x20   permissions: '0644'\n\
         \x20   content: |\n\
         {content}\
         {apt}\
         runcmd:\n\
         \x20 - [chmod, '0755', {settings}]\n\
         \x20 - [loginctl, enable-linger, ubuntu]\n",
        path = link_files::layout::System::MachineKind.path().display(),
        settings = link_files::layout::SystemRoot::Settings.path().display(),
        apt = apt_settings()?,
    );
    let Some(trust) = trust else {
        return Ok(init);
    };
    let drop_keys: String = crate::ssh::certs::DROP_KEYS
        .lines()
        .map(|l| format!("    {l}\n"))
        .collect();
    Ok(link_cloud_contract::first_boot::with(
        &init,
        &crate::ssh::certs::trust_files(&trust.authority, &trust.principal),
        &format!("  - |\n{drop_keys}  - [systemctl, try-reload-or-restart, ssh]\n"),
    ))
}

/// apt's retries and timeout ([`link_machine::apt`]) as a file of its
/// configuration under `write_files:`, so every apt on the box, Link's
/// and its person's, fetches a dropped download again.
fn apt_settings() -> anyhow::Result<String> {
    let apt = link_machine::apt::apt()?;
    let lines = apt.file_lines();
    Ok(link_cloud_contract::first_boot::file(
        apt.file(),
        &lines.iter().map(String::as_str).collect::<Vec<_>>(),
    ))
}

/// What a box's sshd trusts: this computer's certificate authority (its
/// public half), for certificates naming the box's principal.
#[derive(Clone, Debug, PartialEq)]
pub struct Trust {
    pub authority: String,
    pub principal: String,
}

/// The public modules installed alongside `lnk` on a box.
pub fn modules() -> Vec<&'static str> {
    vec!["secret", "sandbox"]
}

/// The releases a box installs `lnk` from: this machine's
/// (`link_core::release::Source::here`, Link's own unless
/// `LNK_RELEASES_URL` names others or this `lnk` runs from a bundle, and
/// `LNK_VERSION`). What they serve is
/// trusted only as this machine's `lnk` trusts it: the `install.sh` a box
/// runs is checked here against the keys this `lnk` was built with, and
/// sent to the box as it was checked; the box fetches no script of its
/// own. Every `lnk` run on a box (`lnk box run`, as a move's upgrade is)
/// is pointed at releases this machine is pointed at, so a box follows
/// its machine at the same address. When they're where only this machine
/// reads them (a bucket of yours, a folder here, a bundle), the box is
/// given a copy of them ([`Staged`]) and follows that copy for good
/// ([`for_box`]), whatever this machine follows by then.
#[derive(Clone, Debug, PartialEq)]
pub struct Releases {
    pub source: release::Source,
    /// `LNK_VERSION`, when given.
    pub version: Option<Version>,
    /// Whether `LNK_RELEASES_URL` or this `lnk`'s bundle named them,
    /// rather than Link's own.
    pub followed: bool,
    /// The keys whose signature on a release is trusted: this `lnk`'s
    /// (`release::trusted_signers`).
    pub signers: String,
}

/// Where on a box, under its home, the releases it's given are
/// ([`Staged`]), each at `download/v<version>` as a release's address has it.
pub fn staged() -> String {
    link_files::layout::Hidden::Releases
        .relative_to_home()
        .display()
        .to_string()
}

/// `LNK_RELEASES_URL=...`, for an `lnk` run on a box: the copy staged on
/// it when it follows one ([`crate::state::Machine::staged`]), else the
/// releases this machine follows now, so an upgrade there (a move's)
/// installs from where this one did.
pub fn for_box(staged: bool) -> anyhow::Result<String> {
    if staged {
        Ok(staged_env())
    } else {
        Ok(Releases::here()?.env())
    }
}

/// `LNK_RELEASES_URL=<the box's copy> `, for a command on a box that
/// follows the releases staged on it ([`staged`]).
pub fn staged_env() -> String {
    format!("{}=\"file://$HOME/{}\" ", release::RELEASES_VAR, staged())
}

impl Releases {
    /// This machine's.
    pub fn here() -> anyhow::Result<Releases> {
        let floor = link_core::RELEASED.then(link_version::version);
        let version = asked(std::env::var("LNK_VERSION").ok(), floor.as_ref())?;
        let followed = release::Source::followed()?;
        Ok(Releases {
            followed: followed.is_some(),
            source: match followed {
                Some(source) => source,
                None => release::Source::here()?,
            },
            version,
            signers: release::trusted_signers(),
        })
    }

    /// `LNK_RELEASES_URL=<where> `, for a command on the box, when this
    /// machine was pointed at releases of its own: their address, or the
    /// box's copy of them ([`staged`]) when the box can't read them where
    /// they are. Else nothing.
    pub fn env(&self) -> String {
        match (self.followed, self.source.shared()) {
            (false, _) => String::new(),
            (true, true) => format!(
                "{}={} ",
                release::RELEASES_VAR,
                quote(&self.source.to_string())
            ),
            (true, false) => staged_env(),
        }
    }

    /// Whether the box is given a copy of them ([`staged`]), which it then
    /// follows for good: this machine was pointed at releases only it
    /// reads.
    pub fn follows_staged(&self) -> bool {
        self.followed && !self.source.shared()
    }

    /// The version a box installs: a spec's, else `LNK_VERSION`'s, else
    /// none (the latest).
    pub fn version(&self, spec: Option<&Version>) -> Option<Version> {
        spec.or(self.version.as_ref()).cloned()
    }

    /// Their `install.sh` of `version` (else the latest), checked here:
    /// its release's `SHA256SUMS` signed by one of `signers` (a release
    /// `lnk`'s trusts Link's signer alone), the script's checksum the
    /// signed one, and the release one `release::check_installable` takes,
    /// the latest at or above `floor`.
    pub fn install(
        &self,
        version: Option<&Version>,
        floor: Option<&Version>,
    ) -> anyhow::Result<Checked> {
        let base = release::base(&self.source, version)?;
        let checked = runtime()?.block_on(async {
            let found = release::Release::open(&base, &self.signers).await?;
            let version = release::check_installable(found.version().await?, version, floor)?;
            anyhow::Ok((version, found.get("install.sh").await?))
        });
        let (version, script) = checked.within(format!(
            "the install.sh of {} isn't one this lnk trusts, so no box installs from there",
            link_format::clean(&base.to_string())
        ))?;
        let script = String::from_utf8(script).within("its install.sh isn't text")?;
        Ok(Checked { version, script })
    }

    /// What a box of `arch` installs of `checked`'s release, copied here
    /// for it, when the box can't read the releases where they are (a
    /// bucket read with this machine's login, a folder here): the signed
    /// `SHA256SUMS` and its signature, `VERSION`, `install.sh` and every
    /// archive for its processor, `lnk`'s and each module's, each checked
    /// against the signed sums as it's copied, so the box is given only
    /// what this `lnk` trusts, and checks it again as it installs. Nothing
    /// when the box reads them itself, at their address.
    pub fn stage(&self, checked: &Checked, arch: Arch) -> anyhow::Result<Staged> {
        let version = &checked.version;
        let folder = format!("~/{}/download/v{version}", staged());
        if self.source.shared() {
            return Ok(Staged {
                folder,
                files: Vec::new(),
                _dir: None,
            });
        }
        let base = release::base(&self.source, Some(version))?;
        let target = match arch {
            Arch::X8664 => "x86_64-unknown-linux-musl",
            Arch::Arm64 => "aarch64-unknown-linux-musl",
        };
        let dir = StagedDir(
            link_files::private_temp_dir("box-release").within("making a folder for it")?,
        );
        let files = runtime()?.block_on(async {
            let found = release::Release::open(&base, &self.signers).await?;
            let assets = staged_assets(found.assets(), target);
            ensure!(
                assets.iter().any(|a| a.ends_with(".tar.gz")),
                "lnk {version} has no build for {target} there"
            );
            found.save(&dir.0, &assets).await
        });
        let files = files.within(format!(
            "copying lnk {version} from {} for the box",
            link_format::clean(&base.to_string())
        ))?;
        Ok(Staged {
            folder,
            files,
            _dir: Some(dir),
        })
    }
}

/// Of a release's `assets`, those a box of `target` is given: `VERSION`,
/// `install.sh` and each program's archive for it.
fn staged_assets<'a>(assets: impl Iterator<Item = &'a str>, target: &str) -> Vec<&'a str> {
    let suffix = format!("-{target}.tar.gz");
    assets
        .filter(|a| {
            matches!(*a, "VERSION" | "install.sh")
                || (a.starts_with("lnk-") && a.ends_with(&suffix))
        })
        .collect()
}

/// A release copied here for a box ([`Releases::stage`]), in a private
/// folder of its own that goes when this does; nothing, for a box that
/// reads its releases itself.
#[derive(Debug)]
pub struct Staged {
    /// Where on the box it goes, as `lnk box copy` is given a folder.
    pub folder: String,
    /// Its files, each checked.
    pub files: Vec<std::path::PathBuf>,
    /// Their folder, removed when this goes.
    _dir: Option<StagedDir>,
}

impl Staged {
    /// Copies it to the box at `ip`, over its SSH, if there's anything.
    pub fn send(&self, ip: &str) -> anyhow::Result<()> {
        match self.files.is_empty() {
            true => Ok(()),
            false => crate::ssh::copy(ip, &self.files, &self.folder),
        }
    }
}

#[derive(Debug)]
struct StagedDir(std::path::PathBuf);

impl Drop for StagedDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?)
}

/// A release checked here ([`Releases::install`]): the version it is, and
/// its `install.sh`.
#[derive(Clone, Debug, PartialEq)]
pub struct Checked {
    pub version: Version,
    pub script: String,
}

/// The version `LNK_VERSION` asks for (`value`), none for the latest: one
/// at or above `floor`: a released `lnk` (a release's or a staging
/// build's) makes no box behind itself, as a spec read from a file can't
/// ask for one either.
fn asked(value: Option<String>, floor: Option<&Version>) -> anyhow::Result<Option<Version>> {
    let version: Version = match value {
        Some(v) if !v.is_empty() && v != "latest" => {
            crate::spec::lnk_version(&v).map_err(|e| anyhow::anyhow!("LNK_VERSION: {e}"))?
        }
        _ => return Ok(None),
    };
    if let Some(floor) = floor {
        ensure!(
            version >= *floor,
            "LNK_VERSION is {version}, behind this lnk {floor}: a box gets this lnk or newer"
        );
    }
    Ok(Some(version))
}

/// What ends the copy of a checked `install.sh` in a box's setup script.
const INSTALL_END: &str = "LNK_CHECKED_INSTALL_SH";

/// The script `lnk box create` runs on the box over SSH, as `ubuntu`:
/// waits for the first boot, gives the box its installation ID and name
/// (`installation.toml`, before `lnk` first runs there), then runs `checked`,
/// the `install.sh` of `releases` that [`Releases::install`] checked here,
/// copied into the script as it is, at the version checked (`install.sh`'s
/// `LNK_VERSION`, even for the latest: the box installs that version or
/// nothing, whatever the latest is by then), and checks that the `lnk` it
/// installed says it's that version, as `install.sh`, `lnk upgrade` and
/// `lnk module add` check what they install: one that doesn't is removed
/// and the setup fails. An `lnk` already there (a box from an image) is
/// kept only when it says it's that version; another is replaced the same
/// way, and then brings the image's modules to its own version (`lnk
/// upgrade`), so a kept image never runs an earlier build's modules.
/// Then adds its modules ([`modules`], `login` saying whether it signs
/// in), which that `lnk` checks with its own keys.
pub fn install_script(
    installation: &Installation,
    releases: &Releases,
    checked: &Checked,
) -> anyhow::Result<String> {
    let Checked {
        version,
        script: checked,
    } = checked;
    ensure!(
        !checked.lines().any(|l| l == INSTALL_END),
        "its install.sh holds the line that ends it in a box's setup: not sending it"
    );
    let env = releases.env();
    let said = quote(&format!("lnk {version}"));
    let wanted = quote(&version.to_string());
    let version = format!("LNK_VERSION={} ", quote(&version.to_string()));
    let newline = if checked.ends_with('\n') { "" } else { "\n" };
    Ok(format!(
        "set -e\n\
         echo 'Waiting for the first boot to finish...'\n\
         cloud-init status --wait >/dev/null 2>&1 || true\n\
         {machine}\
         had=\n\
         if [ -x \"$HOME/.lnk/exec/bin/lnk\" ]; then had=$(\"$HOME/.lnk/exec/bin/lnk\" --version | sed -n 1p || true); fi\n\
         if [ \"$had\" != {said} ]; then\n\
         {env}{version}sh <<'{INSTALL_END}'\n\
         {checked}{newline}\
         {INSTALL_END}\n\
         said=$(\"$HOME/.lnk/exec/bin/lnk\" --version | sed -n 1p)\n\
         if [ \"$said\" != {said} ]; then\n\
         rm -f \"$HOME/.lnk/exec/bin/lnk\"\n\
         echo \"The lnk installed says $said, not \"{said}\": not keeping it\" >&2\n\
         exit 1\n\
         fi\n\
         fi\n\
         if [ -n \"$had\" ] && [ \"$had\" != {said} ]; then\n\
         {env}\"$HOME/.lnk/exec/bin/lnk\" upgrade {wanted}\n\
         fi\n\
         {profile}\
         {env}\"$HOME/.lnk/exec/bin/lnk\" module add {modules}\n\
         \"$HOME/.lnk/exec/bin/lnk\" secret keeper create\n\
         \"$HOME/.lnk/exec/bin/lnk\" sandbox workers --install\n",
        machine = installation_script(installation)?,
        profile = PROFILE_PATH,
        modules = modules().join(" "),
    ))
}

/// The shell line putting the box's `lnk` on its user's `PATH` in every
/// login shell, as `lnk box ssh` opens: added to `~/.profile` once.
const PROFILE_PATH: &str = "line='PATH=\"$HOME/.lnk/exec/bin:$PATH\"'\n\
     grep -qxF \"$line\" \"$HOME/.profile\" 2>/dev/null || printf '%s\\n' \"$line\" >> \"$HOME/.profile\"\n";

/// Shell lines writing the box's `~/.lnk/safe/installation.toml` (700 folder,
/// 600 file): `installation`'s ID and name, keeping an ID it already has (a
/// box made before IDs, whose `lnk` made one), and printing the ID.
pub fn installation_script(installation: &Installation) -> anyhow::Result<String> {
    link_machine::machine::check_id(&installation.id)?;
    link_machine::machine::check_name(&installation.name)?;
    let file = link_files::layout::Hidden::Installation.relative_to_home();
    Ok(format!(
        "umask 077\n\
         mkdir -p \"$HOME/{folder}\"\n\
         f=\"$HOME/{file}\"\n\
         id=$(sed -n 's/^id = \"\\([a-z0-9]*\\)\"$/\\1/p' \"$f\" 2>/dev/null || true)\n\
         [ ${{#id}} -eq 12 ] || id={id}\n\
         printf 'id = \"%s\"\\nname = \"%s\"\\n' \"$id\" {name} > \"$f\"\n\
         echo \"lnk-installation $id\"\n",
        folder = file.parent().unwrap().display(),
        file = file.display(),
        id = installation.id,
        name = quote(&installation.name),
    ))
}

/// A shell line forgetting the box's installation ID and name: a box made
/// from an image has its saved box's, and gets its own (the install's
/// [`installation_script`] after it) rather than keep it.
pub fn forget_installation_script() -> String {
    format!(
        "rm -f \"$HOME/{}\"\n",
        link_files::layout::Hidden::Installation
            .relative_to_home()
            .display()
    )
}

/// `text` quoted for a POSIX shell.
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The shell line `lnk box ssh <box> -- <words>` runs: one word is a
/// shell line of its own (`'cd app && make'`), and several are a command
/// and its arguments, each quoted so it arrives as one argument.
pub fn command_line(words: &[String]) -> String {
    match words {
        [line] => line.clone(),
        words => words.iter().map(|w| quote(w)).collect::<Vec<_>>().join(" "),
    }
}

/// Removes [`with_deadline`]'s files from the box's disk, leaving the
/// timer already loaded to end it: an image saved from it never ends the
/// boxes made from it as they start.
/// Prints `timed` when the box has the timer of its deadline
/// ([`with_deadline`]).
pub fn has_deadline() -> String {
    format!(
        "[ -f {} ] && echo timed || true\n",
        link_cloud_contract::first_boot::DEADLINE_TIMER
    )
}

pub const WITHOUT_DEADLINE: &str = "sudo -n rm -f /etc/systemd/system/lnk-deadline.service \
    /etc/systemd/system/lnk-deadline.timer /etc/systemd/system/timers.target.wants/lnk-deadline.timer\n";

/// The first boot of a box that ends at `started` plus `deadline`, by a
/// root-owned calendar timer ([`link_cloud_contract::first_boot`]).
pub fn with_deadline(init: &str, deadline: link_cloud_contract::Deadline, started: u64) -> String {
    use link_cloud_contract::first_boot;
    let (files, commands) = first_boot::deadline(started + deadline.get().as_secs());
    first_boot::with(init, &files, &commands)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_boot_keeps_services_up_and_installs_nothing_else() {
        let kind = Kind::Box {
            os: link_cloud_contract::Os::Ubuntu2204,
            arch: link_cloud_contract::Arch::Arm64,
        };
        let init = cloud_init(kind, None).unwrap();
        assert!(init.starts_with("#cloud-config\n"));
        assert!(!init.contains("bubblewrap") && !init.contains("apparmor"));
        assert!(init.contains("[loginctl, enable-linger, ubuntu]"));
        assert!(init.contains("ssh:\n  emit_keys_to_console: true\n"));
        // It says it's a box, root's, as the box's machine is.
        assert!(init.contains(
            "write_files:\n  - path: /etc/lnk/kind.toml\n    owner: root:root\n    \
             permissions: '0644'\n    content: |\n      kind = \"box\"\n      os = \
             \"ubuntu-22.04\"\n      arch = \"arm64\"\n"
        ));
        // Every apt on it fetches a dropped download again: apt's
        // configuration holds its retries, root's, before anything runs.
        assert!(init.contains(
            "  - path: /etc/apt/apt.conf.d/80-lnk\n    owner: root:root\n    permissions: \
             '0644'\n    content: |\n      Acquire::Retries \""
        ));
        assert!(init.contains("      Acquire::http::Timeout \""));
        assert!(init.contains("      Acquire::https::Timeout \""));
        assert!(init.find("80-lnk").unwrap() < init.find("runcmd:").unwrap());
        assert!(init.contains("\nruncmd:\n  - [chmod, '0755', /etc/lnk]\n"));
        assert!(cloud_init(Kind::Computer {}, None).is_err());
        // Trusting this computer's certificates for its own principal, its
        // sshd told, Link's plain keys gone; every item two spaces in.
        let trust = Trust {
            authority: "ssh-ed25519 AAAAca lnk-ca abc dana".into(),
            principal: "lnk-box-k3v9q2x7m1ab".into(),
        };
        let init = cloud_init(kind, Some(&trust)).unwrap();
        assert!(init.contains(
            "  - path: /etc/ssh/lnk/user-cas.pub\n    owner: root:root\n    permissions: \
             '0644'\n    content: |\n      ssh-ed25519 AAAAca lnk-ca abc dana\n"
        ));
        assert!(init.contains("      lnk-box-k3v9q2x7m1ab\n"));
        assert!(init.contains("      AuthorizedPrincipalsFile /etc/ssh/lnk/principals/%u\n"));
        assert!(init.find("user-cas.pub").unwrap() < init.find("runcmd:").unwrap());
        assert!(init.contains("  - |\n    f=/home/ubuntu/.ssh/authorized_keys\n"));
        assert!(init.ends_with("  - [systemctl, try-reload-or-restart, ssh]\n"));
        assert!(
            init.lines()
                .filter(|l| l.trim_start().starts_with("- "))
                .all(|l| l.starts_with("  - ")),
            "{init}"
        );
    }

    /// The profile line, run at each setup, puts `lnk` on a login shell's
    /// `PATH` once, keeping what the profile held.
    #[test]
    fn a_login_shell_finds_the_boxs_lnk() {
        let dir = link_test_kit::TempDir::new("box-profile");
        let home = dir.join("home");
        std::fs::create_dir_all(home.join(".lnk/exec/bin")).unwrap();
        std::fs::write(home.join(".profile"), "# the image's own\n").unwrap();
        for _ in 0..2 {
            let status = link_test_kit::env::command("sh", &home)
                .args(["-c", PROFILE_PATH])
                .status()
                .unwrap();
            assert!(status.success());
        }
        let profile = std::fs::read_to_string(home.join(".profile")).unwrap();
        assert_eq!(
            profile,
            "# the image's own\nPATH=\"$HOME/.lnk/exec/bin:$PATH\"\n"
        );
        let path = link_test_kit::env::command("sh", &home)
            .args(["-c", ". \"$HOME/.profile\"; printf %s \"$PATH\""])
            .output()
            .unwrap();
        let path = String::from_utf8(path.stdout).unwrap();
        let bin = home.join(".lnk/exec/bin");
        assert!(path.starts_with(&format!("{}:", bin.display())), "{path}");
    }

    /// `lnk box ssh <box> -- pgrep -f 'sleep 9138'` reaches the box's
    /// shell as two arguments to `pgrep`, and a lone word stays a shell line.
    #[test]
    fn a_remote_command_keeps_its_arguments_apart() {
        let dir = link_test_kit::TempDir::new("box-command-line");
        let words = |list: &[&str]| list.iter().map(|w| w.to_string()).collect::<Vec<_>>();
        let run = |line: String| {
            let output = link_test_kit::env::command("sh", &dir)
                .args(["-c", &line])
                .output()
                .unwrap();
            assert!(output.status.success(), "{line}");
            String::from_utf8(output.stdout).unwrap()
        };
        let several = words(&["printf", "[%s]", "-f", "sleep 9138", "it's", "$HOME"]);
        assert_eq!(run(command_line(&several)), "[-f][sleep 9138][it's][$HOME]");
        let lone = words(&["printf one && printf ' two'"]);
        assert_eq!(run(command_line(&lone)), "one two");
    }

    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn a_box_gets_its_id_and_name_and_keeps_an_id_it_has() {
        let home =
            std::env::temp_dir().join(format!("lnk-installation-script-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let run = |id: &str, name: &str| {
            let machine = Installation {
                id: id.into(),
                name: name.into(),
            };
            let output = std::process::Command::new("sh")
                .args(["-c", &installation_script(&machine).unwrap()])
                .env("HOME", &home)
                .output()
                .unwrap();
            assert!(output.status.success());
            String::from_utf8(output.stdout).unwrap()
        };
        let file = link_files::layout::Hidden::Installation.in_home(&home);
        assert_eq!(
            run("k3v9q2x7m1ab", "gcp-1"),
            "lnk-installation k3v9q2x7m1ab\n"
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "id = \"k3v9q2x7m1ab\"\nname = \"gcp-1\"\n"
        );
        // Renamed later: the ID it has stays, and is what it says.
        assert_eq!(
            run("p8r2c4n6w0zt", "aws-1"),
            "lnk-installation k3v9q2x7m1ab\n"
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "id = \"k3v9q2x7m1ab\"\nname = \"aws-1\"\n"
        );
        let mode = std::os::unix::fs::PermissionsExt::mode(&file.metadata().unwrap().permissions());
        assert_eq!(mode & 0o077, 0);
        // What it would be told is checked first.
        let bad = Installation {
            id: "x';rm -rf".into(),
            name: "a".into(),
        };
        assert!(installation_script(&bad).is_err());
        let _ = std::fs::remove_dir_all(&home);
    }

    /// A box runs the install.sh this machine checked, copied in whole
    /// (no URL for the box to fetch a script from), at the version
    /// checked, the latest's too; adds its modules; and is pointed at releases this machine
    /// was pointed at, and only those.
    #[test]
    fn a_box_runs_the_install_checked_here() {
        let machine = Installation {
            id: "k3v9q2x7m1ab".into(),
            name: "aws-1".into(),
        };
        let url = "https://builds.example/staging/0123abcd";
        let v = |t: &str| t.parse::<Version>().unwrap();
        let followed = Releases {
            source: url.parse().unwrap(),
            version: Some(v("0.0.4-staging.202610131900")),
            followed: true,
            signers: String::new(),
        };
        let text = "#!/bin/sh\necho 'the checked install.sh'\n";
        let checked = Checked {
            version: v("0.0.4-staging.202610131900"),
            script: text.into(),
        };
        assert_eq!(
            followed.version(None),
            Some(v("0.0.4-staging.202610131900"))
        );
        let script = install_script(&machine, &followed, &checked).unwrap();
        assert!(
            script.contains("[ ${#id} -eq 12 ] || id=k3v9q2x7m1ab\n"),
            "{script}"
        );
        // Written before lnk is installed, so it never makes its own.
        assert!(script.find("installation.toml").unwrap() < script.find(INSTALL_END).unwrap());
        assert!(
            script.contains(&format!(
                "LNK_RELEASES_URL='{url}' LNK_VERSION='0.0.4-staging.202610131900' sh <<'{INSTALL_END}'\n\
                 {text}{INSTALL_END}\n"
            )),
            "{script}"
        );
        assert!(!script.contains("curl"), "{script}");
        // A login shell on the box finds its lnk, the line added once.
        assert!(script.contains(PROFILE_PATH), "{script}");
        // Login and the keeper precede every workload installation.
        assert!(
            script.ends_with(&format!(
                "LNK_RELEASES_URL='{url}' \"$HOME/.lnk/exec/bin/lnk\" module add secret sandbox\n\
                 \"$HOME/.lnk/exec/bin/lnk\" secret keeper create\n\
                 \"$HOME/.lnk/exec/bin/lnk\" sandbox workers --install\n"
            )),
            "{script}"
        );
        // Link's own: nothing passed, the script all the same.
        let own = Releases {
            source: url.parse().unwrap(),
            version: None,
            followed: false,
            signers: String::new(),
        };
        let latest = Checked {
            version: v("0.0.4"),
            script: text.into(),
        };
        let script = install_script(&machine, &own, &latest).unwrap();
        // The latest checked here, pinned: the box installs it or nothing.
        assert!(
            script.contains(&format!("\nLNK_VERSION='0.0.4' sh <<'{INSTALL_END}'\n")),
            "{script}"
        );
        assert!(!script.contains("LNK_RELEASES_URL"), "{script}");
        // A script that would end its own copy early isn't sent.
        let sneaky = format!("echo one\n{INSTALL_END}\necho two\n");
        let sneaky = Checked {
            script: sneaky,
            ..latest
        };
        assert!(install_script(&machine, &own, &sneaky).is_err());
        // Releases the box can't read where they are (a bucket of yours):
        // it reads the copy it's given, in its home.
        let bucket = Releases {
            source: "s3://my-builds/dev/0123abcd".parse().unwrap(),
            ..followed.clone()
        };
        let staged = format!("LNK_RELEASES_URL=\"file://$HOME/{}\" ", staged());
        assert_eq!(bucket.env(), staged);
        // A box given that copy follows it for good, and one reading an
        // address itself, or Link's own, follows this machine.
        assert!(bucket.follows_staged());
        assert!(!followed.follows_staged() && !own.follows_staged());
        assert_eq!(for_box(true).unwrap(), staged);
        let script = install_script(&machine, &bucket, &checked).unwrap();
        assert!(
            script.contains(&format!(
                "{staged}LNK_VERSION='0.0.4-staging.202610131900' sh <<'{INSTALL_END}'\n"
            )),
            "{script}"
        );
        assert!(
            script.contains(&format!("{staged}\"$HOME/.lnk/exec/bin/lnk\" module add")),
            "{script}"
        );
        assert!(!script.contains("s3://"), "{script}");
        // An address it reads itself is given none.
        let none = followed.stage(&checked, Arch::X8664).unwrap();
        assert!(none.files.is_empty());
        none.send("192.0.2.1").unwrap();
        // A spec's version wins over LNK_VERSION.
        let pinned = v("0.0.3");
        assert_eq!(followed.version(Some(&pinned)), Some(pinned));
        assert_eq!(own.version(None), None);
        assert_eq!(quote("it's"), r"'it'\''s'");
    }

    /// A box is given each program's archive for its processor.
    #[test]
    fn a_box_is_given_its_processors_programs() {
        let release = [
            "SHA256SUMS",
            "VERSION",
            "install.sh",
            "lnk-core-x86_64-unknown-linux-musl.tar.gz",
            "lnk-sandbox-x86_64-unknown-linux-musl.tar.gz",
            "lnk-core-aarch64-unknown-linux-musl.tar.gz",
        ];
        assert_eq!(
            staged_assets(release.into_iter(), "x86_64-unknown-linux-musl"),
            [
                "VERSION",
                "install.sh",
                "lnk-core-x86_64-unknown-linux-musl.tar.gz",
                "lnk-sandbox-x86_64-unknown-linux-musl.tar.gz",
            ]
        );
    }

    /// `LNK_VERSION` asks for a version at or above a released `lnk`;
    /// unset, empty or `latest`, the latest.
    #[test]
    fn lnk_version_has_the_floor_a_spec_has() {
        let v = |t: &str| t.parse::<Version>().unwrap();
        let floor = v("0.0.2");
        for latest in [None, Some(""), Some("latest")] {
            assert_eq!(asked(latest.map(String::from), Some(&floor)).unwrap(), None);
        }
        let ask = |text: &str, floor: Option<&Version>| asked(Some(text.into()), floor);
        assert_eq!(ask("0.0.3", Some(&floor)).unwrap(), Some(v("0.0.3")));
        assert_eq!(ask("0.0.2", Some(&floor)).unwrap(), Some(v("0.0.2")));
        let err = ask("0.0.1", Some(&floor)).unwrap_err().to_string();
        assert!(err.contains("behind this lnk 0.0.2"), "{err}");
        // A prerelease of this version comes before it.
        assert!(ask("0.0.2-staging.202610131900", Some(&floor)).is_err());
        // A build from source has no floor: any release.
        assert_eq!(ask("0.0.0", None).unwrap(), Some(v("0.0.0")));
        let err = ask("0.0", None).unwrap_err().to_string();
        assert!(err.contains("LNK_VERSION"), "{err}");
    }

    /// The setup keeps the `lnk` its install made only when it says it's
    /// the version checked: a release under one version that holds
    /// another's `lnk` is caught on the box too.
    #[test]
    #[cfg(unix)]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn a_box_keeps_only_the_lnk_of_the_version_checked() {
        let machine = Installation {
            id: "k3v9q2x7m1ab".into(),
            name: "aws-1".into(),
        };
        let own = Releases {
            source: "https://builds.example/staging".parse().unwrap(),
            version: None,
            followed: false,
            signers: String::new(),
        };
        let run = |installs: &str| {
            let home = link_test_kit::TempDir::new("box-setup-version");
            // An install.sh that puts an lnk saying it's `installs`.
            let checked = Checked {
                version: "0.0.4".parse().unwrap(),
                script: format!(
                    "mkdir -p \"$HOME/.lnk/exec/bin\"\n\
                     printf '#!/bin/sh\\necho \"%s\"\\n' 'lnk {installs}' > \"$HOME/.lnk/exec/bin/lnk\"\n\
                     chmod +x \"$HOME/.lnk/exec/bin/lnk\"\n"
                ),
            };
            let script = install_script(&machine, &own, &checked).unwrap();
            let out = std::process::Command::new("sh")
                .args(["-c", &script])
                .env("HOME", &*home)
                .env("PATH", "/usr/bin:/bin")
                .output()
                .unwrap();
            let kept = link_files::layout::Hidden::CoreProgram
                .in_home(&home)
                .exists();
            (
                out.status.success(),
                kept,
                String::from_utf8_lossy(&out.stderr).to_string(),
            )
        };
        let (ok, kept, err) = run("0.0.4");
        assert!(ok && kept, "{err}");
        let (ok, kept, err) = run("0.0.1");
        assert!(!ok && !kept, "{err}");
        assert!(err.contains("says lnk 0.0.1, not lnk 0.0.4"), "{err}");
    }

    /// A box from an image runs this version's lnk and modules: an image's
    /// lnk of another version is replaced, and the new one brings the
    /// image's modules to its version; one of this version is kept as it is.
    #[test]
    #[cfg(unix)]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn a_box_from_an_image_of_another_lnk_upgrades_it() {
        let machine = Installation {
            id: "k3v9q2x7m1ab".into(),
            name: "aws-1".into(),
        };
        let own = Releases {
            source: "https://builds.example/staging".parse().unwrap(),
            version: None,
            followed: false,
            signers: String::new(),
        };
        // An lnk saying it's `version`, each run's arguments in `ran`.
        let lnk = |version: &str| {
            format!(
                "mkdir -p \"$HOME/.lnk/exec/bin\"\n\
                 printf '#!/bin/sh\\ncase \"$1\" in --version) echo \"%s\" ;; *) echo \"%s $*\" >> \"$HOME/ran\" ;; esac\\n' \
                 'lnk {version}' '{version}' > \"$HOME/.lnk/exec/bin/lnk\"\n\
                 chmod +x \"$HOME/.lnk/exec/bin/lnk\"\n"
            )
        };
        let checked = Checked {
            version: "0.0.4".parse().unwrap(),
            script: lnk("0.0.4"),
        };
        let script = install_script(&machine, &own, &checked).unwrap();
        let run = |image: &str| {
            let home = link_test_kit::TempDir::new("box-setup-image");
            let made = std::process::Command::new("sh")
                .args(["-c", &lnk(image)])
                .env("HOME", &*home)
                .status()
                .unwrap();
            assert!(made.success());
            let out = std::process::Command::new("sh")
                .args(["-c", &script])
                .env("HOME", &*home)
                .env("PATH", "/usr/bin:/bin")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            std::fs::read_to_string(home.join("ran")).unwrap()
        };
        let ran = run("0.0.3");
        assert!(
            ran.lines()
                .next()
                .unwrap()
                .starts_with("0.0.4 upgrade 0.0.4"),
            "{ran}"
        );
        assert!(ran.contains("0.0.4 module add"), "{ran}");
        assert!(!ran.contains("0.0.3"), "{ran}");
        let ran = run("0.0.4");
        assert!(!ran.contains("upgrade"), "{ran}");
        assert!(ran.contains("0.0.4 module add"), "{ran}");
    }
}
#[cfg(test)]
mod deadline_tests {
    #[test]
    fn a_deadline_is_a_root_owned_persistent_timer() {
        let kind = super::Kind::Box {
            os: link_cloud_contract::Os::Ubuntu2404,
            arch: link_cloud_contract::Arch::X8664,
        };
        let first_boot = super::cloud_init(kind, None).unwrap();
        let init = super::with_deadline(&first_boot, "4h".parse().unwrap(), 100);
        // Every list item two spaces in, as the first boot's are: one out
        // of step and cloud-init reads none of the file.
        let items: Vec<&str> = init
            .lines()
            .filter(|l| l.trim_start().starts_with("- "))
            .collect();
        assert_eq!(items.len(), 8, "{init}");
        assert!(items.iter().all(|l| l.starts_with("  - ")), "{init}");
        assert!(init.contains("  - path: /etc/systemd/system/lnk-deadline.timer\n"));
        assert!(init.contains("      OnCalendar=@14500\n"));
        assert!(init.contains("Persistent=true"));
        assert!(init.contains("owner: root:root"));
        assert!(init.contains("[systemctl, enable, --now, lnk-deadline.timer]"));
        assert!(
            init.find("/etc/systemd/system/lnk-deadline.timer").unwrap()
                < init.find("runcmd:").unwrap()
        );
    }
}
