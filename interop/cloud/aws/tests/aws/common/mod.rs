use std::path::{Path, PathBuf};
use std::process::Command;

/// A fake `aws`: logs its arguments and AWS variables, then runs `script`.
pub fn fake(dir: &Path, script: &str) -> PathBuf {
    let path = dir.join("aws");
    link_test_kit::write_program(
        &path,
        format!(
            "#!/bin/sh\necho \"$*\" >> {log}\nenv | grep -E '^AWS_' | sort >> {log}\n{script}\n",
            log = dir.join("aws.log").display()
        ),
    )
    .unwrap();
    path
}

#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
pub fn lnk_aws(dir: &Path, aws: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lnk-aws"));
    command
        .env("HOME", dir.join("home"))
        .env("LNK_SERVICES", "off")
        .envs(link_test_kit::keychain::isolated(&dir.join("home")))
        .env("LNK_AWS", aws)
        .env("PATH", "/usr/bin:/bin")
        // The shell's own AWS settings must not reach the account's CLI.
        .env("AWS_PROFILE", "someone-else")
        .env_remove("AWS_REGION")
        .env_remove("AWS_DEFAULT_REGION");
    command
}

/// Read a JSON argument from the fake AWS CLI's log, compared as a value:
/// its key order is serde_json's, which a build's features can change.
pub fn json_argument(call: &str, flag: &str) -> serde_json::Value {
    let (_, argument) = call.split_once(&format!("{flag} ")).expect("the JSON flag");
    serde_json::Deserializer::from_str(argument)
        .into_iter::<serde_json::Value>()
        .next()
        .expect("a JSON argument")
        .expect("valid JSON")
}
