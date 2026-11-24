//! Link Code's own units: its files, places, and answers.

use super::*;

/// Each declared call says the languages served and names what it
/// works on in its title.
#[test]
fn the_declared_tools_say_what_they_serve() {
    let files = link_test_kit::TempDir::new("code-declared");
    let tools = offered(&code(files.path())).unwrap();
    let names: Vec<&str> = tools
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["definition", "diagnostics", "references"]);
    assert!(!tools.to_string().contains("{languages}"), "{tools}");
    assert_eq!(
        tools[0]["_meta"]["link.local/summary"],
        json!(["file_path", "line", "symbol"])
    );
}

/// A server gets the variables `--var` names, set inside its sandbox,
/// which clears the environment, so it sees what `run` fetched;
/// with none, its line is as before.
#[test]
fn a_servers_variables_are_set_inside_its_sandbox() {
    let mut config = Config {
        files: PathBuf::from("/files"),
        path: "/usr/bin".into(),
        sandbox: vec!["lnk".into(), "sandbox".into(), "run".into()],
        servers: BTreeMap::new(),
        unavailable: BTreeMap::new(),
        timeout: Duration::from_secs(1),
        env: Vec::new(),
    };
    let program = Path::new("/usr/bin/rust-analyzer");
    assert_eq!(
        server_argv(&config, program, &[]),
        ["lnk", "sandbox", "run", "--", "/usr/bin/rust-analyzer"]
    );
    config.env = vec![("CARGO_HOME".into(), "/caches/cargo".into())];
    assert_eq!(
        server_argv(&config, program, &["--stdio".into()]),
        [
            "lnk",
            "sandbox",
            "run",
            "--",
            env_program(),
            "CARGO_HOME=/caches/cargo",
            "/usr/bin/rust-analyzer",
            "--stdio"
        ]
    );
}

#[test]
fn columns_count_characters_and_servers_utf16_units() {
    let text = "fn main() {\n    let é = \"😀x\";\n}\n";
    assert_eq!(
        position_of(text, 1, 4).unwrap(),
        json!({"line": 0, "character": 3})
    );
    // `x` is the 15th character of line 2, after a two-unit emoji.
    assert_eq!(
        position_of(text, 2, 15).unwrap(),
        json!({"line": 1, "character": 15})
    );
    assert_eq!(chars_of("    let é = \"😀x\";", 15), 14);
    assert!(position_of(text, 4, 1).is_err());
    assert!(position_of(text, 0, 1).is_err());
    // Just past a line's end names it; further is refused, never
    // clamped.
    assert!(position_of(text, 1, 12).is_ok());
    let past = format!("{:#}", position_of(text, 1, 13).unwrap_err());
    assert!(past.contains("line 1 has 11 characters, not 13"), "{past}");
}

fn code(files: &Path) -> Code {
    let server = |command: &str, files: &[&str], roots: &[&str]| CodeServer {
        command: Some(vec![command.into()]),
        files: Some(files.iter().map(|f| f.to_string()).collect()),
        roots: Some(roots.iter().map(|f| f.to_string()).collect()),
        settings: Default::default(),
    };
    Code::new(Config {
        env: Vec::new(),
        files: files.to_path_buf(),
        path: "/usr/bin:/bin".into(),
        sandbox: Vec::new(),
        servers: BTreeMap::from([
            (
                "rust".into(),
                server("rust-analyzer", &["rs"], &["Cargo.toml"]),
            ),
            ("go".into(), server("gopls", &["go"], &["go.mod"])),
        ]),
        unavailable: BTreeMap::from([("go".into(), "this machine doesn't have it".into())]),
        timeout: Duration::from_secs(5),
    })
    .unwrap()
}

#[test]
fn a_file_is_asked_about_only_in_the_files_folder_with_a_server() {
    let dir = link_test_kit::TempDir::new("code-asked");
    let files = dir.join("files");
    std::fs::create_dir_all(files.join("work/app/src")).unwrap();
    std::fs::write(files.join("work/app/Cargo.toml"), "").unwrap();
    std::fs::write(files.join("work/app/src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(files.join("main.go"), "package main\n").unwrap();
    std::fs::write(files.join("notes.txt"), "").unwrap();
    std::fs::write(dir.join("outside.rs"), "").unwrap();
    std::os::unix::fs::symlink(dir.join("outside.rs"), files.join("link.rs")).unwrap();
    let code = code(&files);
    let main = asked(&code, "work/app/src/main.rs").unwrap();
    assert_eq!(main.language, "rust");
    assert_eq!(
        root_of(&code, &main.path, &main.server),
        code.files.join("work/app")
    );
    let refused = |name: &str| format!("{:#}", asked(&code, name).err().unwrap());
    assert!(refused("../outside.rs").contains("isn't in your files folder"));
    assert!(refused("link.rs").contains("isn't in your files folder"));
    assert!(refused("nowhere.rs").contains("no file"));
    assert!(refused("notes.txt").contains("no language server serves .txt"));
    let go = refused("main.go");
    assert!(
        go.contains("would run gopls") && go.contains("doesn't have it"),
        "{go}"
    );
    // A file with no root above it: the files folder.
    std::fs::write(files.join("loose.rs"), "").unwrap();
    let loose = asked(&code, "loose.rs").unwrap();
    assert_eq!(root_of(&code, &loose.path, &loose.server), code.files);
}

/// A file is read by its handle from the files folder's, never through
/// a link on its way, whatever a check of its path found.
#[test]
fn a_file_is_read_through_no_link() {
    let dir = link_test_kit::TempDir::new("code-beneath");
    let files = dir.join("files");
    std::fs::create_dir_all(files.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("away")).unwrap();
    std::fs::write(files.join("src/a.rs"), "inside\n").unwrap();
    std::fs::write(dir.join("away/a.rs"), "a secret\n").unwrap();
    let code = code(&files);
    assert_eq!(
        read_beneath(&code, &code.files.join("src/a.rs")).unwrap(),
        "inside\n"
    );
    // A folder swapped for a link after its path was checked.
    std::fs::rename(files.join("src"), files.join("was")).unwrap();
    std::os::unix::fs::symlink(dir.join("away"), files.join("src")).unwrap();
    let err = read_beneath(&code, &code.files.join("src/a.rs")).unwrap_err();
    assert!(format!("{err:#}").contains("link"), "{err:#}");
    assert!(text_of(&code, &code.files.join("src/a.rs")).is_none());
    // Outside the files folder, or climbing out of it, never read.
    assert!(read_beneath(&code, &dir.join("away/a.rs")).is_err());
    assert!(read_beneath(&code, &code.files.join("../away/a.rs")).is_err());
}

/// A path outside the files folder is shown from the home folder, its
/// user's name left out.
#[test]
fn a_path_outside_is_shown_from_home() {
    let dir = link_test_kit::TempDir::new("code-home");
    let mut code = code(dir.path());
    code.home = Some(PathBuf::from("/home/someone"));
    let toolchain = Path::new("/home/someone/.rustup/toolchains/std/lib.rs");
    assert_eq!(shown(&code, toolchain), "~/.rustup/toolchains/std/lib.rs");
    assert_eq!(shown(&code, Path::new("/usr/lib/x.h")), "/usr/lib/x.h");
    assert_eq!(shown(&code, &code.files.join("a.rs")), "a.rs");
}

/// An answer lists places until its bytes run out, counting the rest,
/// and says when its server hadn't settled.
#[test]
fn an_answer_is_bounded_and_says_whether_it_is_all() {
    let long = "x".repeat(limits().line_max);
    let places: Vec<Place> = (0..limits().listed_max as u64)
        .map(|n| Place {
            path: "a.rs".into(),
            line: n + 1,
            column: 1,
            text: Some(long.clone()),
        })
        .collect();
    let (text, value) = answer_places("references", places, false, None).unwrap();
    let listed = value["places"].as_array().unwrap().len();
    assert!(listed < limits().listed_max, "{listed}");
    assert_eq!(value["total"], limits().listed_max);
    assert_eq!(value["complete"], false);
    assert!(text.len() < limits().answer_max + 1024, "{}", text.len());
    assert!(
        text.contains(&format!("…and {} more", limits().listed_max - listed)),
        "{text}"
    );
    assert!(text.contains("may not be all"), "{text}");
    // A long message is cut to its most.
    let message = "m".repeat(limits().message_max * 2);
    assert_eq!(
        capped(&message, limits().message_max).chars().count(),
        limits().message_max + 1
    );
}

#[test]
fn a_line_is_shown_only_from_the_files_folder() {
    let dir = link_test_kit::TempDir::new("code-shown");
    let files = dir.join("files");
    std::fs::create_dir_all(&files).unwrap();
    std::fs::write(files.join("a.rs"), "one\n  two  \n").unwrap();
    std::fs::write(dir.join("secret.rs"), "a secret\n").unwrap();
    let code = code(&files);
    let at = json!({"line": 1, "character": 2});
    let inside = place_of(&code, &lsp::uri_of(&code.files.join("a.rs")), &at).unwrap();
    assert_eq!(inside.line(), "a.rs:2:3: two");
    let outside = place_of(&code, &lsp::uri_of(&dir.join("secret.rs")), &at).unwrap();
    assert!(outside.text.is_none());
    assert!(!outside.line().contains("secret\n"));
}
