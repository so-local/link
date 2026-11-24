//! Hermes 0.23.0 through 0.23.3 select source-install dependencies by the checkout's absolute
//! path and mutable HERMES_HOME. Seal its staged selection before publication,
//! with its catalogue, hosted metadata and gateway fixes ([`model_catalog`])
//! applied while the payload is still private staging. Remove each
//! adapter-local workaround when the pinned release supports it.

mod constants;
mod manager;
pub(crate) mod model_catalog;
use constants::*;

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use std::{path::Path, process::Command, sync::LazyLock};

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct Config {
    commit: String,
    pub code: String,
    pub launcher: String,
    acp: String,
    store: String,
    installs: String,
    facts: String,
    environments: String,
    venv_config: String,
    manifest: String,
    pub extras: Vec<String>,
    install_key_characters: usize,
    max_document_bytes: u64,
    runtime_env: std::collections::BTreeMap<String, String>,
    runtime_remove: Vec<String>,
    pub installer_folders: std::collections::BTreeMap<String, String>,
    manager: manager::Config,
    sources: Sources,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sources {
    environments: Source,
    paths: Source,
    runtime: Source,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    path: String,
    sha256: String,
}

pub(super) static CONFIG: LazyLock<Config> = LazyLock::new(|| {
    let config = Config::shipped();
    config.validate().expect("checked Hermes program settings");
    config
});

impl Config {
    fn shipped() -> Self {
        link_files::fs::from_toml(
            include_str!("../../programs.toml"),
            Path::new("programs.toml"),
        )
        .expect("built-in Hermes program settings")
    }

    fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            self.commit == crate::COMMIT,
            "workaround matches the pinned Hermes release"
        );
        ensure!(
            self.install_key_characters > 0 && self.install_key_characters <= 64,
            "bounded install key"
        );
        ensure!(
            self.max_document_bytes > 0 && self.max_document_bytes < u64::MAX,
            "bounded program documents"
        );
        for value in [
            &self.code,
            &self.launcher,
            &self.acp,
            &self.store,
            &self.installs,
            &self.facts,
            &self.environments,
            &self.venv_config,
            &self.manifest,
            &self.sources.environments.path,
            &self.sources.paths.path,
            &self.sources.runtime.path,
        ]
        .into_iter()
        .chain(self.installer_folders.values())
        {
            link_files::Relative::new(value)?;
        }
        ensure!(
            !self.extras.is_empty() && self.extras.iter().all(|extra| !extra.is_empty()),
            "staged extras"
        );
        for source in [
            &self.sources.environments,
            &self.sources.paths,
            &self.sources.runtime,
        ] {
            ensure!(
                source.sha256.len() == 64 && source.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
                "source SHA-256"
            );
        }
        self.manager.validate()?;
        Ok(())
    }
}

fn read(path: &Path) -> anyhow::Result<Vec<u8>> {
    link_files::fs::read_trusted(
        path,
        link_files::fs::Trust {
            owners: &[],
            deny_mode: 0,
            ancestors: true,
            no_link: true,
        },
        CONFIG.max_document_bytes,
        "Hermes program document",
    )
    .map(|document| document.text.into_bytes())
}

#[derive(Deserialize)]
struct Facts {
    packages: Packages,
}
#[derive(Deserialize)]
struct Packages {
    venv: Venv,
}
#[derive(Deserialize)]
struct Venv {
    environment: std::path::PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    repo: String,
    venv: String,
    store: String,
}

pub(super) struct Payload {
    pub code: std::path::PathBuf,
    pub launcher: std::path::PathBuf,
    /// Hermes's dedicated ACP console entrypoint: the same ACP server,
    /// without first importing and preparing the interactive CLI.
    pub acp: std::path::PathBuf,
}

impl Payload {
    /// The `hermes` CLI, its runtime data in `data`.
    pub fn command(&self, data: &Path) -> Command {
        Self::run(&self.launcher, data)
    }

    /// `hermes-acp`, its ACP over stdio, its runtime data in `data`.
    pub fn acp(&self, data: &Path) -> Command {
        Self::run(&self.acp, data)
    }

    fn run(program: &Path, data: &Path) -> Command {
        let mut command = link_secretive::process::command(program);
        command.env("HERMES_HOME", data);
        runtime(&mut command);
        command
    }
}

/// A launcher alone is never a completed install. Runtime has no source-install
/// fallback when the sealed selection is missing, malformed or incomplete.
pub(super) fn payload(root: &Path) -> anyhow::Result<Payload> {
    let document = read(&root.join(&CONFIG.manifest))?;
    let manifest: Manifest = serde_json::from_slice(&document)?;
    ensure!(
        manifest.repo == CONFIG.code && manifest.store == CONFIG.store,
        "the sealed payload names its code and tools"
    );
    link_files::Relative::new(&manifest.venv)?;
    let suffix = Path::new(&manifest.venv).strip_prefix(&CONFIG.installs)?;
    let mut parts = suffix.components();
    let key = parts
        .next()
        .and_then(|part| part.as_os_str().to_str())
        .context("a sealed install key")?;
    ensure!(
        key.len() == CONFIG.install_key_characters && key.bytes().all(|b| b.is_ascii_hexdigit()),
        "a sealed install key"
    );
    ensure!(
        parts.as_path().starts_with(&CONFIG.environments),
        "a sealed dependency generation"
    );
    let root = root.canonicalize()?;
    let environment = root.join(&manifest.venv).canonicalize()?;
    ensure!(
        environment.starts_with(&root),
        "the sealed environment remains inside its payload"
    );
    read(&environment.join(&CONFIG.venv_config))?;
    let code = root.join(&manifest.repo).canonicalize()?;
    let store = root.join(&manifest.store).canonicalize()?;
    ensure!(
        code.starts_with(&root) && store.starts_with(&root) && store.is_dir(),
        "sealed code and tools remain inside their payload"
    );
    let launcher = code.join(&CONFIG.launcher);
    ensure!(launcher.is_file(), "the sealed Hermes launcher exists");
    let acp = code.join(&CONFIG.acp);
    ensure!(acp.is_file(), "the sealed Hermes ACP entrypoint exists");
    manager::check(&root)?;
    Ok(Payload {
        code,
        launcher,
        acp,
    })
}

fn install_state(root: &Path) -> anyhow::Result<std::path::PathBuf> {
    let root = root.canonicalize()?;
    let code = root.join(&CONFIG.code).canonicalize()?;
    ensure!(
        code.starts_with(&root),
        "Hermes code remains in its staged payload"
    );
    let key = link_digest::sha256(
        code.to_str()
            .context("a UTF-8 Hermes program path")?
            .as_bytes(),
    );
    Ok(root
        .join(&CONFIG.installs)
        .join(&key[..CONFIG.install_key_characters]))
}

fn manifest(root: &Path) -> anyhow::Result<Manifest> {
    let root = root.canonicalize()?;
    let state = install_state(&root)?;
    let facts: Facts = serde_json::from_slice(&read(&state.join(&CONFIG.facts))?)?;
    let environment = facts.packages.venv.environment.canonicalize()?;
    ensure!(
        environment.starts_with(state.join(&CONFIG.environments).canonicalize()?),
        "the selected environment belongs to this staged install"
    );
    ensure!(
        environment.join(&CONFIG.venv_config).is_file(),
        "the selected environment is complete"
    );
    let store = root.join(&CONFIG.store).canonicalize()?;
    ensure!(
        store.starts_with(&root),
        "the tools store remains in its staged payload"
    );
    Ok(Manifest {
        repo: CONFIG.code.clone(),
        venv: environment
            .strip_prefix(&root)?
            .to_str()
            .context("a UTF-8 dependency path")?
            .into(),
        store: store
            .strip_prefix(&root)?
            .to_str()
            .context("a UTF-8 tools path")?
            .into(),
    })
}

fn patched(bytes: &[u8], sha256: &str, replacements: &[(&str, &str)]) -> anyhow::Result<String> {
    ensure!(
        link_digest::sha256(bytes) == sha256,
        "Hermes source differs from the pinned workaround"
    );
    let mut text = std::str::from_utf8(bytes)?.to_owned();
    for (before, after) in replacements {
        ensure!(
            text.matches(before).count() == 1,
            "a pinned Hermes source fragment differs"
        );
        text = text.replacen(before, after, 1);
    }
    Ok(text)
}

/// Apply only after the installer and all supported extras have completed.
pub(super) fn seal(root: &Path) -> anyhow::Result<()> {
    let manifest = manifest(root)?;
    let code = root.join(&CONFIG.code);
    ensure!(
        link_digest::sha256(&read(&code.join(&CONFIG.sources.runtime.path))?)
            == CONFIG.sources.runtime.sha256,
        "Hermes's resident PM contract differs from the pinned source"
    );
    // Validate both sources before changing either. The whole payload is still
    // private staging; a failed seal is never published.
    let sources = [
        (
            &CONFIG.sources.environments,
            &[DEPENDENCY_HOME, STORE, ACTIVATION, SELECTION][..],
        ),
        (&CONFIG.sources.paths, &[WRITABLE_STORE][..]),
    ];
    let changes = sources
        .iter()
        .map(|(source, replacements)| {
            let path = code.join(&source.path);
            Ok((
                path.clone(),
                patched(&read(&path)?, &source.sha256, replacements)?,
            ))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    // The catalogue fixes validate all their sources before changing any,
    // and the published payload is read-only to every run: apply them here.
    model_catalog::apply(&code)?;
    for (path, text) in changes {
        link_files::write_private(&path, text.as_bytes())?;
    }
    manager::stage(root, &install_state(root)?)?;
    link_files::write_private(
        &root.join(&CONFIG.manifest),
        &serde_json::to_vec(&manifest)?,
    )?;
    Ok(())
}

/// Runtime data stays mutable; executable dependencies are selected by payload.
pub(super) fn runtime(command: &mut Command) -> &mut Command {
    for name in &CONFIG.runtime_remove {
        command.env_remove(name);
    }
    command.envs(&CONFIG.runtime_env)
}

#[cfg(test)]
pub(super) mod tests;
