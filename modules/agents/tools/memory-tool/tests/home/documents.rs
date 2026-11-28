//! Documents in Home by their text: PDFs and Word documents parsed by
//! Link Document Converter in a child holding no secret, and only `memory/`
//! indexed.

use super::*;

/// A PDF and a Word document in `memory/` are found by a sentence inside
/// them; one whose text can't be had, by its name, and status says why.
#[test]
fn a_pdf_and_a_word_document_are_found_by_their_text() {
    let dir = TempDir::new("home-documents");
    std::fs::create_dir_all(dir.join("memory/taxes")).unwrap();
    std::fs::copy(
        Path::new(FIXTURES).join("refund.pdf"),
        dir.join("memory/taxes/refund.pdf"),
    )
    .unwrap();
    std::fs::copy(
        Path::new(FIXTURES).join("lease.docx"),
        dir.join("memory/lease.docx"),
    )
    .unwrap();
    std::fs::write(dir.join("memory/scan.pdf"), b"%PDF-1.4 not really").unwrap();
    let mut index = Index::open(&dir, None).unwrap();
    index.extract_with(extractor());
    index.refresh().unwrap();
    assert_eq!(
        paths(&index, "accountant refund"),
        ["memory/taxes/refund.pdf"]
    );
    assert_eq!(paths(&index, "landlord deposit"), ["memory/lease.docx"]);
    assert_eq!(paths(&index, "scan"), ["memory/scan.pdf"]);
    let status = serde_json::to_value(index.status().unwrap()).unwrap();
    let notices = status["notices"].to_string();
    assert!(
        notices.contains("memory/scan.pdf is found by its name only"),
        "{notices}"
    );
    // The files stay as they are: a read gives the text indexed.
    let read = index
        .read("memory/lease.docx", 1, None, None)
        .unwrap()
        .unwrap();
    assert!(
        serde_json::to_string(&read)
            .unwrap()
            .contains("renewed by the first of March")
    );
}

/// In the service, which isn't confined, the parser runs in `lnk sandbox
/// run` with nothing granted but its own program: no network, no folder
/// of the user's, and given only HOME. It still reads a document from its
/// stdin.
#[cfg(target_os = "linux")]
#[test]
fn the_services_parser_runs_in_a_sandbox() {
    let home = TempDir::new("extract-sandboxed");
    let programs = link_test_kit::programs();
    if !link_test_kit::gate::sandbox(&programs.join("lnk-sandbox"), &home)
        || !link_test_kit::gate::out_of_tmp(&programs)
    {
        return;
    }
    let extractor = extractor().sandboxed(extract::Sandbox {
        lnk: programs.join("lnk"),
        home: home.to_path_buf(),
    });
    let pdf = std::fs::read(Path::new(FIXTURES).join("refund.pdf")).unwrap();
    let text = extractor.text(Format::Pdf, &pdf).unwrap();
    assert!(text.contains("refund"), "{text}");
}

/// A program at `dir/name` that answers as the extracting child would,
/// running `body` first.
fn child(dir: &Path, name: &str, body: &str) -> PathBuf {
    let program = dir.join(name);
    link_test_kit::write_program(&program, format!("#!/bin/sh\n{body}\n")).unwrap();
    program
}

/// The child extracting a document's text is given nothing of the
/// process holding the agents' keys: no environment (no variable named
/// like a secret's, nor any other), no descriptor but its stdin, stdout
/// and stderr, and limits on its memory and processor time.
#[cfg(target_os = "linux")]
#[test]
fn a_documents_parser_runs_in_a_child_holding_no_secret() {
    let dir = TempDir::new("extract-child");
    let seen = dir.join("seen");
    std::fs::create_dir(&seen).unwrap();
    let probe = child(
        &dir,
        "probe",
        &format!(
            "/bin/cat /proc/$$/environ > {seen}/environ
/bin/cat /proc/$$/limits > {seen}/limits
echo \"$1 $2 $3\" > {seen}/args
/bin/cat > {seen}/document
printf '{{\"text\":\"'
for f in /proc/$$/fd/*; do
  [ -e \"$f\" ] || continue
  printf '%s -> ' \"${{f##*/}}\"
  /bin/readlink -n \"$f\"
  printf '|'
done
printf '\"}}'",
            seen = seen.display()
        ),
    );
    // Its descriptors read by the shell's own glob, each printed straight
    // to its stdout: a pipeline's pipe, or a redirect's file, the shell
    // holds while it starts the command, which would list it as its own.
    // This process has an environment for the child not to be given.
    assert!(std::env::vars_os().next().is_some());
    let extractor = Extractor::program(&probe, extract::deadline());
    // The answer: its descriptors, each `<fd> -> <target>`, `|` after each.
    let fds = extractor
        .text(Format::Pdf, b"%PDF-1.4 the document")
        .unwrap();
    let read = |name: &str| std::fs::read_to_string(seen.join(name)).unwrap();
    assert_eq!(read("environ"), "", "the child's environment");
    assert_eq!(read("args"), "text show pdf\n");
    assert_eq!(read("document"), "%PDF-1.4 the document");
    // Its descriptors: stdin and stdout pipes, stderr nothing, and the
    // shell's own of its script; none of this process's.
    let fds: Vec<&str> = fds.split('|').filter(|l| l.contains(" -> ")).collect();
    assert!(fds.len() >= 3, "{fds:?}");
    for line in &fds {
        let (fd, to) = line.rsplit_once(" -> ").unwrap();
        let fd: u32 = fd.rsplit(' ').next().unwrap().parse().unwrap();
        match fd {
            0 | 1 => assert!(to.starts_with("pipe:"), "{line}"),
            2 => assert_eq!(to, "/dev/null", "{line}"),
            _ => assert_eq!(Path::new(to), probe, "{fds:?}"),
        }
    }
    let limits = read("limits");
    let limit = |name: &str| {
        let line = limits.lines().find(|l| l.starts_with(name)).unwrap();
        line[name.len()..]
            .split_whitespace()
            .next()
            .unwrap()
            .to_string()
    };
    assert_eq!(limit("Max address space"), (1u64 << 30).to_string());
    assert_eq!(
        limit("Max cpu time"),
        extract::deadline().as_secs().to_string()
    );
    assert_eq!(limit("Max core file size"), "0");
}

/// A parser that crashes, or runs past its deadline, fails its document
/// alone: found by its name, status saying why, and the rest indexed.
#[test]
fn a_parser_that_crashes_or_hangs_leaves_its_document_unread() {
    let dir = TempDir::new("extract-crash");
    std::fs::create_dir_all(dir.join("memory")).unwrap();
    std::fs::write(dir.join("memory/crash.pdf"), b"%PDF-1.4 a crash").unwrap();
    std::fs::write(dir.join("memory/hang.docx"), b"PK a hang").unwrap();
    std::fs::write(dir.join("memory/notes.md"), b"The plumber comes Tuesday.").unwrap();
    let programs = TempDir::new("extract-crash-programs");
    let runs = programs.join("runs");
    let program = child(
        &programs,
        "parser",
        &format!(
            "echo $3 >> {runs}
case $3 in
pdf) kill -SEGV $$ ;;
*) while :; do :; done ;;
esac",
            runs = runs.display()
        ),
    );
    let mut index = Index::open(&dir, None).unwrap();
    index.extract_with(Extractor::program(program, Duration::from_millis(500)));
    let started = Instant::now();
    index.refresh().unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(paths(&index, "plumber"), ["memory/notes.md"]);
    assert_eq!(paths(&index, "crash"), ["memory/crash.pdf"]);
    assert_eq!(paths(&index, "hang"), ["memory/hang.docx"]);
    let status = serde_json::to_value(index.status().unwrap()).unwrap();
    let notices = status["notices"].to_string();
    assert!(
        notices.contains("memory/crash.pdf is found by its name only: its parser failed on it")
            && notices.contains("SIGSEGV"),
        "{notices}"
    );
    assert!(
        notices.contains("memory/hang.docx is found by its name only: its parser took over 0.5 s"),
        "{notices}"
    );
    // Not parsed again while they're as they were: another file's change
    // costs no deadline.
    std::fs::write(dir.join("memory/more.md"), b"The roofer comes Friday.").unwrap();
    index.refresh().unwrap();
    assert_eq!(paths(&index, "roofer"), ["memory/more.md"]);
    assert_eq!(std::fs::read_to_string(&runs).unwrap(), "pdf\ndocx\n");
    // Changed, parsed again.
    std::fs::write(dir.join("memory/crash.pdf"), b"%PDF-1.4 another crash").unwrap();
    index.refresh().unwrap();
    assert_eq!(std::fs::read_to_string(&runs).unwrap(), "pdf\ndocx\npdf\n");
}

/// A document's text is extracted in the background, never in a
/// refresh: a refresh, and a search after it, go on while its parser
/// runs, the document found by its name and status saying it's being
/// read; once the parser answers, it's found by its text.
#[test]
fn a_document_is_extracted_while_searches_go_on() {
    let dir = TempDir::new("extract-background");
    std::fs::create_dir_all(dir.join("memory")).unwrap();
    std::fs::write(dir.join("memory/slow.pdf"), b"%PDF-1.4 slow").unwrap();
    std::fs::write(dir.join("memory/notes.md"), b"The plumber comes Tuesday.").unwrap();
    let programs = TempDir::new("extract-background-programs");
    let (started, go) = (programs.join("started"), programs.join("go"));
    // It answers only once the test lets it: what the index does
    // meanwhile is what a slow parser leaves it doing.
    let program = child(
        &programs,
        "parser",
        &format!(
            "/bin/cat > /dev/null
: > {started}
while [ ! -e {go} ]; do /bin/sleep 0.05; done
printf '{{\"text\":\"The gutters were cleaned in May.\"}}'",
            started = started.display(),
            go = go.display()
        ),
    );
    let state = TempDir::new("extract-background-index");
    let mut index = Index::open(&dir, Some(&state.join("index.sqlite"))).unwrap();
    index.extract_with(Extractor::program(program, extract::deadline()));
    index.refresh().unwrap();
    link_test_kit::wait::appears("the parser started", &started, Duration::from_secs(30));
    assert!(index.extracting());
    // Its parser runs: a refresh and searches go on.
    index.refresh().unwrap();
    assert_eq!(paths(&index, "plumber"), ["memory/notes.md"]);
    assert_eq!(paths(&index, "slow"), ["memory/slow.pdf"]);
    assert!(paths(&index, "gutters").is_empty());
    let status = serde_json::to_value(index.status().unwrap()).unwrap();
    assert!(
        status["notices"].to_string().contains(&format!(
            "memory/slow.pdf is found by its name only: {}",
            extract::PENDING
        )),
        "{status}"
    );
    std::fs::write(&go, b"").unwrap();
    link_test_kit::wait::eventually("its text indexed", Duration::from_secs(30), || {
        paths(&index, "gutters") == ["memory/slow.pdf"]
    });
    assert!(
        !serde_json::to_value(index.status().unwrap())
            .unwrap()
            .to_string()
            .contains("memory/slow.pdf")
    );
}

/// Without Link Document Converter a document is found by its name, status
/// naming the module that reads it; once it's installed, the next
/// refresh reads it, though the file didn't change.
#[test]
fn a_document_is_read_once_link_document_converter_is_installed() {
    let dir = TempDir::new("extract-installed");
    std::fs::create_dir_all(dir.join("memory")).unwrap();
    std::fs::copy(
        Path::new(FIXTURES).join("refund.pdf"),
        dir.join("memory/refund.pdf"),
    )
    .unwrap();
    let mut index = Index::open(&dir, None).unwrap();
    index.extract_with(Extractor::program(
        dir.join("not-installed"),
        extract::deadline(),
    ));
    index.refresh().unwrap();
    assert!(paths(&index, "accountant").is_empty());
    let status = serde_json::to_value(index.status().unwrap()).unwrap();
    let notices = status["notices"].to_string();
    assert!(
        notices.contains("memory/refund.pdf is found by its name only: reading it needs the document-converter module: 'lnk module add document-converter'"),
        "{notices}"
    );
    index.extract_with(extractor());
    index.refresh().unwrap();
    assert_eq!(paths(&index, "accountant"), ["memory/refund.pdf"]);
    let status = serde_json::to_value(index.status().unwrap()).unwrap();
    assert!(
        !status["notices"]
            .to_string()
            .contains("found by its name only"),
        "{status}"
    );
}

/// Memory finds Link Document Converter beside its own program for each document:
/// its own program, or Link Document Converter's, replaced under it as an upgrade
/// does, documents are still read, never failing until a restart.
#[cfg(target_os = "linux")]
#[test]
fn documents_are_extracted_after_the_programs_are_replaced() {
    use std::process::Stdio;
    let dir = TempDir::new("extract-replaced");
    std::fs::create_dir_all(dir.join("memory")).unwrap();
    std::fs::copy(
        Path::new(FIXTURES).join("refund.pdf"),
        dir.join("memory/refund.pdf"),
    )
    .unwrap();
    let programs = TempDir::new("extract-replaced-program");
    let program = programs.join("lnk-memory-tool");
    let documents = programs.join("lnk-document-converter");
    // Never open for writing here, or a child another test forks meanwhile
    // could hold it open and its spawn fail with "Text file busy".
    link_test_kit::copy_program(Path::new(env!("CARGO_BIN_EXE_lnk-memory-tool")), &program)
        .unwrap();
    link_test_kit::copy_program(&documents_program(), &documents).unwrap();
    let log = programs.join("memory.stderr");
    let mut command = link_test_kit::env::command(&program, &dir);
    command
        .arg("serve")
        .arg(&*dir)
        // Only the converter beside this process is available. The
        // test must not find an unrelated install on the caller's PATH.
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(&log).unwrap());
    let mut server = link_test_kit::process::ChildGroup::spawn(&mut command).unwrap();
    let mut input = server.take_stdin().unwrap();
    let mut output = BufReader::new(server.take_stdout().unwrap());
    let diagnostic = || std::fs::read_to_string(&log).unwrap();
    let mut ask = |message: Value| -> Value {
        writeln!(input, "{message}").unwrap();
        let mut line = String::new();
        assert_ne!(
            output.read_line(&mut line).unwrap(),
            0,
            "the memory child stopped: {}",
            diagnostic()
        );
        serde_json::from_str(&line).unwrap_or_else(|error| {
            panic!(
                "the memory child returned {line:?}: {error}; {}",
                diagnostic()
            )
        })
    };
    ask(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-03-26", "capabilities": {},
                   "clientInfo": {"name": "t", "version": "1"}}}));
    // Replaced: the file memory was started from is gone, and Link
    // documents' is a new one.
    std::fs::remove_file(&program).unwrap();
    std::fs::remove_file(&documents).unwrap();
    link_test_kit::copy_program(&documents_program(), &documents).unwrap();
    // Its writer finds the converter at its next turn, and the document
    // read in the background.
    let found = link_test_kit::wait::within("the document read", Duration::from_secs(20), || {
        let found = ask(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "search", "arguments": {"query": "accountant refund"}}}))
        .to_string();
        // Its path, not the query's words, which every reply quotes back.
        match found.contains("memory/refund.pdf") && !found.contains("found by its name only") {
            true => Ok(found),
            false => Err(found),
        }
    })
    .unwrap_or_else(|timed_out| timed_out.last);
    server.stop();
    assert!(
        found.contains("memory/refund.pdf"),
        "{found}; {}",
        diagnostic()
    );
    assert!(found.contains("accountant"), "{found}; {}", diagnostic());
    assert!(
        !found.contains("found by its name only"),
        "{found}; {}",
        diagnostic()
    );
}

/// A file in `work/`, or anywhere else of the folder, is never found;
/// one in `memory/` is.
#[test]
fn only_memory_is_indexed() {
    let dir = TempDir::new("home-only-memory");
    for folder in ["memory", "work", "instructions", "skills/s"] {
        std::fs::create_dir_all(dir.join(folder)).unwrap();
    }
    for file in [
        "memory/a.md",
        "work/b.md",
        "instructions/notes.md",
        "skills/s/SKILL.md",
        "c.md",
    ] {
        std::fs::write(dir.join(file), "The lighthouse keeper.\n").unwrap();
    }
    let mut index = Index::open(&dir, None).unwrap();
    index.refresh().unwrap();
    assert_eq!(paths(&index, "lighthouse"), ["memory/a.md"]);
}

/// A sandbox that can't start says nothing of the document: it stays
/// waiting, tried again at the next refresh, never marked unread for good.
/// The service's sandbox is run as `lnk sandbox run`, granting the parser
/// nothing but its own program, given HOME alone; once it starts, the
/// document is read.
#[test]
fn a_sandbox_that_cant_start_leaves_its_document_waiting() {
    let dir = TempDir::new("extract-retry");
    std::fs::create_dir_all(dir.join("memory")).unwrap();
    std::fs::write(dir.join("memory/later.pdf"), b"%PDF-1.4 later").unwrap();
    let programs = TempDir::new("extract-retry-programs");
    let parser = child(
        &programs,
        "parser",
        "/bin/cat > /dev/null\nprintf '{\"text\":\"The boiler was serviced in June.\"}'",
    );
    let (args, home, works) = (
        programs.join("args"),
        programs.join("home"),
        programs.join("works"),
    );
    // `lnk`: what it was asked, then a sandbox that couldn't run the
    // program (126), until it works and runs what follows `--`.
    let lnk = child(
        &programs,
        "lnk",
        &format!(
            "echo \"$@\" > {args}
echo \"$HOME\" > {home}
[ -e {works} ] || exit 126
while [ \"$1\" != -- ]; do shift; done
shift
exec \"$@\"",
            args = args.display(),
            home = home.display(),
            works = works.display()
        ),
    );
    let mut index = Index::open(&dir, None).unwrap();
    index.extract_with(Extractor::program(&parser, extract::deadline()).sandboxed(
        extract::Sandbox {
            lnk,
            home: programs.join("lnk-home"),
        },
    ));
    index.refresh().unwrap();
    let p = parser.display();
    assert_eq!(
        std::fs::read_to_string(&args).unwrap(),
        format!("sandbox run --files.read {p} --commands.allow {p} -- {p} text show pdf\n")
    );
    assert_eq!(
        std::fs::read_to_string(&home).unwrap(),
        format!("{}\n", programs.join("lnk-home").display())
    );
    let notices = || serde_json::to_value(index.status().unwrap()).unwrap()["notices"].to_string();
    assert!(
        notices().contains(&format!(
            "memory/later.pdf is found by its name only: {}",
            extract::PENDING
        )),
        "{}",
        notices()
    );
    std::fs::write(&works, b"").unwrap();
    index.refresh().unwrap();
    assert_eq!(paths(&index, "boiler"), ["memory/later.pdf"]);
}

/// Each batch takes the documents tried fewest times first: one a
/// process ended on before it was read never holds up those behind it.
#[test]
fn a_batch_takes_the_documents_tried_fewest_first() {
    let dir = TempDir::new("extract-rotate");
    std::fs::create_dir_all(dir.join("memory")).unwrap();
    for name in ["a", "b", "c", "d", "e"] {
        std::fs::write(
            dir.join(format!("memory/{name}.pdf")),
            format!("%PDF-1.4 {name}\n"),
        )
        .unwrap();
    }
    let programs = TempDir::new("extract-rotate-programs");
    let runs = programs.join("runs");
    // Every try ends before the parser runs, as a process ended would.
    let lnk = child(
        &programs,
        "lnk",
        &format!(
            "read line\necho \"$line\" >> {runs}\nexit 126",
            runs = runs.display()
        ),
    );
    let parser = child(&programs, "parser", "exit 1");
    let mut index = Index::open(&dir, None).unwrap();
    index.extract_with(Extractor::program(&parser, extract::deadline()).sandboxed(
        extract::Sandbox {
            lnk,
            home: programs.to_path_buf(),
        },
    ));
    index.refresh().unwrap();
    index.refresh().unwrap();
    let tried: Vec<String> = std::fs::read_to_string(&runs)
        .unwrap()
        .lines()
        .map(|line| line.trim_start_matches("%PDF-1.4 ").to_string())
        .collect();
    assert_eq!(tried, ["a", "b", "c", "d", "e", "a", "b", "c"]);
}

/// An index dropped while a document is extracted waits for that
/// document and starts no other: nothing of it runs past its index.
#[test]
fn a_dropped_index_waits_for_its_extraction() {
    let dir = TempDir::new("extract-drop");
    std::fs::create_dir_all(dir.join("memory")).unwrap();
    for name in ["a", "b", "c"] {
        std::fs::write(
            dir.join(format!("memory/{name}.pdf")),
            format!("%PDF-1.4 {name}\n"),
        )
        .unwrap();
    }
    let programs = TempDir::new("extract-drop-programs");
    let (log, go) = (programs.join("log"), programs.join("go"));
    let parser = child(
        &programs,
        "parser",
        &format!(
            "read line
echo \"start $line\" >> {log}
case \"$line\" in *' a') while [ ! -e {go} ]; do /bin/sleep 0.05; done ;; esac
echo \"done $line\" >> {log}
printf '{{\"text\":\"read\"}}'",
            log = log.display(),
            go = go.display()
        ),
    );
    let state = TempDir::new("extract-drop-index");
    let mut index = Index::open(&dir, Some(&state.join("index.sqlite"))).unwrap();
    index.extract_with(Extractor::program(&parser, extract::deadline()));
    index.refresh().unwrap();
    link_test_kit::wait::eventually("the parser started", Duration::from_secs(30), || {
        std::fs::read_to_string(&log).is_ok_and(|log| log.contains("start"))
    });
    let log_at = log.clone();
    let dropped = std::thread::spawn(move || {
        drop(index);
        std::fs::read_to_string(&log_at).unwrap()
    });
    std::fs::write(&go, b"").unwrap();
    let seen = dropped.join().unwrap();
    let count = |word: &str| seen.lines().filter(|l| l.starts_with(word)).count();
    assert_eq!(count("start"), count("done"), "{seen}");
}
