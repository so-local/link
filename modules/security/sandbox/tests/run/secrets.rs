//! Secrets on descriptor 3: variables given while the secret stays outside,
//! writes refused as a spec's, nothing left behind, sealed git tokens.

use super::*;
use std::os::unix::fs::MetadataExt;

/// `--with-fd`: the variables reach the program, a secret its proxy
/// adds doesn't (only where its service is and a placeholder), and
/// nothing of it is left in Link's settings once the run ends.
#[test]
fn with_fd_gives_variables_and_keeps_a_secret_outside() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    // The secret module's bridge serves this route, found beside
    // lnk-sandbox: built there first, as a whole workspace's build would.
    let _bridge = crate::module("secret");
    let world = World::for_specs("with-fd");
    let spec = world.root.join("run.toml");
    std::fs::write(
        &spec,
        format!(
            "schema = 1\nname = \"run\"\nversion = 1\n\n[files]\nwrite = [\"{}\"]\n\n[commands]\ntoolchains = [\"shell\"]\n",
            world.work.display()
        ),
    )
    .unwrap();
    let with = serde_json::json!({
        "env": {"DATABASE_URL": "postgres://localhost/app"},
        "secrets": [{
            "secret": {
                "name": "stripe-key",
                "value": "sk_live_SEALED",
                "host": "api.stripe.com",
                "paths": [{"prefix": "", "rest": "path"}],
                "header": {"name": "Authorization", "value": "Bearer {secret}"},
                "methods": ["GET", "POST"]
            },
            "url": "LNK_SEALED_STRIPE_KEY_URL",
            "placeholder": "STRIPE_KEY"
        }],
        "write": [world.work.join("out")]
    });
    std::fs::create_dir_all(world.work.join("out")).unwrap();
    let with_file = world.root.join("with.json");
    std::fs::write(&with_file, with.to_string()).unwrap();
    let script = r#"env > "$HOME/work/env"
printf 'sk_live_%s\n' SEALED > /tmp/pattern
grep -rlsf /tmp/pattern --exclude=pattern "$HOME" /tmp /proc/$$/environ /proc/$$/cmdline > "$HOME/work/found"
curl -s -H "Authorization: Bearer wrong" "$LNK_SEALED_STRIPE_KEY_URL/v1/charges" > "$HOME/work/answer" 2>&1
echo ok > "$HOME/work/out/written""#;
    let output = world
        .lnk(&[
            "run",
            "--spec",
            &spec.display().to_string(),
            "--yes",
            "--with-fd",
            "3",
            "--",
            "/bin/bash",
            "-c",
            script,
        ])
        .output_with_fd3(&with_file);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let env = read(&world.work.join("env"));
    assert!(
        env.contains("DATABASE_URL=postgres://localhost/app"),
        "{env}"
    );
    assert!(
        env.contains("LNK_SEALED_STRIPE_KEY_URL=http://127.0.0.1:40781/_link/secret/stripe-key"),
        "{env}"
    );
    assert!(env.contains("STRIPE_KEY=link-wire-"), "{env}");
    assert!(!env.contains("sk_live_SEALED"), "{env}");
    assert_eq!(read(&world.work.join("found")), "", "a file holds it");
    assert!(
        !read(&world.work.join("answer")).contains("sk_live_SEALED"),
        "the proxy's answer holds it"
    );
    assert_eq!(read(&world.work.join("out/written")), "ok\n");
    let settings = world.home.join(".lnk/safe/sandboxes");
    let left: Vec<String> = std::fs::read_dir(&settings)
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.contains("secret"))
                .collect()
        })
        .unwrap_or_default();
    assert!(left.is_empty(), "left in Link's settings: {left:?}");
}

/// `--with-fd`'s `write`: each folder refused as a spec's write would
/// be, a dot folder's however deep; a folder a spec could write, written.
#[test]
fn with_fd_writes_are_refused_as_a_specs() {
    let world = World::for_specs("with-fd-write");
    let spec = world.root.join("run.toml");
    std::fs::write(
        &spec,
        "schema = 1\nname = \"run\"\nversion = 1\n\n[commands]\ntoolchains = [\"shell\"]\n",
    )
    .unwrap();
    let home = &world.home;
    let cache = home.join(".cache");
    for folder in ["Link/Agents/main", ".cache/0a1b2c3d4e/files"] {
        std::fs::create_dir_all(home.join(folder)).unwrap();
    }
    std::os::unix::fs::symlink(home.join(".ssh"), cache.join("linked")).unwrap();
    // An ordinary user's link is refused before its target is followed;
    // only a root-owned link reaches the target's dot-folder check.
    let linked_why = if std::fs::symlink_metadata(cache.join("linked"))
        .unwrap()
        .uid()
        == 0
    {
        "dot folder"
    } else {
        "a non-root user controls"
    };
    // A link another user could plant, to a folder a spec could write.
    let shared = home.join("shared");
    std::fs::create_dir_all(home.join("elsewhere")).unwrap();
    let planted = non_root_link(&home.join("elsewhere"), &shared);
    let run = |write: &Path, script: &str| {
        let with = serde_json::json!({ "write": [write] });
        let with_file = world.root.join("with.json");
        std::fs::write(&with_file, with.to_string()).unwrap();
        world
            .lnk(&[
                "run",
                "--spec",
                &spec.display().to_string(),
                "--yes",
                "--with-fd",
                "3",
                "--",
                "/bin/bash",
                "-c",
                script,
            ])
            .output_with_fd3(&with_file)
    };
    for (folder, why) in [
        (home.join(".ssh"), "dot folder"),
        (home.clone(), "home folder itself"),
        (home.join("Link/Agents/main"), "closed to every spec"),
        (
            home.join("Link"),
            "holds Link's settings or agents' folders",
        ),
        // It holds root's installed programs (`/usr/local/lib/lnk`).
        (PathBuf::from("/usr"), "holds Link's settings"),
        (cache.join("../work"), "in full"),
        (cache.join("linked"), linked_why),
        (cache.join("missing"), "in full"),
        (cache.join("0a1b2c3d4e/files"), "dot folder"),
        (shared.clone(), "a non-root user controls"),
    ] {
        if folder == shared && !planted {
            link_test_kit::gate::lacks(
                "its case of a link another user owns",
                "root here can't hand a link to nobody (`lchown` is refused)",
                None,
            );
            continue;
        }
        let output = run(&folder, "true");
        let err = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{} was taken", folder.display());
        assert!(err.contains(why), "{}: {err}", folder.display());
    }
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let out = &world.work;
    let output = run(out, &format!("echo ok > '{}/result'", out.display()));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(read(&out.join("result")), "ok\n");
}

/// A run stopped by a signal (a job cancelled, its services ended)
/// leaves no secret at rest: its proxy removed the file once read, and
/// the run cleans up after what it runs has ended.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_run_stopped_leaves_no_secret_behind() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    // The secret module's bridge serves this route, found beside
    // lnk-sandbox: built there first, as a whole workspace's build would.
    let _bridge = crate::module("secret");
    let world = World::for_specs("with-fd-stopped");
    let spec = world.root.join("run.toml");
    std::fs::write(
        &spec,
        format!(
            "schema = 1\nname = \"run\"\nversion = 1\n\n[files]\nwrite = [\"{}\"]\n\n[commands]\ntoolchains = [\"shell\"]\n",
            world.work.display()
        ),
    )
    .unwrap();
    let with = serde_json::json!({
        "secrets": [{
            "secret": {
                "name": "stripe-key",
                "value": "sk_live_SEALED",
                "host": "api.stripe.com",
                "paths": [{"prefix": "", "rest": "path"}],
                "header": {"name": "Authorization", "value": "Bearer {secret}"},
                "methods": ["GET"]
            },
            "url": "LNK_SEALED_STRIPE_KEY_URL",
            "placeholder": "STRIPE_KEY"
        }]
    });
    let with_file = world.root.join("with.json");
    std::fs::write(&with_file, with.to_string()).unwrap();
    let mut child = world
        .lnk(&[
            "run",
            "--spec",
            &spec.display().to_string(),
            "--yes",
            "--with-fd",
            "3",
            "--",
            "/bin/bash",
            "-c",
            "touch \"$HOME/work/started\"; exec sleep 60",
        ])
        .spawn_with_fd3(&with_file);
    let started = world.work.join("started");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !started.exists() {
        assert!(std::time::Instant::now() < deadline, "it never started");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let settings = world.home.join(".lnk/safe/sandboxes");
    let holding = || -> Vec<String> {
        std::fs::read_dir(&settings)
            .map(|d| {
                d.flatten()
                    .filter(|e| {
                        std::fs::read_to_string(e.path())
                            .is_ok_and(|t| t.contains("sk_live_SEALED"))
                    })
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    assert_eq!(holding(), Vec::<String>::new(), "at rest while it runs");
    // SAFETY: a signal to the run this test started.
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
    let status = child.wait().unwrap();
    assert!(!status.success());
    assert_eq!(holding(), Vec::<String>::new(), "left once stopped");
    let left: Vec<String> = std::fs::read_dir(&settings)
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.starts_with("run-"))
                .collect()
        })
        .unwrap_or_default();
    assert!(left.is_empty(), "left in Link's settings: {left:?}");
}

/// A command run with `file` open as its descriptor 3.
trait WithFd3 {
    fn output_with_fd3(&mut self, file: &Path) -> std::process::Output;
    fn spawn_with_fd3(&mut self, file: &Path) -> std::process::Child;
}

impl WithFd3 for Command {
    fn spawn_with_fd3(&mut self, file: &Path) -> std::process::Child {
        use std::os::fd::IntoRawFd;
        use std::os::unix::process::CommandExt;
        let fd = std::fs::File::open(file).unwrap().into_raw_fd();
        // SAFETY: dup2 is async-signal-safe; the file is open for this.
        unsafe {
            self.pre_exec(move || {
                let kept = match fd {
                    3 => libc::fcntl(3, libc::F_SETFD, 0),
                    _ => libc::dup2(fd, 3),
                };
                if kept < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = self.spawn().unwrap();
        // SAFETY: ours, opened above.
        drop(unsafe { <std::fs::File as std::os::fd::FromRawFd>::from_raw_fd(fd) });
        child
    }

    fn output_with_fd3(&mut self, file: &Path) -> std::process::Output {
        use std::os::fd::IntoRawFd;
        use std::os::unix::process::CommandExt;
        let fd = std::fs::File::open(file).unwrap().into_raw_fd();
        // SAFETY: dup2 is async-signal-safe; the file is open for this.
        unsafe {
            self.pre_exec(move || {
                // Opened as 3 itself, it's closed at exec unless kept.
                let kept = match fd {
                    3 => libc::fcntl(3, libc::F_SETFD, 0),
                    _ => libc::dup2(fd, 3),
                };
                if kept < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let output = self.output().unwrap();
        // SAFETY: ours, opened above.
        drop(unsafe { <std::fs::File as std::os::fd::FromRawFd>::from_raw_fd(fd) });
        output
    }
}

/// A git host for [`a_sealed_token_clones_with_git_and_stays_outside`]:
/// a repository's files over HTTPS (git's dumb protocol), refused (401)
/// to a request without `Authorization: <want>`, each request's header
/// kept in `seen`; and a CONNECT proxy on `connect` that leads to it,
/// whatever name is asked.
const GIT_HOST: &str = r#"
import base64, http.server, os, socket, ssl, sys, threading
root, cert, key, want, seen, connect, settings, token = sys.argv[1:9]
def disk():
    # While the proxy runs: each file of the settings holding the token,
    # and the proxy's command line.
    for folder, _, files in os.walk(settings):
        for name in files:
            try:
                if token.encode() in open(os.path.join(folder, name), "rb").read():
                    open(seen + ".disk", "a").write(os.path.join(folder, name) + "\n")
            except OSError: pass
    for pid in os.listdir("/proc"):
        try: line = open("/proc/" + pid + "/cmdline", "rb").read().split(b"\0")
        except OSError: continue
        # Concurrent fixtures have their own proxies and route options.
        # Observe only this fixture's actual proxy, identified by its wire.
        if line[1:2] != [b"proxy"]: continue
        wire = next((b for a, b in zip(line, line[1:]) if a == b"--wire"), None)
        if wire == os.fsencode(os.path.join(settings, "safe", "sandboxes")):
            open(seen + ".proxy", "a").write(" ".join(a.decode() for a in line) + "\n")
class Git(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        got = self.headers.get("Authorization", "")
        open(seen, "a").write(got + "\n")
        disk()
        if got != want:
            self.send_response(401)
            self.send_header("WWW-Authenticate", 'Basic realm="git"')
            self.end_headers()
            return
        path = os.path.join(root, self.path.split("?")[0].lstrip("/").split("/", 2)[-1])
        if not os.path.isfile(path):
            self.send_response(404); self.end_headers(); return
        body = open(path, "rb").read()
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *a): pass
server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Git)
tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
tls.load_cert_chain(cert, key)
server.socket = tls.wrap_socket(server.socket, server_side=True)
port = server.server_address[1]
threading.Thread(target=server.serve_forever, daemon=True).start()
def pipe(a, b):
    try:
        while True:
            data = a.recv(65536)
            if not data: break
            b.sendall(data)
    except OSError: pass
    for s in (a, b):
        try: s.shutdown(socket.SHUT_RDWR)
        except OSError: pass
def tunnel(c):
    head = b""
    while b"\r\n\r\n" not in head:
        part = c.recv(4096)
        if not part: return
        head += part
    c.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
    up = socket.create_connection(("127.0.0.1", port))
    threading.Thread(target=pipe, args=(c, up), daemon=True).start()
    pipe(up, c)
listener = socket.socket()
# As http.server does: the port may still be in TIME_WAIT from a run before.
listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
listener.bind(("127.0.0.1", int(connect))); listener.listen()
print("ready", flush=True)
while True:
    c, _ = listener.accept()
    threading.Thread(target=tunnel, args=(c,), daemon=True).start()
"#;

/// A sealed secret given to git: `lnk sandbox run` sets git's own
/// configuration in the run's variables, so `git clone https://<host>/...`
/// goes to the proxy, which adds the token as HTTP Basic's credentials
/// (`x-access-token:<token>`) and speaks TLS to the host. The clone works,
/// and the token is in neither `.git/config`, the run's variables, its
/// files, its output nor Link's settings while it runs.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_sealed_token_clones_with_git_and_stays_outside() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    // The secret module's bridge serves this route, found beside
    // lnk-sandbox: built there first, as a whole workspace's build would.
    let _bridge = crate::module("secret");
    const TOKEN: &str = "ghs_SEALEDgittoken123";
    let world = World::for_specs("git-clone");
    // The spec's files stay outside /tmp. Its home, with the machine
    // folder, is a separate short, private fixture so bridge sockets fit
    // sun_path even when this checkout's path is long.
    let settings = TempDir::short("run-git-settings");
    let config = settings.join(".lnk");
    let host = world.root.join("host");
    std::fs::create_dir_all(&host).unwrap();
    // A certificate authority this test trusts, and git.test's certificate.
    let ssl = |args: &[&str]| {
        let out = Command::new("openssl")
            .args(args)
            .current_dir(&host)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    ssl(&[
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        "ca.key",
        "-out",
        "ca.pem",
        "-days",
        "1",
        "-subj",
        "/CN=Link test CA",
    ]);
    ssl(&[
        "req",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        "git.key",
        "-out",
        "git.csr",
        "-subj",
        "/CN=git.test",
    ]);
    std::fs::write(host.join("ext"), "subjectAltName=DNS:git.test\n").unwrap();
    ssl(&[
        "x509",
        "-req",
        "-in",
        "git.csr",
        "-CA",
        "ca.pem",
        "-CAkey",
        "ca.key",
        "-CAcreateserial",
        "-out",
        "git.pem",
        "-days",
        "1",
        "-extfile",
        "ext",
    ]);
    // A repository, served as files.
    let git = |dir: &Path, args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    let source = world.root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("README"), "private\n").unwrap();
    git(&source, &["init", "-q", "-b", "main"]);
    git(&source, &["add", "README"]);
    git(&source, &["commit", "-q", "-m", "first"]);
    let bare = host.join("app.git");
    git(
        &world.root,
        &[
            "clone",
            "-q",
            "--bare",
            &source.display().to_string(),
            &bare.display().to_string(),
        ],
    );
    git(&bare, &["update-server-info"]);
    use base64::Engine;
    let want = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{TOKEN}"))
    );
    let seen = world.root.join("seen");
    let held = crate::proxy::free_port();
    let connect = held.number() as u16;
    let mut served = Command::new("python3")
        .args(["-c", GIT_HOST])
        .args([&bare, &host.join("git.pem"), &host.join("git.key")])
        .arg(&want)
        .arg(&seen)
        .arg(connect.to_string())
        .arg(&config)
        .arg(TOKEN)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(served.stdout.as_mut().unwrap()),
        &mut ready,
    )
    .unwrap();
    assert_eq!(ready.trim(), "ready");
    let spec = world.root.join("clone.toml");
    std::fs::write(
        &spec,
        format!(
            "schema = 1\nname = \"clone\"\nversion = 1\n\n[files]\nwrite = [\"{}\"]\n\n[internet]\nhosts = \
             [\"git.test\"]\n\n[commands]\ntoolchains = [\"shell\", \"git\"]\n",
            world.work.display()
        ),
    )
    .unwrap();
    let with = serde_json::json!({
        "secrets": [{
            "secret": {
                "name": "github-token",
                "value": TOKEN,
                "host": "git.test",
                "paths": [{"prefix": "", "rest": "path"}],
                "header": {"name": "Authorization", "value": "Basic {secret}", "basic": "x-access-token"},
                "methods": ["GET", "POST"]
            },
            "url": "LNK_SEALED_GITHUB_TOKEN_URL",
            "placeholder": "GITHUB_TOKEN",
            "git": true
        }]
    });
    let with_file = world.root.join("with.json");
    std::fs::write(&with_file, with.to_string()).unwrap();
    let script = format!(
        "git clone https://git.test/me/app.git \"{work}/app\" 2>&1\nenv > \"{work}/env\"",
        work = world.work.display()
    );
    let output = world
        .lnk(&[
            "run",
            "--spec",
            &spec.display().to_string(),
            "--upstream",
            &format!("http://127.0.0.1:{connect}"),
            "--yes",
            "--with-fd",
            "3",
            "--",
            "/bin/bash",
            "-c",
            &script,
        ])
        .env("HOME", &*settings)
        .env("LNK_TEST_CA", host.join("ca.pem"))
        .output_with_fd3(&with_file);
    let _ = served.kill();
    let _ = served.wait();
    let shown = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let wire = read(
        &link_files::layout::Hidden::Sandboxes
            .at(&config)
            .join("wire.log"),
    );
    assert!(output.status.success(), "{shown}");
    // It cloned, through the proxy, with the token as the host wants it.
    assert_eq!(
        read(&world.work.join("app/README")),
        "private\n",
        "{shown}\nthe host saw: {}\nthe proxy: {wire}",
        read(&seen)
    );
    assert!(read(&seen).lines().any(|l| l == want), "{}", read(&seen));
    // Nowhere in the run: its output, its variables, the clone's config.
    let encoded = want.trim_start_matches("Basic ");
    let config = read(&world.work.join("app/.git/config"));
    assert!(
        config.contains("url = https://git.test/me/app.git"),
        "{config}"
    );
    let env = read(&world.work.join("env"));
    assert!(env.contains("GIT_CONFIG_COUNT=2"), "{env}");
    for (what, text) in [("output", &shown), ("config", &config), ("env", &env)] {
        assert!(
            !text.contains(TOKEN) && !text.contains(encoded),
            "the token in the {what}: {text}"
        );
    }
    // Nor on the disk while it ran: its proxy was handed it on a pipe.
    assert_eq!(read(&seen.with_extension("disk")), "");
    let proxy = read(&seen.with_extension("proxy"));
    assert!(
        proxy.contains("--routes-handed LNK_SANDBOX_ROUTES") && !proxy.contains("--routes "),
        "{proxy}"
    );
    // Nothing of it under the clone.
    let found = Command::new("grep")
        .args(["-rlF", TOKEN])
        .arg(world.work.join("app"))
        .output()
        .unwrap();
    assert!(
        found.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&found.stdout)
    );
}

/// A run's proxy folder, in the system's temporary folder, is named so
/// no other user can make it first: never for the run's process or its
/// sandbox's name, whether its spec opens a network or a secret given it
/// needs a proxy.
#[test]
fn a_runs_proxy_folder_is_never_one_another_user_could_make_first() {
    if !link_test_kit::gate::bubblewrap() {
        return;
    }
    let _bridge = crate::module("secret");
    let world = World::for_specs("proxy-folder");
    let spec = world.root.join("run.toml");
    let with = serde_json::json!({
        "secrets": [{
            "secret": {
                "name": "stripe-key",
                "value": "sk_live_SEALED",
                "host": "api.stripe.com",
                "paths": [{"prefix": "", "rest": "path"}],
                "header": {"name": "Authorization", "value": "Bearer {secret}"},
                "methods": ["GET"]
            },
            "url": "LNK_SEALED_STRIPE_KEY_URL",
            "placeholder": "STRIPE_KEY"
        }]
    });
    let with_file = world.root.join("with.json");
    std::fs::write(&with_file, with.to_string()).unwrap();
    for network in ["", "[internet]\nhosts = [\"example.org\"]\n"] {
        std::fs::write(
            &spec,
            format!(
                "schema = 1\nname = \"run\"\nversion = 1\n\n[commands]\ntoolchains = [\"shell\"]\n\n{network}"
            ),
        )
        .unwrap();
        let output = world
            .lnk(&[
                "run",
                "--spec",
                &spec.display().to_string(),
                "--yes",
                "--with-fd",
                "3",
                "--",
                "/bin/ls",
                &world.short.join("tmp").display().to_string(),
            ])
            .output_with_fd3(&with_file);
        let listed = String::from_utf8_lossy(&output.stdout).to_string();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let folders: Vec<&str> = listed
            .lines()
            .filter(|l| l.starts_with("lnk-sandbox-"))
            .collect();
        assert!(!folders.is_empty(), "its proxy's folder: {listed}");
        for folder in folders {
            let suffix = &folder["lnk-sandbox-".len()..];
            assert!(
                suffix.len() >= 32 && suffix.chars().all(|c| c.is_ascii_hexdigit()),
                "{folder}: a name another user could make first ({network:?})"
            );
        }
    }
}

/// A link at `at` to `target` owned by a user other than root: this
/// user's own, or, run by root, handed to `nobody` (`lchown`, the link's
/// own owner). False where neither can be made.
fn non_root_link(target: &Path, at: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::os::unix::fs::symlink(target, at).unwrap();
    let owner = || std::fs::symlink_metadata(at).unwrap().uid();
    if owner() == 0 {
        let _ = std::os::unix::fs::lchown(at, Some(65534), None);
    }
    owner() != 0
}
