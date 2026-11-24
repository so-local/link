//! `lnk-file-store`: the `file` place's driver. An agent's collections in its
//! folder on this machine (`LNK_STORE_DIR`), as a person reads and edits
//! them: its files by path as plain files, each conversation a log only
//! added to, beside a copy for people. It answers `link_store_contract::store`'s
//! contract (`link_store_contract::store::file`), run by the storage bridge
//! outside every sandbox, and follows no link in the folder.

use std::path::PathBuf;

use anyhow::Context;
use link_format::{Done, Out, Report};
use link_store_contract::store::{self, file::Folder};

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::describe_component!();
    let code = link_settings::output::run(try_main);
    // A compact conversation's folds handed on, done before it ends.
    link_store_contract::store::compact::settle();
    code
}

fn try_main(out: &mut Out) -> Result<Done, Report> {
    let contract = store::Contract {
        files: true,
        jobs: true,
        ..store::Contract::serving()
    };
    let Some(operation) = link_file_store::cli::operation().map_err(|err| {
        Report::error(link_format::sentence!("Couldn't uninstall it.")).cause(err)
    })?
    else {
        return Ok(Done::shown());
    };
    store::run(out, operation, contract, |request| {
        let agent = std::env::var(store::AGENT_ENV).with_context(|| {
            format!("{} isn't set: the storage bridge runs it", store::AGENT_ENV)
        })?;
        Folder::at(&dir()?, &agent)?.answer(&request)
    })
}

/// The agent's folder: absolute.
fn dir() -> anyhow::Result<PathBuf> {
    let dir = std::env::var_os(store::DIR_ENV)
        .map(PathBuf::from)
        .with_context(|| format!("{} isn't set: the storage bridge runs it", store::DIR_ENV))?;
    anyhow::ensure!(dir.is_absolute(), "{} isn't absolute", store::DIR_ENV);
    Ok(dir)
}

#[cfg(test)]
link_test_kit::program!(unit);
