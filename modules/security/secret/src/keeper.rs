//! The keeper's user on Linux (`lnk-secrets`): made once, with `sudo`,
//! by `lnk secret keeper create` or a yes at the first `lnk secret create`; and the
//! keeper itself, `lnk secret keep`, which systemd runs as that user
//! ([`link_secret_contract::keeper`]).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::ensure;
use link_format::{
    Done, Exit, Note, Out, Path as Shown, Product, Question, Report, WithinExt, cmd, phrase, prose,
    sentence,
};
use link_secret_contract::{self as secrets, keeper};

/// The keeper's service.
pub const SERVICE: &str = "lnk-secrets.service";

/// Where its unit is.
pub const UNIT: &str = "/etc/systemd/system/lnk-secrets.service";

/// The TPM's resource manager, which the keeper alone of Link's users
/// opens.
const DEVICE: &str = "/dev/tpmrm0";

/// The group setup gives that device where the machine's own rules leave
/// it root's, as Google Cloud's Ubuntu image does.
pub const DEVICE_GROUP: &str = "lnk-tpm";

/// Its udev rule, which gives it the group again at every boot: after
/// any package's rule, and final (`:=`).
pub const DEVICE_RULE: &str = "/etc/udev/rules.d/70-lnk-tpm.rules";

/// How long a keeper just started has to answer.
const START_WAIT: Duration = Duration::from_secs(15);

/// The keeper's unit: run as its user, which runs nothing else, with
/// nothing of the machine it doesn't need, never dumped, its memory
/// locked out of swap.
pub fn unit() -> String {
    let user = keeper::KEEPER_USER;
    let program = link_files::layout::System::SecretProgram.path();
    let store =
        link_files::layout::System::SecretStore.path_for(link_files::layout::SystemOs::Linux);
    use std::os::unix::fs::MetadataExt;
    let device_group = std::fs::metadata(device())
        .ok()
        .map(|m| format!("SupplementaryGroups={}\n", m.gid()))
        .unwrap_or_default();
    format!(
        "# Made by lnk secret keeper create: the keeper of each user's secrets.
[Unit]
Description=Link's keeper of your secrets

[Service]
User={user}
Group={user}
{device_group}\
ExecStart={program} keep --store {store}
RuntimeDirectory={user}
RuntimeDirectoryMode=0755
ReadWritePaths={store}
UMask=0077
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
DevicePolicy=closed
DeviceAllow=/dev/tpmrm0 rw
LimitCORE=0
LimitMEMLOCK=infinity
Restart=on-failure

[Install]
WantedBy=multi-user.target
",
        program = program.display(),
        store = store.display(),
    )
}

/// The TPM's device, or a stand-in for tests (`LNK_TEST_TPM_DEVICE`),
/// which a debug build alone reads.
fn device() -> PathBuf {
    link_secretive::test_var("LNK_TEST_TPM_DEVICE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEVICE))
}

/// The udev rule that keeps the device [`DEVICE_GROUP`]'s.
pub fn device_rule() -> String {
    format!(
        "# Made by lnk secret keeper create: the TPM's resource manager, read and written by the
# keeper's group alone.
KERNEL==\"tpmrm[0-9]*\", SUBSYSTEM==\"tpmrm\", GROUP:=\"{DEVICE_GROUP}\", MODE:=\"0660\"
"
    )
}

/// Whether the device is a non-root group's to read and write, which the
/// keeper's service joins.
fn device_given(device: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    device.gid() != 0 && device.mode() & 0o060 == 0o060
}

/// Gives the TPM's device a group of its own where it's root's alone: the
/// group made, the device given it now and, by its rule, at every boot.
/// A device that still isn't a group's is refused.
fn give_device() -> anyhow::Result<()> {
    let device = device();
    let Ok(found) = std::fs::metadata(&device) else {
        return Ok(());
    };
    if device_given(&found) {
        return Ok(());
    }
    let path = device.to_str().within("the TPM's path isn't text")?;
    as_root(&["groupadd", "--system", "--force", DEVICE_GROUP])?;
    let made = link_files::private_temp_dir("keeper").within("making a folder for its rule")?;
    let file = made.join("lnk-tpm.rules");
    std::fs::write(&file, device_rule()).within("writing its rule")?;
    let installed = as_root(&[
        "install",
        "-o",
        "root",
        "-g",
        "root",
        "-m",
        "644",
        file.to_str().within("its rule's path isn't text")?,
        DEVICE_RULE,
    ]);
    let _ = std::fs::remove_dir_all(&made);
    installed?;
    as_root(&["chgrp", DEVICE_GROUP, path])?;
    as_root(&["chmod", "0660", path])?;
    let given = std::fs::metadata(&device).is_ok_and(|found| device_given(&found));
    ensure!(
        given,
        "{DEVICE} needs a dedicated non-root group with read and write permission for the keeper"
    );
    Ok(())
}

/// `sudo`, or a stand-in for tests (`LNK_SUDO`), which a debug build
/// alone reads: what runs as root is never a program a variable names.
fn sudo() -> PathBuf {
    link_secretive::test_var("LNK_SUDO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("sudo"))
}

/// Runs `args` as root, with `sudo`, which asks for your password where
/// it must.
fn as_root(args: &[&str]) -> anyhow::Result<()> {
    let mut command =
        if link_secretive::test_var("LNK_SUDO").is_none() && unsafe { libc::geteuid() } == 0 {
            link_secretive::process::command(args[0])
        } else {
            let mut command = link_secretive::process::command(sudo());
            command.arg(args[0]);
            command
        };
    let status = command
        .args(&args[1..])
        .status()
        .within(format!("running sudo {}", args.join(" ")))?;
    ensure!(
        status.success(),
        "sudo {} failed ({status})",
        args.join(" ")
    );
    Ok(())
}

/// A keeper that could not be made: its steps' own words are the why.
fn unmade(err: anyhow::Error) -> Report {
    crate::failed(sentence!("The keeper could not be made."), err)
        .next(cmd!("lnk secret keeper create"), phrase!("try again"))
}

/// Makes the keeper's user and its service, and starts it: then your
/// secrets, kept in your own files until now, move to it. Done again, it
/// puts this `lnk-secret` in the keeper's place and starts it again.
pub fn make(out: &mut Out) -> Result<Done, Report> {
    if !cfg!(target_os = "linux") {
        return Err(Report::error(sentence!("The keeper is for Linux."))
            .why(prose!(
                "On a Mac the Keychain keeps your secrets, each to the programs allowed it."
            ))
            .next(cmd!("lnk secret list"), phrase!("see what is kept")));
    }
    if !(Path::new("/run/systemd/system").exists()
        || link_secretive::test_var("LNK_SUDO").is_some())
    {
        return Err(Report::error(sentence!(
            "The keeper runs as a service of systemd, which this machine doesn't run."
        ))
        .why(prose!(
            "Your secrets stay in {}, only yours.",
            Shown::new(secrets::path())
        ))
        .next(cmd!("lnk secret list"), phrase!("see what is kept")));
    }
    // A keeper never takes your file: its secrets are added to it again.
    secrets::keeper::yours_held().map_err(|err| {
        crate::failed(
            sentence!("The keeper would leave your secrets behind."),
            err,
        )
    })?;
    // sudo asks for a password on the terminal, and its lines are its own.
    let _ = out.hand_over();
    steps(out).map_err(unmade)?;
    Ok(Done::success(sentence!(
        "Your secrets are kept by {} now: no program of yours reads their files, and none in a \
         sandbox reaches the keeper.",
        Product::new(keeper::KEEPER_USER)
    ))
    .next(cmd!("lnk secret list"), phrase!("see what is kept")))
}

#[expect(
    clippy::disallowed_methods,
    reason = "polls a starting keeper until it answers"
)]
fn steps(out: &mut Out) -> anyhow::Result<()> {
    let user = keeper::KEEPER_USER;
    let store = link_files::layout::System::SecretStore.path();
    let store = store
        .to_str()
        .expect("the installed system store path is text");
    out.block(Note(sentence!(
        "Making {}, a user that holds your secrets and runs nothing else: sudo asks for your \
         password once.",
        Product::new(user)
    )));
    if !keeper::here() {
        as_root(&[
            "useradd",
            "--system",
            "--home-dir",
            store,
            "--no-create-home",
            "--shell",
            "/usr/sbin/nologin",
            user,
        ])?;
    }
    as_root(&["install", "-d", "-o", user, "-g", user, "-m", "700", store])?;
    if secrets::at_rest::has_tpm() {
        let found = link_secretive::process::command("tpm2")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !found {
            // apt fetching a dropped download again, as the sandbox's
            // install does (link_machine::apt).
            for args in [
                &["update", "-q"][..],
                &["install", "-y", "-q", "tpm2-tools"][..],
            ] {
                let words = link_machine::apt::apt_get(args)?;
                as_root(&words.iter().map(String::as_str).collect::<Vec<_>>())?;
            }
        }
        // After the install, whose packages may bring a rule of their own.
        give_device()?;
    }
    out.block(Note(match secrets::at_rest::has_tpm() {
        true => sentence!("The keeper seals its store to this machine's TPM."),
        false => sentence!("No TPM was found, so the keeper uses its private files."),
    }));
    let program = std::env::current_exe().within("finding lnk-secret")?;
    let program = program.to_str().within("lnk-secret's path isn't text")?;
    let installed_program = link_files::layout::System::SecretProgram.path();
    let installed_program = installed_program
        .to_str()
        .expect("the installed keeper program path is text");
    as_root(&[
        "install",
        "-D",
        "-o",
        "root",
        "-g",
        "root",
        "-m",
        "755",
        program,
        installed_program,
    ])?;
    let made = link_files::private_temp_dir("keeper").within("making a folder for its unit")?;
    let file = made.join(SERVICE);
    std::fs::write(&file, unit()).within("writing its unit")?;
    let installed = as_root(&[
        "install",
        "-o",
        "root",
        "-g",
        "root",
        "-m",
        "644",
        file.to_str().within("its unit's path isn't text")?,
        UNIT,
    ]);
    let _ = std::fs::remove_dir_all(&made);
    installed?;
    as_root(&["systemctl", "daemon-reload"])?;
    as_root(&["systemctl", "enable", SERVICE])?;
    as_root(&["systemctl", "restart", SERVICE])?;
    // Up once it answers.
    let started = Instant::now();
    loop {
        match secrets::list() {
            Ok(_) => break,
            Err(_) if started.elapsed() < START_WAIT => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(err) => return Err(err.context(format!("{SERVICE} didn't start"))),
        }
    }
    Ok(())
}

/// Before your first secret on a Linux computer, where there's no keeper
/// yet and you haven't said no: asks whether to make one, once. Yes
/// (`--yes` says it) makes it; no is remembered (`secret-settings.toml`'s
/// `keeper = false`). Without a terminal nothing is asked, and it says
/// where your secrets are kept.
pub fn ask_first(out: &mut Out, yes: bool) -> Result<(), Report> {
    let first = !secrets::path().exists();
    if !cfg!(target_os = "linux")
        || keeper::here()
        || !first
        || crate::preferences::keeper_refused().map_err(crate::unreadable)?
    {
        return Ok(());
    }
    let made = |out: &mut Out| -> Result<(), Report> {
        let done = make(out)?;
        if let Some(success) = done.success {
            out.block(success);
        }
        Ok(())
    };
    if yes {
        return made(out);
    }
    if !out.can_ask() {
        out.block(Note(sentence!(
            "Kept in {}, which any program you run can read: lnk secret keeper create keeps your \
             secrets under a user of their own instead.",
            Shown::new(secrets::path())
        )));
        return Ok(());
    }
    let answer = out.ask(
        Question::confirm(
            sentence!(
                "Keep your secrets under a user of their own ({}), which no program of yours \
                 reads?",
                Product::new(keeper::KEEPER_USER)
            ),
            true,
        )
        .why(prose!("It needs sudo once.")),
    )?;
    match answer {
        true => made(out),
        false => {
            crate::preferences::refuse_keeper().map_err(crate::unreadable)?;
            out.block(Note(sentence!(
                "Kept in {}, only yours, which any program you run can read: not asked again \
                 here; lnk secret keeper create makes the keeper's user later.",
                Shown::new(secrets::path())
            )));
            Ok(())
        }
    }
}

/// Serves the keeper: as its user alone, at `socket` ([`keeper::SOCKET`]),
/// each user's store under `store` (`System::SecretStore` by default).
pub fn keep(socket: Option<PathBuf>, store: Option<PathBuf>) -> Result<(), Report> {
    let test = link_secretive::test_var(keeper::TEST_KEEPER);
    if test.is_none() && !keeper::running_as_keeper() {
        return Err(Report::error(sentence!(
            "The keeper runs as {} alone.",
            Product::new(keeper::KEEPER_USER)
        ))
        .why(prose!("Its service starts it."))
        .exit(Exit::Refused)
        .next(
            cmd!("lnk secret keeper create"),
            phrase!("make the keeper and its service"),
        ));
    }
    link_format::daemon::logs();
    serve(socket, store, test).map_err(|err| {
        crate::failed(sentence!("The keeper stopped."), err).next(
            cmd!("lnk secret keeper create"),
            phrase!("make the keeper again"),
        )
    })
}

fn serve(
    socket: Option<PathBuf>,
    store: Option<PathBuf>,
    test: Option<std::ffi::OsString>,
) -> anyhow::Result<()> {
    let socket = socket
        .or(test.map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(keeper::SOCKET));
    let store = store.unwrap_or_else(|| link_files::layout::System::SecretStore.path());
    // Anyone's to call: each caller reaches its own user's store alone.
    let listener = link_sandbox_bridge::socket::listen_open(&socket)?;
    let routes = std::sync::Arc::new(keeper::Routes::default());
    let management = routes.clone();
    let stores = store.clone();
    std::thread::spawn(move || keeper::serve_with_routes(listener, &store, management));
    let route_socket = socket.with_file_name("secret.sock");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let listener = link_sandbox_bridge::socket::listen_async(
            &route_socket,
            link_sandbox_bridge::socket::Given {
                mode: 0o666,
                ..Default::default()
            },
        )?;
        link_sandbox_binding::serve_streams_from(
            listener,
            routes.clone(),
            &crate::bridge::LIMITS,
            &std::sync::atomic::AtomicBool::new(false),
            |uid, who, mut call: link_sandbox_route::Call, stream| {
                let routes = routes.clone();
                // A secret granted by name is read from the store it keeps
                // for the proxy's user, while it's that user's.
                let kept = keeper::user_store(&stores, uid);
                async move {
                    let kept = kept?;
                    let given = routes.open(uid, &who, &call.given)?;
                    call.given = link_secretive::Secret::new(serde_json::to_string(&given)?);
                    crate::bridge::handle(crate::bridge::Granted::Kept(kept), who, call, stream)
                        .await
                }
            },
        )
        .await;
        anyhow::Ok(())
    })
}

/// First boot's receiver and the restart cache. Only the image's root
/// launcher invokes this helper; its output is a private pipe, never a
/// terminal. A missing cache requests a new sealed handoff, and a broken
/// cache fails closed rather than replaying bootstrap metadata.
pub fn bootstrap(file: &Path, instance: &str) -> anyhow::Result<()> {
    use std::io::{Read, Write};
    ensure!(
        !link_format::streams::stdout_is_terminal(),
        "bootstrap credentials go only to their launcher pipe"
    );
    ensure!(
        unsafe { libc::geteuid() } == 0 || link_secretive::test_var("LNK_TEST_BOOTSTRAP").is_some(),
        "only the boot launcher receives box credentials"
    );
    ensure!(
        secrets::at_rest::has_tpm(),
        "packed bootstrap requires its vTPM; no plaintext fallback"
    );
    let mut out = link_format::streams::protocol();
    let _lock = link_files::lock_file(&file.with_extension("lock"))?;
    let bytes = match secrets::at_rest::read(file, true)? {
        Some(bytes) => {
            check_bootstrap(&bytes)?;
            writeln!(out, "lnk-boot-ready")?;
            out.flush()?;
            bytes
        }
        None => {
            let recipient = link_secretive::Recipient::new();
            recipient.announce(&mut out)?;
            let mut input = link_secretive::Secret::new(Vec::new());
            std::io::stdin()
                .take((16 << 20) + 1)
                .read_to_end(input.expose_mut())?;
            ensure!(
                input.expose().len() <= 16 << 20,
                "bootstrap credentials exceed the handoff limit"
            );
            let sealed =
                serde_json::from_slice(input.expose()).within("not a sealed bootstrap handoff")?;
            let about = link_secretive::About {
                owner: instance.into(),
                kind: "box-bootstrap".into(),
                version: 1,
            };
            let bytes: link_secretive::Secret<Vec<u8>> = recipient.open_sealed(&sealed, &about)?;
            // Validate before caching anything that would fail every restart.
            check_bootstrap(&bytes)?;
            secrets::at_rest::write(file, bytes.expose())?;
            bytes
        }
    };
    out.write_all(bytes.expose())?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

fn check_bootstrap(bytes: &link_secretive::Secret<Vec<u8>>) -> anyhow::Result<()> {
    #[derive(serde::Deserialize)]
    #[serde(transparent)]
    struct Bundle(
        #[serde(with = "link_secretive::bare")]
        std::collections::BTreeMap<String, link_secretive::Secret>,
    );
    serde_json::from_slice::<Bundle>(bytes.expose())
        .map(|bundle| drop(bundle.0))
        .map_err(|_| anyhow::anyhow!("invalid bootstrap bundle"))
}
