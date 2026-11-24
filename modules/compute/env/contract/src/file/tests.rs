use super::*;
use link_test_kit::TempDir;
mod inputs;

const FILE: &str = r#"name = "app"
version = 1
workdir = "app"

[variables]
DATABASE_URL = "postgres://localhost:5432/app"

[secrets]
names = ["NPM_TOKEN"]
sealed = ["STRIPE_KEY"]

[[stage]]
name = "system"
run = "apt-get install -y postgresql"
user = "root"

[[stage]]
name = "clone"
run = "git clone https://github.com/me/app app"
sandbox = "clone.toml"

[[stage]]
name = "deps"
run = "pip install -r app/requirements.txt"
sandbox = "deps.toml"
inputs = ["requirements.txt"]
timeout = "10m"

[[stage]]
name = "pull"
run = "git -C app pull --ff-only"
sandbox = "clone.toml"
when = "start"

[[service]]
name = "postgres"
run = "postgres -D data -p 5432"
sandbox = "postgres.toml"
ready = { port = 5432 }
reset = "sh app/reset.sh"
"#;

fn folder(name: &str) -> TempDir {
    let dir = TempDir::new(&format!("envfile-{name}"));
    for file in ["deps.toml", "clone.toml", "postgres.toml"] {
        std::fs::write(dir.join(file), "name = \"x\"\nversion = 1\n").unwrap();
    }
    std::fs::write(dir.join("requirements.txt"), "flask\n").unwrap();
    std::fs::write(dir.join("README.md"), "hi\n").unwrap();
    dir
}

/// `text`, written as a person writes a spec, naming its schema first.
fn with(dir: &Path, text: &str) -> anyhow::Result<Read> {
    std::fs::write(dir.join("env.toml"), format!("schema = 1\n{text}")).unwrap();
    read(&dir.join("env.toml"))
}

fn error(dir: &Path, text: &str) -> String {
    format!("{:#}", with(dir, text).unwrap_err())
}

fn platform() -> Platform {
    Platform {
        os: Os::Ubuntu2404,
        arch: Arch::X8664,
    }
}

#[test]
fn the_whole_file_reads_and_prints_back_as_itself() {
    let dir = folder("whole");
    let read = with(&dir, FILE).unwrap();
    let file = &read.file;
    assert_eq!(file.name, "app");
    assert_eq!(file.workdir(), Some(Path::new("app")));
    assert_eq!(file.stages().len(), 4);
    assert!(file.stages()[0].root() && file.stages()[1].saved() && !file.stages()[3].saved());
    assert_eq!(file.stages()[2].timeout(), Some(Timeout::minutes(10)));
    assert_eq!(file.sealed_names(), ["STRIPE_KEY"]);
    assert_eq!(file.isolation(false, false), Isolation::Sandbox);
    assert_eq!(file.isolation(true, false), Isolation::OwnBox);
    assert_eq!(file.isolation(false, true), Isolation::OwnBox);
    assert_eq!(
        file.named_files(),
        [
            "clone.toml",
            "deps.toml",
            "postgres.toml",
            "requirements.txt"
        ]
        .map(PathBuf::from)
    );
    assert_eq!(
        file.service("postgres").unwrap().ready.timeout,
        Timeout::minutes(1)
    );
    assert!(
        file.service("redis")
            .unwrap_err()
            .to_string()
            .contains("it has postgres")
    );
    let printed = link_spec::print_spec(file).unwrap();
    assert!(printed.contains("when = \"image\""), "{printed}");
    assert!(printed.contains("timeout = \"1m\""), "{printed}");
    assert_eq!(&parse(&printed).unwrap(), file, "{printed}");
}

#[test]
fn an_unknown_key_or_a_missing_sandbox_is_refused_by_its_line() {
    let dir = folder("refused");
    for (field, guidance) in [
        ("box", "lnk box create --spec"),
        ("repository", "name = \"clone\""),
    ] {
        let err = error(
            &dir,
            &format!("name = \"app\"\nversion = 1\n[{field}]\nx = 1\n"),
        );
        assert!(err.contains("line 4") && err.contains(guidance), "{err}");
    }
    let err = error(&dir, &FILE.replace("workdir = ", "folder = "));
    assert!(
        err.contains("line 4") && err.contains("unknown field"),
        "{err}"
    );
    std::fs::remove_file(dir.join("deps.toml")).unwrap();
    let err = error(&dir, FILE);
    assert!(
        err.contains("line 26: stage deps's sandbox deps.toml isn't a file"),
        "{err}"
    );
    // Each refusal names its line, `..` too, however the file is read.
    let err = error(&dir, &FILE.replace("deps.toml", "../deps.toml"));
    assert!(
        err.contains("line 26: ") && err.contains("without .."),
        "{err}"
    );
}

#[test]
fn what_cant_be_is_refused() {
    let dir = folder("cant");
    let cases = [
        (
            FILE.replace("user = \"root\"", "user = \"root\"\nwhen = \"start\""),
            "image is built",
        ),
        (
            FILE.replace(
                "user = \"root\"",
                "user = \"root\"\nsandbox = \"deps.toml\"",
            ),
            "has a sandbox and user = \"root\"",
        ),
        (
            FILE.replace("sandbox = \"clone.toml\"\nwhen", "when"),
            "stage pull has no sandbox",
        ),
        (
            FILE.replace("user = \"root\"", "user = \"Al ice\""),
            "lowercase letters, digits, _ and -, at most 32",
        ),
        (
            FILE.replace(
                "user = \"root\"",
                "user = \"builder\"\nsandbox = \"deps.toml\"",
            ),
            "stage system has a sandbox and user = \"builder\"",
        ),
        (
            FILE.replace("DATABASE_URL", "API_TOKEN"),
            "API_TOKEN is named like a secret",
        ),
        (FILE.replace("NPM_TOKEN", "DATABASE_URL"), "given twice"),
        (FILE.replace("NPM_TOKEN", "STRIPE_KEY"), "given twice"),
        (FILE.replace("DATABASE_URL", "LNK_X"), "Link sets"),
        (
            FILE.replace("timeout = \"10m\"", "timeout = \"25h\""),
            "from a second",
        ),
        (
            FILE.replace("ready = { port = 5432 }", "ready = {}"),
            "one of them",
        ),
        (
            FILE.replace("{ port = 5432 }", "{ port = 5432, run = \"true\" }"),
            "one of them",
        ),
        (
            FILE.replace("workdir = \"app\"", "workdir = \"../app\""),
            "workdir",
        ),
        (FILE.replace("version = 1", "version = 2"), "lnk upgrade"),
        (
            FILE.replace("version = 1", "version = 1\nos = \"debian\""),
            "unknown variant",
        ),
        (
            FILE.replace("name = \"app\"", "name = \"App\""),
            "can't be a name",
        ),
    ];
    for (text, want) in cases {
        let err = error(&dir, &text);
        assert!(err.contains(want), "{want}: {err}");
    }
}

#[test]
fn a_root_service_needs_a_box_of_its_own() {
    let dir = folder("root");
    let text = FILE.replace("sandbox = \"postgres.toml\"", "user = \"root\"");
    let file = with(&dir, &text).unwrap().file;
    assert_eq!(file.isolation(false, false), Isolation::OwnBox);
    assert!(
        file.own_box_because(false, false)
            .unwrap()
            .contains("postgres")
    );
}

/// A file's hash is the same from every build of Link: a program whose
/// serde_json keeps keys as written (`preserve_order`, which a crate
/// elsewhere in the workspace turns on) hashes it as one that sorts them.
#[test]
fn the_hash_is_the_same_whatever_serde_json_keeps() {
    let written = serde_json::json!({"b": 1, "a": [{"d": 2, "c": "x"}]});
    assert_eq!(super::sorted(&written), r#"{"a":[{"c":"x","d":2}],"b":1}"#);
    let dir = folder("hash-pinned");
    assert_eq!(
        hash(&with(&dir, FILE).unwrap(), platform()).unwrap(),
        "80bbc6b91dac4b8558c8ed26f19463d1"
    );
}

#[test]
fn a_changed_input_makes_a_new_hash_and_a_readme_doesnt() {
    let dir = folder("hash");
    let first = hash(&with(&dir, FILE).unwrap(), platform()).unwrap();
    assert_eq!(first.len(), 32);
    link_cloud_contract::check_image_hash(&first).unwrap();
    std::fs::write(dir.join("README.md"), "changed\n").unwrap();
    assert_eq!(hash(&with(&dir, FILE).unwrap(), platform()).unwrap(), first);
    std::fs::write(dir.join("requirements.txt"), "flask==3\n").unwrap();
    let second = hash(&with(&dir, FILE).unwrap(), platform()).unwrap();
    assert_ne!(second, first);
    // What the image never holds: a comment, a secret, a service, a
    // stage run at every start.
    std::fs::write(dir.join("postgres.toml"), "name = \"y\"\nversion = 1\n").unwrap();
    for text in [
        format!("# a comment\n{FILE}\n\n"),
        FILE.replace("NPM_TOKEN", "PIP_TOKEN"),
        FILE.replace("STRIPE_KEY", "OTHER_KEY"),
        FILE.replace("ready = { port = 5432 }", "ready = { port = 5433 }"),
        FILE.replace("git -C app pull --ff-only", "git -C app pull"),
    ] {
        assert_eq!(
            hash(&with(&dir, &text).unwrap(), platform()).unwrap(),
            second,
            "{text}"
        );
    }
    // What builds it: a variable, a saved stage's run or user, the
    // workdir, the OS and the processor.
    for text in [
        FILE.replace("5432/app", "5432/other"),
        FILE.replace("pip install", "pip3 install"),
        FILE.replace("workdir = \"app\"", "workdir = \"src\""),
    ] {
        assert_ne!(
            hash(&with(&dir, &text).unwrap(), platform()).unwrap(),
            second,
            "{text}"
        );
    }
    let read = with(&dir, FILE).unwrap();
    for other in [
        Platform {
            os: Os::Ubuntu2204,
            arch: Arch::X8664,
        },
        Platform {
            os: Os::Ubuntu2404,
            arch: Arch::Arm64,
        },
    ] {
        assert_ne!(hash(&read, other).unwrap(), second);
    }
}

#[test]
fn groups_change_the_hash_and_invalid_compositions_are_refused() {
    let dir = folder("groups");
    let sequential = "name = \"app\"\nversion = 1\n[[stage]]\nname = \"a\"\nrun = \"true\"\nsandbox = \"deps.toml\"\n[[stage]]\nname = \"b\"\nrun = \"true\"\nsandbox = \"deps.toml\"\n";
    let grouped = sequential
        .replacen(
            "[[stage]]",
            "[[stage]]\nname = \"both\"\n[[stage.parallel]]",
            1,
        )
        .replacen(
            "[[stage]]\nname = \"b\"",
            "[[stage.parallel]]\nname = \"b\"",
            1,
        );
    assert_ne!(
        hash(&with(&dir, sequential).unwrap(), platform()).unwrap(),
        hash(&with(&dir, &grouped).unwrap(), platform()).unwrap()
    );
    for invalid in [
        grouped.replace("name = \"both\"", "name = \"both\"\nrun = \"true\""),
        grouped.replace("name = \"b\"", "name = \"a\""),
        "name = \"app\"\nversion = 1\n[[stage]]\nname = \"empty\"\nparallel = []".into(),
        grouped.replace("sandbox = \"deps.toml\"", "parallel = []"),
    ] {
        assert!(with(&dir, &invalid).is_err(), "{invalid}");
    }
}

#[test]
fn a_path_through_a_link_is_refused() {
    let dir = folder("link");
    let elsewhere = TempDir::new("envfile-elsewhere");
    std::fs::write(elsewhere.join("secrets.toml"), "x").unwrap();
    std::os::unix::fs::symlink(&*elsewhere, dir.join("specs")).unwrap();
    let text = FILE.replace(
        "inputs = [\"requirements.txt\"]",
        "inputs = [\"specs/secrets.toml\"]",
    );
    let err = error(&dir, &text);
    assert!(err.contains("goes through a link"), "{err}");
    std::os::unix::fs::symlink(dir.join("deps.toml"), dir.join("linked.toml")).unwrap();
    let err = error(&dir, &FILE.replace("\"deps.toml\"", "\"linked.toml\""));
    assert!(err.contains("goes through a link"), "{err}");
}

#[test]
fn durations_and_names() {
    assert_eq!(duration("30s").unwrap(), std::time::Duration::from_secs(30));
    assert_eq!(
        duration("2h").unwrap(),
        std::time::Duration::from_secs(7200)
    );
    assert!(duration("0s").is_err() && duration("2d").is_err() && duration("x").is_err());
    assert_eq!(
        Timeout::from_std(std::time::Duration::from_secs(90)).to_string(),
        "90s"
    );
    assert_eq!(
        Timeout::from_std(std::time::Duration::from_secs(7200)).to_string(),
        "2h"
    );
    assert_eq!(proxy_name("STRIPE_KEY"), "stripe-key");
}

#[test]
fn a_secret_variable_refusal_names_current_file_approval() {
    let text = "name = \"app\"\nversion = 1\n[variables]\nAPI_TOKEN = \"fixture\"\n";
    let variables = BTreeMap::from([("API_TOKEN".into(), "fixture".into())]);
    let error = checks::check_values(text, &variables, &Secrets::default()).unwrap_err();
    let error = error.to_string();
    assert!(error.contains("line 4"), "{error}");
    assert!(error.contains("lnk secret create API_TOKEN"), "{error}");
    assert!(
        error.contains("lnk env approval add API_TOKEN --spec <file>"),
        "{error}"
    );
    assert!(!error.contains("lnk secret add"), "{error}");
}

/// A file names its schema: an empty environment file is refused, with
/// field flags or without, as the kind's own reader refuses empty text.
#[test]
fn an_empty_environment_file_is_refused() {
    let dir = folder("empty");
    let file = dir.join("env.toml");
    std::fs::write(&file, "").unwrap();
    let flags = link_spec::FieldFlags::default();
    for refused in [read(&file), read_with(&file, &flags)] {
        let said = format!("{:#}", refused.unwrap_err());
        assert!(said.contains("schema"), "{said}");
    }
}
