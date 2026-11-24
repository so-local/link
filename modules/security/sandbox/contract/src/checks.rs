//! The checks `actions.toml` names, run after the declared ones pass:
//! which of a command's arguments go together, and what a host, an
//! endpoint or a machine of the network may be.

use crate::spec::{
    SandboxConfineArgs, SandboxEnterArgs, SandboxProxyArgs, SandboxProxyInsideArgs,
    SandboxWorkersArgs, ToolAddArgs,
};

/// `--flag` needs `--other`.
fn needs(given: bool, flag: &str, other: bool, wanted: &str) -> Result<(), String> {
    match !given || other {
        true => Ok(()),
        false => Err(format!("--{flag} needs {wanted}")),
    }
}

/// `--flag` can't be given with `--other`.
fn apart(given: bool, flag: &str, other: bool, beside: &str) -> Result<(), String> {
    match given && other {
        true => Err(format!("--{flag} can't be given with --{beside}")),
        false => Ok(()),
    }
}

/// Each of `values` as `check` takes it, naming `flag`.
fn each(
    values: &[String],
    flag: &str,
    check: impl Fn(&str) -> anyhow::Result<()>,
) -> Result<(), String> {
    values
        .iter()
        .try_for_each(|value| check(value).map_err(|err| format!("--{flag} {value:?}: {err:#}")))
}

/// Each endpoint a host and its port.
fn endpoints(tcp: &[String]) -> Result<(), String> {
    each(tcp, "tcp", |endpoint| {
        crate::net::check_endpoint(endpoint).map(|_| ())
    })
}

/// `lnk sandbox confine`: a worker's user with its groups, and the file
/// wall's paths and the program list with what turns each on.
pub fn confined_as_asked(args: &SandboxConfineArgs) -> Result<(), String> {
    let worker = args.worker_user.is_some();
    needs(
        worker,
        "worker-user",
        !args.worker_group.is_empty(),
        "--worker-group",
    )?;
    needs(
        !args.worker_group.is_empty(),
        "worker-group",
        worker,
        "--worker-user",
    )?;
    needs(
        args.worker_dir.is_some(),
        "worker-dir",
        worker,
        "--worker-user",
    )?;
    needs(!args.exec.is_empty(), "exec", args.only, "--only")?;
    needs(!args.read.is_empty(), "read", args.file_wall, "--file-wall")?;
    needs(
        !args.write.is_empty(),
        "write",
        args.file_wall,
        "--file-wall",
    )
}

/// `lnk sandbox enter`: a session or a run, never both.
pub fn entered_as_asked(args: &SandboxEnterArgs) -> Result<(), String> {
    apart(
        args.check,
        "check",
        !args.command.is_empty(),
        "the command after --",
    )
}

/// `lnk sandbox proxy`: a socket folder or a loopback port, and a blank
/// proxy alone with its control socket.
pub fn proxied_as_asked(args: &SandboxProxyArgs) -> Result<(), String> {
    let listen = args.listen.is_some();
    let dir = args.dir.is_some();
    match (dir, listen) {
        (true, true) => return Err("--dir can't be given with --listen".into()),
        (false, false) => return Err("give --dir or --listen".into()),
        _ => {}
    }
    needs(args.launch.is_some(), "launch", listen, "--listen")?;
    if !args.blank && args.upstream.is_none() {
        return Err("give --upstream, unless --blank".into());
    }
    if args.blank {
        needs(
            true,
            "blank",
            args.control.is_some() && dir,
            "--control and --dir",
        )?;
        let beside = [
            ("name", args.name.is_some()),
            ("wire", args.wire.is_some()),
            ("listen", listen),
            ("upstream", args.upstream.is_some()),
            ("host", !args.host.is_empty()),
            ("tcp", !args.tcp.is_empty()),
            ("serve", args.serve.is_some()),
            ("reach", args.reach.is_some()),
            ("desktop", !args.desktop.is_empty()),
            ("lan", !args.lan.is_empty()),
            ("no-internet", args.no_internet),
            ("per-minute", args.per_minute.is_some()),
            ("routes", args.routes.is_some()),
            ("routes-handed", args.routes_handed.is_some()),
            ("url-secret", args.url_secret.is_some()),
            ("pointed-by", args.pointed_by.is_some()),
            ("delete-routes", args.delete_routes),
            ("dns", args.dns),
            ("launch", args.launch.is_some()),
        ];
        for (flag, given) in beside {
            apart(true, "blank", given, flag)?;
        }
    }
    needs(args.control.is_some(), "control", args.blank, "--blank")?;
    needs(args.binder.is_some(), "binder", args.blank, "--blank")?;
    needs(
        args.agent_user.is_some(),
        "agent-user",
        args.group.is_some() && dir,
        "--group and --dir",
    )?;
    needs(
        args.group.is_some(),
        "group",
        args.agent_user.is_some(),
        "--agent-user",
    )?;
    apart(
        args.routes.is_some(),
        "routes",
        args.routes_handed.is_some(),
        "routes-handed",
    )?;
    needs(args.dns, "dns", dir, "--dir")?;
    needs(args.hold, "hold", dir, "--dir")?;
    apart(args.hold, "hold", args.dns, "dns")?;
    each(&args.host, "host", crate::net::check_host)?;
    endpoints(&args.tcp)?;
    each(&args.lan, "lan", crate::net::check_lan)
}

/// `lnk sandbox proxy-inside`: each other proxy as `DIR=PORT`, and each
/// endpoint a host and its port.
pub fn proxied_inside_as_asked(args: &SandboxProxyInsideArgs) -> Result<(), String> {
    each(&args.also, "also", |also| match also.rsplit_once('=') {
        Some((dir, port)) if !dir.is_empty() && !port.is_empty() => Ok(()),
        _ => Err(anyhow::anyhow!("isn't DIR=PORT")),
    })?;
    endpoints(&args.tcp)
}

/// `lnk sandbox workers`: one task a run.
pub fn one_task(args: &SandboxWorkersArgs) -> Result<(), String> {
    apart(args.install, "install", args.configure, "configure")?;
    apart(args.install, "install", args.reap, "reap")?;
    apart(args.configure, "configure", args.reap, "reap")
}

/// `lnk sandbox tool add`: a server's command or its URL, and a URL's
/// headers with the URL alone.
pub fn one_server(args: &ToolAddArgs) -> Result<(), String> {
    match (args.url.is_some(), args.command.is_empty()) {
        (true, false) => return Err("--url can't be given with a command after --".into()),
        (false, true) => return Err("give the server's command after --, or --url".into()),
        _ => {}
    }
    needs(
        args.header_file.is_some(),
        "header-file",
        args.url.is_some(),
        "--url",
    )
}
