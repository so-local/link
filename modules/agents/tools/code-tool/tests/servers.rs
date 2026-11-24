//! Link code against a real language server: a small Rust project with
//! an error, through `rust-analyzer` under `lnk sandbox run` with no
//! network, as Link defines it. Its diagnostic, a definition and its
//! references, by place and by symbol. Skipped, saying why, where this
//! machine has no `rust-analyzer` (which `LNK_REQUIRE_PROGRAMS` makes a
//! failure) or the sandbox doesn't run (which `LNK_REQUIRE_SANDBOX` does).
//! The same for Java through `jdtls` and Kotlin through
//! `kotlin-language-server`, each where this machine has it and a JDK.

#![cfg(target_os = "linux")]

link_test_kit::program!(system);

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use link_code_tool::{Code, Config, answer};
use link_test_kit::{TempDir, gate};
use serde_json::{Value, json};

/// `lnk-sandbox`, among the suite's declared programs.
fn lnk_sandbox() -> PathBuf {
    link_test_kit::programs().join("lnk-sandbox")
}

struct Toolchain {
    read: Vec<PathBuf>,
    path: String,
}

/// Where this machine's toolchain is, for the sandbox to read: the
/// folders of `rust-analyzer` and of the sysroot (rustup's, when that's
/// how it's installed). None, said by the gate, without a `rust-analyzer`
/// that runs as the server will: in `project`, without the
/// `RUSTUP_TOOLCHAIN` Cargo gives this test, since rustup picks a
/// toolchain by those and the checkout's pinned one can have it where the
/// machine's default doesn't.
#[expect(
    clippy::disallowed_methods,
    reason = "probes the host's selected toolchain before confining its language server"
)]
fn toolchain(project: &Path) -> Option<Toolchain> {
    let runs = |program: &str, args: &[&str]| {
        let ran = std::process::Command::new(program)
            .args(args)
            .current_dir(project)
            .env_remove("RUSTUP_TOOLCHAIN")
            .output();
        let why = match ran {
            Ok(ran) if ran.status.success() => return Some(ran),
            Ok(ran) => format!(
                "`{program} {}` in the project, without RUSTUP_TOOLCHAIN, {}: {}",
                args.join(" "),
                ran.status,
                String::from_utf8_lossy(&ran.stderr).trim()
            ),
            Err(err) => format!("`{program}`: {err}"),
        };
        gate::lacks(program, why, Some(gate::PROGRAMS));
        None
    };
    runs("rust-analyzer", &["--version"])?;
    let path = std::env::var("PATH").unwrap_or_default();
    let Some(found) = link_harness_contract::harness::find_program("rust-analyzer", &path) else {
        gate::lacks("rust-analyzer", "it isn't on PATH", Some(gate::PROGRAMS));
        return None;
    };
    let sysroot = runs("rustc", &["--print", "sysroot"])?;
    let sysroot = PathBuf::from(String::from_utf8_lossy(&sysroot.stdout).trim());
    // The sandbox drops rustup's environment inputs. Run this selected
    // toolchain directly, including the cargo and rustc its server asks.
    let path = std::env::join_paths(
        std::iter::once(sysroot.join("bin")).chain(std::env::split_paths(&path)),
    )
    .unwrap()
    .into_string()
    .unwrap();
    let mut read = vec![found.parent()?.to_path_buf()];
    // rustup's toolchains folder, which its proxies read, or the sysroot.
    read.push(match sysroot.parent().and_then(Path::parent) {
        Some(rustup) if rustup.join("toolchains").is_dir() => rustup.to_path_buf(),
        _ => sysroot,
    });
    Some(Toolchain { read, path })
}

fn call(code: &Code, name: &str, arguments: Value) -> String {
    let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": name, "arguments": arguments}});
    let result = answer(code, &message.to_string()).unwrap()["result"].clone();
    let raw = result["content"][0]["text"].as_str().unwrap_or("");
    // What a server or a file says is fenced: diagnostics as such, a
    // place's line as a file's.
    if let Some(fenced) = link_untrusted::fenced(raw) {
        let kind = if name == "diagnostics" {
            "diagnostics"
        } else {
            "file"
        };
        assert_eq!(fenced.kind, kind, "{raw}");
    }
    let text = link_untrusted::unfenced(raw);
    assert_ne!(result["isError"], true, "{name}: {text}");
    text
}

#[test]
fn rust_analyzer_answers_in_its_sandbox() {
    let dir = TempDir::new("code-rust");
    let files = dir.join("files");
    let project = files.join("work/demo");
    std::fs::create_dir_all(project.join("src")).unwrap();
    let Some(toolchain) = toolchain(&project) else {
        return;
    };
    let program = lnk_sandbox();
    // Probed in a home of its own, never the shared temporary folder's.
    let probe = link_test_kit::TempDir::new("sandbox-probe");
    if !gate::sandbox(&program, &probe) {
        return;
    }
    std::fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(
        project.join("src/lib.rs"),
        "pub fn helper() -> u32 {\n    1\n}\n\npub fn caller() -> u32 {\n    helper() + helper()\n}\n\npub fn broken() -> u32 {\n    let x: u32 = ;\n    x\n}\n\n\
         pub struct HelperBox;\n\
         pub struct A;\nimpl A {\n    pub fn new() -> A {\n        A\n    }\n}\n\
         pub struct B;\nimpl B {\n    pub fn new() -> B {\n        B\n    }\n}\n",
    )
    .unwrap();
    let mut sandbox = vec![
        program.display().to_string(),
        "run".into(),
        "--files.write".into(),
        files.display().to_string(),
        "--commands.toolchains".into(),
        "shell".into(),
    ];
    sandbox.extend(["--commands.toolchains".into(), "rust".into()]);
    // The toolchain it reads runs too: rust-analyzer, and the cargo and
    // rustc it asks, by the folders they're in.
    for folder in toolchain.read {
        sandbox.extend(["--files.read".into(), folder.display().to_string()]);
        sandbox.extend(["--commands.allow".into(), format!("{}/", folder.display())]);
    }
    let code = Code::new(Config {
        env: Vec::new(),
        files: files.clone(),
        path: toolchain.path,
        sandbox,
        // Link's own server, with its settings: every symbol searched.
        servers: BTreeMap::from([(
            "rust".into(),
            link_agent_spec::link().tool("code").servers["rust"].clone(),
        )]),
        unavailable: BTreeMap::new(),
        timeout: Duration::from_secs(180),
    })
    .unwrap();
    let file = "work/demo/src/lib.rs";
    let diagnostics = call(&code, "diagnostics", json!({"file_path": file}));
    assert!(
        diagnostics
            .lines()
            .any(|l| l.starts_with("work/demo/src/lib.rs:10:") && l.contains("error")),
        "{diagnostics}"
    );
    // `helper` called in `caller`, on line 6.
    let definition = call(
        &code,
        "definition",
        json!({"file_path": file, "line": 6, "character": 6}),
    );
    assert_eq!(
        definition,
        "work/demo/src/lib.rs:1:8: pub fn helper() -> u32 {"
    );
    let references = call(
        &code,
        "references",
        json!({"file_path": file, "line": 1, "character": 9}),
    );
    assert_eq!(references.lines().count(), 3, "{references}");
    let by_name = call(
        &code,
        "definition",
        json!({"symbol": "helper", "file_path": file}),
    );
    // A function found by name though a type's name is close.
    assert_eq!(by_name, definition);
    // Two named `new`: both definitions listed, and references refused,
    // naming both, rather than one chosen.
    let news = call(
        &code,
        "definition",
        json!({"symbol": "new", "file_path": file}),
    );
    assert!(news.contains("2 symbols are named new"), "{news}");
    assert_eq!(
        news.lines().filter(|l| l.contains("pub fn new")).count(),
        2,
        "{news}"
    );
    let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "references", "arguments": {"symbol": "new", "file_path": file}}});
    let refused = answer(&code, &message.to_string()).unwrap()["result"].clone();
    assert_eq!(refused["isError"], true, "{refused}");
    let said = refused["content"][0]["text"].as_str().unwrap();
    assert!(
        said.contains("2 symbols are named new") && said.contains("line and character"),
        "{said}"
    );
    code.stop();
}

/// Where a JVM language server's launcher is, for the sandbox to read and
/// run: the folder it's installed in, its link followed. None, said by
/// the gate, where `program` isn't on `PATH`, which few machines have, so
/// `LNK_REQUIRE_PROGRAMS` doesn't require it, or there's no JDK beside it.
fn jvm_server(program: &str) -> Option<PathBuf> {
    let path = std::env::var("PATH").unwrap_or_default();
    let Some(found) = link_harness_contract::harness::find_program(program, &path) else {
        gate::lacks(program, "it isn't on PATH", None);
        return None;
    };
    if !gate::program("java", &["-version"]) {
        return None;
    }
    // The installed tree: `bin/<launcher>`'s parent's parent.
    let found = std::fs::canonicalize(found).ok()?;
    Some(found.parent()?.parent()?.to_path_buf())
}

/// Link's `language` server, its install at `server`, asked for the
/// diagnostics of `file` under `files`, in a sandbox running the shell,
/// Python and JVM toolchains and reading the install.
fn jvm_code(language: &str, server: &Path, files: &Path) -> Option<Code> {
    let program = lnk_sandbox();
    let probe = link_test_kit::TempDir::new("sandbox-probe");
    if !gate::sandbox(&program, &probe) {
        return None;
    }
    let mut sandbox = vec![
        program.display().to_string(),
        "run".into(),
        "--files.write".into(),
        files.display().to_string(),
    ];
    for toolchain in ["shell", "python", "jvm"] {
        sandbox.extend(["--commands.toolchains".into(), toolchain.into()]);
    }
    sandbox.extend(["--files.read".into(), server.display().to_string()]);
    sandbox.extend(["--commands.allow".into(), format!("{}/", server.display())]);
    let path = std::env::join_paths(std::iter::once(server.join("bin")).chain(
        std::env::split_paths(&std::env::var("PATH").unwrap_or_default()),
    ))
    .unwrap()
    .into_string()
    .unwrap();
    Some(
        Code::new(Config {
            env: Vec::new(),
            files: files.to_path_buf(),
            path,
            sandbox,
            servers: BTreeMap::from([(
                language.into(),
                link_agent_spec::link().tool("code").servers[language].clone(),
            )]),
            unavailable: BTreeMap::new(),
            timeout: Duration::from_secs(300),
        })
        .unwrap(),
    )
}

#[test]
fn jdtls_answers_in_its_sandbox() {
    let Some(server) = jvm_server("jdtls") else {
        return;
    };
    let dir = TempDir::new("code-java");
    let files = dir.join("files");
    let project = files.join("demo");
    let source = project.join("src/main/java/demo");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        project.join("pom.xml"),
        "<project xmlns=\"http://maven.apache.org/POM/4.0.0\">\n  <modelVersion>4.0.0</modelVersion>\n  \
         <groupId>demo</groupId>\n  <artifactId>demo</artifactId>\n  <version>0.1.0</version>\n  \
         <properties><maven.compiler.release>21</maven.compiler.release></properties>\n</project>\n",
    )
    .unwrap();
    std::fs::write(
        source.join("Demo.java"),
        "package demo;\n\npublic class Demo {\n    static int helper() {\n        return 1;\n    }\n\n    \
         static int caller() {\n        return helper() + helper();\n    }\n\n    \
         static int broken() {\n        int x = ;\n        return x;\n    }\n}\n",
    )
    .unwrap();
    let Some(code) = jvm_code("java", &server, &files) else {
        return;
    };
    let file = "demo/src/main/java/demo/Demo.java";
    let diagnostics = call(&code, "diagnostics", json!({"file_path": file}));
    assert!(
        diagnostics
            .lines()
            .any(|l| l.starts_with(&format!("{file}:13:"))),
        "{diagnostics}"
    );
    // `helper` called in `caller`, on line 9.
    let definition = call(
        &code,
        "definition",
        json!({"file_path": file, "line": 9, "character": 16}),
    );
    assert!(
        definition.starts_with(&format!("{file}:4:")),
        "{definition}"
    );
    let references = call(
        &code,
        "references",
        json!({"file_path": file, "line": 4, "character": 16}),
    );
    assert!(references.lines().count() >= 2, "{references}");
    code.stop();
}

#[test]
fn kotlin_language_server_answers_in_its_sandbox() {
    let Some(server) = jvm_server("kotlin-language-server") else {
        return;
    };
    let dir = TempDir::new("code-kotlin");
    let files = dir.join("files");
    let project = files.join("demo");
    let source = project.join("src/main/kotlin/demo");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        project.join("settings.gradle.kts"),
        "rootProject.name = \"demo\"\n",
    )
    .unwrap();
    std::fs::write(
        source.join("Demo.kt"),
        "package demo\n\nfun helper(): Int {\n    return 1\n}\n\nfun caller(): Int {\n    \
         return helper() + helper()\n}\n\nfun broken(): Int {\n    val x: Int = \"text\"\n    return x\n}\n",
    )
    .unwrap();
    let Some(code) = jvm_code("kotlin", &server, &files) else {
        return;
    };
    let file = "demo/src/main/kotlin/demo/Demo.kt";
    let diagnostics = call(&code, "diagnostics", json!({"file_path": file}));
    assert!(
        diagnostics
            .lines()
            .any(|l| l.starts_with(&format!("{file}:12:"))),
        "{diagnostics}"
    );
    // `helper` called in `caller`, on line 8.
    let definition = call(
        &code,
        "definition",
        json!({"file_path": file, "line": 8, "character": 12}),
    );
    assert!(
        definition.starts_with(&format!("{file}:3:")),
        "{definition}"
    );
    code.stop();
}
