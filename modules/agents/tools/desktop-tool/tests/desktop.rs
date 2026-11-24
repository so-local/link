//! Link desktop over MCP on a display of its own: a window of this
//! test's, on it, seen in a screenshot and sent the tool's clicks,
//! typing, keys and scrolls as a person's; and a program without the
//! display's cookie refused. It needs `Xvfb` and the sandbox Xvfb runs
//! in (`lnk-sandbox`, built beside it): without either, its gate says so.
//! A program in `lnk sandbox run` reaches it as Link gives `run` its
//! display.

link_test_kit::program!(system);

use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use link_desktop_tool::{Config, Desktop, auth, mcp, png, socket};
use link_test_kit::TempDir;
use serde_json::{Value, json};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{self, ConnectionExt as _, CreateWindowAux, EventMask};
use x11rb::rust_connection::{DefaultStream, RustConnection};

/// `Xvfb` on PATH. None, said by the gate, where it isn't; with
/// `LNK_REQUIRE_PROGRAMS`, it must be.
fn xvfb() -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let found = std::env::split_paths(&path)
        .map(|dir| dir.join("Xvfb"))
        .find(|p| p.is_file());
    if found.is_none() {
        let gate = link_test_kit::gate::PROGRAMS;
        link_test_kit::gate::lacks("Xvfb", "it isn't on PATH", Some(gate));
    }
    found
}

/// A display number this test holds until it's dropped, for an Xvfb the
/// test starts after picking it: one no X server holds, kept from this
/// file's other tests and other runs of it ([`HeldNumber`]).
///
/// [`HeldNumber`]: link_test_kit::HeldNumber
fn free_display() -> link_test_kit::HeldNumber {
    link_test_kit::HeldNumber::first("X", 300..700, |number| {
        !socket(number).exists() && !PathBuf::from(format!("/tmp/.X{number}-lock")).exists()
    })
}

/// `lnk-sandbox`, built beside this test.
fn lnk_sandbox() -> PathBuf {
    link_test_kit::programs().join("lnk-sandbox")
}

/// The sandbox Xvfb runs in, as Link gives the desktop its own, with a
/// home in `dir`. None, said by the gate, where no sandbox works here.
fn xvfb_sandbox(dir: &std::path::Path) -> Option<Vec<String>> {
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let sandbox = lnk_sandbox();
    if !link_test_kit::gate::sandbox(&sandbox, &home) {
        return None;
    }
    Some(vec![
        "/usr/bin/env".into(),
        format!("HOME={}", home.display()),
        format!(
            "XDG_RUNTIME_DIR={}",
            link_test_kit::env::made_runtime(&home).display()
        ),
        sandbox.display().to_string(),
        "run".into(),
    ])
}

fn client(display: u32, cookie: &[u8]) -> anyhow::Result<RustConnection> {
    let stream = UnixStream::connect(socket(display))?;
    let (stream, _) = DefaultStream::from_unix_stream(stream)?;
    Ok(RustConnection::connect_to_stream_with_auth_info(
        stream,
        0,
        auth::NAME.to_vec(),
        cookie.to_vec(),
    )?)
}

fn call(desktop: &mut Desktop, name: &str, arguments: Value) -> Value {
    let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": name, "arguments": arguments}});
    let result = mcp::answer(desktop, &message.to_string()).unwrap()["result"].clone();
    assert!(result["isError"].is_null(), "{name}: {result}");
    result
}

/// How long the window's events may take to come.
const EVENTS: Duration = Duration::from_secs(5);

/// The events the window gets, waited for until `done` says they're
/// all there; any more already sent come with them. The tool's every
/// call waits for the display to have done it, so what it sent is on its
/// way when the call returns.
fn events(conn: &RustConnection, what: &str, done: impl Fn(&[Event]) -> bool) -> Vec<Event> {
    let mut got = Vec::new();
    link_test_kit::wait::eventually(what, EVENTS, || {
        while let Some(event) = conn.poll_for_event().unwrap() {
            got.push(event);
        }
        done(&got)
    });
    got
}

/// How many presses of a key `events` holds.
fn key_presses(got: &[Event]) -> usize {
    got.iter()
        .filter(|e| matches!(e, Event::KeyPress(_)))
        .count()
}

#[test]
fn a_window_is_seen_clicked_and_typed_into() {
    let Some(xvfb) = xvfb() else {
        return;
    };
    let dir = TempDir::new("desktop");
    let Some(sandbox) = xvfb_sandbox(&dir) else {
        return;
    };
    let held = free_display();
    let display = held.number();
    let cookie_file = dir.join("auth");
    auth::make(&cookie_file, display).unwrap();
    let made = auth::read(&cookie_file).unwrap();
    let mut desktop = Desktop::new(Config {
        display,
        auth: cookie_file.clone(),
        xvfb: Some(xvfb),
        sandbox,
    });

    // The first call starts the display: all of it black.
    let shot = call(&mut desktop, "screenshot", json!({}));
    // Its server took a new cookie, and the one made before opens nothing.
    let cookie = auth::read(&cookie_file).unwrap();
    assert_ne!(cookie, made);
    assert!(client(display, &made).is_err());
    // The fence's note first, naming it a screenshot, then the image.
    let note = shot["content"][0]["text"].as_str().unwrap_or_default();
    let fenced = link_untrusted::fenced(note).expect("the note fenced");
    assert_eq!(fenced.kind, "screenshot", "{note}");
    assert_eq!(shot["content"][1]["mimeType"], "image/png");

    // Without the cookie, nothing reaches it.
    assert!(client(display, &[0u8; 16]).is_err());

    // A red window of this test's, 400 by 300 at (100, 100), focused.
    let conn = client(display, &cookie).unwrap();
    let screen = &conn.setup().roots[0];
    let window = conn.generate_id().unwrap();
    conn.create_window(
        screen.root_depth,
        window,
        screen.root,
        100,
        100,
        400,
        300,
        0,
        xproto::WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new()
            .background_pixel(0xff0000)
            .override_redirect(1)
            .event_mask(EventMask::KEY_PRESS | EventMask::BUTTON_PRESS | EventMask::EXPOSURE),
    )
    .unwrap();
    conn.map_window(window).unwrap();
    conn.flush().unwrap();
    events(&conn, "the window shown", |got| {
        got.iter().any(|e| matches!(e, Event::Expose(_)))
    });
    conn.set_input_focus(xproto::InputFocus::PARENT, window, 0u32)
        .unwrap();
    conn.flush().unwrap();

    // Seen.
    let shot = call(&mut desktop, "screenshot", json!({}));
    use base64::Engine;
    let png = base64::engine::general_purpose::STANDARD
        .decode(shot["content"][1]["data"].as_str().unwrap())
        .unwrap();
    let (width, height, rgb) = png::decode(&png).unwrap();
    assert_eq!((width, height), (1280, 800));
    let pixel = |x: usize, y: usize| {
        let at = (y * width as usize + x) * 3;
        [rgb[at], rgb[at + 1], rgb[at + 2]]
    };
    assert_eq!(pixel(150, 150), [255, 0, 0]);
    assert_eq!(pixel(10, 10), [0, 0, 0]);

    // Clicked where it was told, in the window's own pixels.
    call(&mut desktop, "click", json!({"coordinate": [200, 150]}));
    let clicks: Vec<(u8, i16, i16)> = events(&conn, "a click", |got| {
        got.iter().any(|e| matches!(e, Event::ButtonPress(_)))
    })
    .into_iter()
    .filter_map(|e| match e {
        Event::ButtonPress(b) => Some((b.detail, b.event_x, b.event_y)),
        _ => None,
    })
    .collect();
    assert_eq!(clicks, [(1, 100, 50)]);
    call(
        &mut desktop,
        "click",
        json!({"coordinate": [200, 150], "button": "right", "double": true}),
    );
    let right = |e: &Event| matches!(e, Event::ButtonPress(b) if b.detail == 3);
    let clicks = events(&conn, "a double right click", |got| {
        got.iter().filter(|e| right(e)).count() >= 2
    })
    .into_iter()
    .filter(right)
    .count();
    assert_eq!(clicks, 2);

    // Typed: each character's key, shifted where it must be, read back
    // as the window's keyboard mapping gives them. A character the keymap
    // lacks borrowed a spare key only while it was typed, so its key
    // reads as none now (`\0`).
    call(&mut desktop, "type", json!({"text": "Hi! é"}));
    let setup = conn.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    // Five characters, and a shift for the capital and the `!`.
    let presses: Vec<(u8, u16)> =
        events(&conn, "five characters typed", |got| key_presses(got) >= 7)
            .into_iter()
            .filter_map(|e| match e {
                Event::KeyPress(k) => Some((k.detail, u16::from(k.state))),
                _ => None,
            })
            .collect();
    let mapping = conn
        .get_keyboard_mapping(min, max - min + 1)
        .unwrap()
        .reply()
        .unwrap();
    let per = mapping.keysyms_per_keycode as usize;
    let shift = u16::from(xproto::KeyButMask::SHIFT);
    let typed: String = presses
        .iter()
        .filter_map(|(code, state)| {
            let syms = &mapping.keysyms[(code - min) as usize * per..][..per];
            let sym = if state & shift != 0 { syms[1] } else { syms[0] };
            // The shift keys themselves type nothing.
            (sym < 0xff00).then(|| char::from_u32(sym).unwrap())
        })
        .collect();
    assert_eq!(typed, "Hi! \0");

    // Characters the keymap lacks, more than it has spare keys: each
    // borrows one only while it's typed, so the fortieth types as the
    // first did, and none is left borrowed.
    let spare = |conn: &RustConnection| {
        let mapping = conn
            .get_keyboard_mapping(min, max - min + 1)
            .unwrap()
            .reply()
            .unwrap();
        mapping
            .keysyms
            .chunks(mapping.keysyms_per_keycode as usize)
            .filter(|syms| syms.iter().all(|s| *s == 0))
            .count()
    };
    let before = spare(&conn);
    let cyrillic: String = ('а'..='я').chain('А'..='Й').take(40).collect();
    assert_eq!(cyrillic.chars().count(), 40);
    let typed = call(&mut desktop, "type", json!({"text": cyrillic}));
    assert_ne!(typed["isError"], true, "{typed}");
    let pressed = key_presses(&events(&conn, "forty characters typed", |got| {
        key_presses(got) >= 40
    }));
    assert_eq!(pressed, 40);
    assert_eq!(spare(&conn), before);

    // A combination: held in order.
    call(&mut desktop, "key", json!({"text": "ctrl+c"}));
    let control = u16::from(xproto::KeyButMask::CONTROL);
    let held = events(&conn, "ctrl+c pressed", |got| key_presses(got) >= 2)
        .into_iter()
        .any(|e| match e {
            Event::KeyPress(k) => {
                let syms = &mapping.keysyms[(k.detail - min) as usize * per..][..per];
                syms[0] == 'c' as u32 && u16::from(k.state) & control != 0
            }
            _ => false,
        });
    assert!(held, "ctrl+c");
    // A capital among a combination's keys is its key, shift not held.
    call(&mut desktop, "key", json!({"text": "ctrl+V"}));
    let pressed: Vec<(u32, u16)> = events(&conn, "ctrl+V pressed", |got| key_presses(got) >= 2)
        .into_iter()
        .filter_map(|e| match e {
            Event::KeyPress(k) => {
                let syms = &mapping.keysyms[(k.detail - min) as usize * per..][..per];
                Some((syms[0], u16::from(k.state)))
            }
            _ => None,
        })
        .collect();
    let v = pressed.iter().find(|(sym, _)| *sym == 'v' as u32);
    assert!(
        v.is_some_and(|(_, state)| state & control != 0 && state & shift == 0),
        "{pressed:?}"
    );

    // Scrolled: the wheel's buttons, at the place given.
    call(
        &mut desktop,
        "scroll",
        json!({"coordinate": [300, 200], "scroll_direction": "down", "scroll_amount": 2}),
    );
    let down = |e: &Event| matches!(e, Event::ButtonPress(b) if b.detail == 5 && b.event_x == 200);
    let wheel = events(&conn, "two clicks of the wheel", |got| {
        got.iter().filter(|e| down(e)).count() >= 2
    })
    .into_iter()
    .filter(down)
    .count();
    assert_eq!(wheel, 2);

    // Off the desktop: refused.
    let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "click", "arguments": {"coordinate": [5000, 1]}}});
    let off = mcp::answer(&mut desktop, &message.to_string()).unwrap();
    assert_eq!(off["result"]["isError"], true, "{off}");

    // Its X server ended under it: the next call starts another, and
    // says what ran on the display is gone; the one after doesn't.
    #[cfg(target_os = "linux")]
    {
        server_ends(&conn);
        let next = call(&mut desktop, "screenshot", json!({}));
        let said = next["content"][0]["text"].as_str().unwrap_or_default();
        assert!(said.contains("started again"), "{next}");
        let after = call(&mut desktop, "screenshot", json!({}));
        let said = after["content"][0]["text"].as_str().unwrap_or_default();
        assert!(!said.contains("started again"), "{after}");
    }
    drop(conn);

    // The display ends with the tool.
    drop(desktop);
    link_test_kit::wait::eventually("the display's socket gone", EVENTS, || {
        !socket(display).exists()
    });
}

/// Ends the X server at the other end of `conn`, as a crash would: the
/// process the kernel names behind its socket, and waits for its end.
#[cfg(target_os = "linux")]
fn server_ends(conn: &RustConnection) {
    use std::os::fd::AsRawFd;
    let fd = conn.stream().as_raw_fd();
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut size = size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: a live socket, and one ucred of the size given.
    let got = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credentials).cast(),
            &raw mut size,
        )
    };
    assert_eq!(got, 0, "{}", std::io::Error::last_os_error());
    assert!(credentials.pid > 1);
    // SAFETY: a signal to the server's process, named by the kernel.
    unsafe { libc::kill(credentials.pid, libc::SIGKILL) };
    link_test_kit::wait::eventually("the X server ended", EVENTS, || {
        !conn.get_input_focus().is_ok_and(|c| c.reply().is_ok())
    });
}

/// Run inside a sandbox by the test below: connects to `$DISPLAY` with
/// `$XAUTHORITY`'s cookie, as any X program does, and says whether it
/// could. Outside that test it does nothing.
#[test]
fn an_x_client_inside() {
    if std::env::var_os("LNK_TEST_X_CLIENT").is_none() {
        return;
    }
    match x11rb::connect(None) {
        Ok(_) => println!("CONNECTED"),
        Err(err) => println!("REFUSED {err}"),
    }
}

/// A program `run` starts, in `lnk sandbox run` with what Link gives it
/// for the agent's desktop: that display's socket and cookie read, and
/// `DISPLAY` and `XAUTHORITY` set by `env`. It reaches the display; with
/// another cookie it's refused.
#[cfg(target_os = "linux")]
#[test]
fn a_sandboxed_program_reaches_the_display_with_its_cookie() {
    let Some(xvfb) = xvfb() else {
        return;
    };
    let dir = TempDir::new("desktop-run");
    let home = dir.join("home");
    let Some(xvfb_sandbox) = xvfb_sandbox(&dir) else {
        return;
    };
    let sandbox = lnk_sandbox();
    let held = free_display();
    let display = held.number();
    let cookie = dir.join("auth");
    auth::make(&cookie, display).unwrap();
    let mut desktop = Desktop::new(Config {
        display,
        auth: cookie.clone(),
        xvfb: Some(xvfb),
        sandbox: xvfb_sandbox,
    });
    call(&mut desktop, "screenshot", json!({}));
    let other = dir.join("other");
    auth::make(&other, display).unwrap();
    let me = std::env::current_exe().unwrap();
    let inside = |given: &std::path::Path| -> String {
        let mut command = link_test_kit::env::command(&sandbox, &home);
        command
            .env_clear()
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(&home))
            .env("PATH", "/usr/bin:/bin")
            .args([
                "run",
                "--commands.allow",
                "/usr/bin/env",
                "--commands.allow",
            ])
            .arg(&me)
            .arg("--files.read")
            .arg(socket(display))
            .arg("--files.read")
            .arg(given)
            .arg("--files.read")
            .arg(&me)
            .args(["--", "/usr/bin/env", "LNK_TEST_X_CLIENT=1"])
            .arg(format!("DISPLAY=:{display}"))
            .arg(format!("XAUTHORITY={}", given.display()))
            .arg(&me)
            .args([
                "--exact",
                "an_x_client_inside",
                "--nocapture",
                "--test-threads=1",
            ]);
        let out = link_test_kit::Deadline::new("the X client in its sandbox", EVENTS * 6)
            .output(&mut command);
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let said = inside(&cookie);
    assert!(said.contains("CONNECTED"), "{said}");
    let said = inside(&other);
    assert!(said.contains("REFUSED"), "{said}");
}
