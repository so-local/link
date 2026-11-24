//! Publish the already installed PM generation using its native sealed marker.

use super::*;
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct Config {
    pub directory: String,
    selection: String,
    generations: String,
    marker: String,
    python: String,
    libraries: String,
    python_prefix: String,
    site_packages: String,
}

impl Config {
    pub fn validate(&self) -> anyhow::Result<()> {
        for name in [
            &self.directory,
            &self.selection,
            &self.generations,
            &self.marker,
            &self.python,
            &self.libraries,
            &self.python_prefix,
            &self.site_packages,
        ] {
            link_files::Relative::new(name)?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    inputs: String,
    generation: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Resident {
    python: String,
    site_packages: String,
}

fn site(environment: &Path) -> anyhow::Result<PathBuf> {
    let layout = &CONFIG.manager;
    let mut selected = None;
    for entry in std::fs::read_dir(environment.join(&layout.libraries))? {
        let entry = entry?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(&layout.python_prefix))
        {
            let path = entry.path().join(&layout.site_packages);
            if path.is_dir() {
                ensure!(
                    selected.replace(path.canonicalize()?).is_none(),
                    "one staged PM site-packages directory"
                );
            }
        }
    }
    let selected = selected.context("a staged PM site-packages directory")?;
    ensure!(
        selected.starts_with(environment),
        "PM site-packages remain in their generation"
    );
    Ok(selected)
}

pub(super) fn stage(root: &Path, state: &Path) -> anyhow::Result<()> {
    let root = root.canonicalize()?;
    let layout = &CONFIG.manager;
    let runtime = state.join(&layout.directory);
    let selected: Selection = serde_json::from_slice(&read(&runtime.join(&layout.selection))?)?;
    ensure!(
        !selected.inputs.is_empty(),
        "PM records its verified inputs"
    );
    link_files::Relative::new(&selected.generation)?;
    ensure!(
        Path::new(&selected.generation).starts_with(&layout.generations),
        "PM selects an installed generation"
    );
    let environment = runtime.join(selected.generation).canonicalize()?;
    ensure!(
        environment.starts_with(runtime.join(&layout.generations).canonicalize()?),
        "the PM selection stays inside its generations"
    );
    // The source marker is installed last by PM. Its exact bytes need not be
    // retained; the resident marker below describes the sealed layout instead.
    read(&environment.join(&layout.marker))?;
    let python = environment.join(&layout.python).canonicalize()?;
    ensure!(
        python.is_file() && python.starts_with(root.join(&CONFIG.store).canonicalize()?),
        "PM uses its staged store interpreter"
    );
    let site = site(&environment)?;
    let marker = Resident {
        python: Path::new("..")
            .join(python.strip_prefix(&root)?)
            .to_str()
            .context("a UTF-8 PM interpreter path")?
            .into(),
        site_packages: site
            .strip_prefix(&environment)?
            .to_str()
            .context("a UTF-8 PM site path")?
            .into(),
    };
    let destination = root.join(&layout.directory);
    ensure!(
        !destination.exists(),
        "the sealed PM runtime is not already present"
    );
    std::fs::rename(environment, &destination)?;
    link_files::write_private(
        &destination.join(&layout.marker),
        &serde_json::to_vec(&marker)?,
    )?;
    check(&root)
}

pub(super) fn check(root: &Path) -> anyhow::Result<()> {
    let runtime = root.join(&CONFIG.manager.directory);
    let marker: Resident = serde_json::from_slice(&read(&runtime.join(&CONFIG.manager.marker))?)?;
    ensure!(
        Path::new(&marker.python).is_relative(),
        "a portable PM interpreter path"
    );
    link_files::Relative::new(&marker.site_packages)?;
    let python = runtime.join(marker.python).canonicalize()?;
    let site = runtime.join(marker.site_packages).canonicalize()?;
    ensure!(
        python.is_file() && python.starts_with(root.join(&CONFIG.store).canonicalize()?),
        "the sealed PM interpreter belongs to its store"
    );
    ensure!(
        site.is_dir() && site.starts_with(&runtime),
        "the sealed PM dependencies belong to their runtime"
    );
    Ok(())
}
