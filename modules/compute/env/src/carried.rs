//! An environment prepared in a workdir for a sandboxed program (`lnk env
//! prepare`), and carried with its reviewed files to another machine
//! (`export`, `import`).

use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::ensure;
use link_format::WithinExt;

use crate::environment::{self, Packed, Prepared};
use crate::{file, here, remote, values};

/// The file and its inputs, packed to carry to another machine.
pub fn export(read: &file::Read) -> anyhow::Result<Packed> {
    let packed = environment::pack(read, &file::hash(read, read.file.platform())?)?;
    ensure!(
        packed.files.len() <= 1024 && serde_json::to_vec(&packed)?.len() <= 8 << 20,
        "an environment carries at most 1024 files and 8 MiB"
    );
    Ok(packed)
}

/// Prepare before the program that works there starts. The image hash avoids repeating
/// successful build stages; start stages and services run on every start.
pub fn prepare(read: &file::Read) -> anyhow::Result<Prepared> {
    let workdir = read
        .file
        .workdir()
        .within("an environment prepared for a sandbox names a workdir")?;
    ensure!(
        read.file.root_service().is_none(),
        "an environment prepared for a sandbox has no root service"
    );
    let destination = link_files::try_home_dir()?.join(workdir);
    if let Some(why) = link_sandbox_contract::unsafe_files(&destination) {
        anyhow::bail!("{} isn't a sandbox's workdir: {why}", destination.display());
    }
    ensure!(
        link_sandbox_contract::hidden_dirs()
            .iter()
            .all(|closed| !destination.starts_with(closed) && !closed.starts_with(&destination)),
        "an environment's workdir cannot open the folders every sandbox hides"
    );
    let digest = file::hash(read, read.file.platform())?;
    let receipts = link_files::layout::Hidden::Envs.path().join("prepared");
    let receipt = receipts.join(&read.file.name);
    let missing = !link_files::try_home_dir()?.join(workdir).is_dir();
    let build =
        std::fs::read_to_string(&receipt).ok().as_deref() != Some(digest.as_str()) || missing;
    if build && !link_machine::machine::kind()?.is_box() {
        ensure!(
            remote::root_stage(&read.file).is_none(),
            "an environment's root build stages need a box"
        );
    }
    // A writable path must exist on the host before Linux binds it
    // into a sandbox; creating it inside an empty sandboxed home would
    // disappear with that sandbox. Empty clone destinations are valid.
    let mut folder = link_files::nofollow::Dir::open(&link_files::try_home_dir()?)?;
    for part in workdir.components() {
        if let std::path::Component::Normal(name) = part {
            folder = folder.create_folder(name, 0o700)?;
        }
    }
    link_files::create_private_dir(&receipts)?;
    if build {
        remote::setup(read, None, None).map_err(|report| anyhow::anyhow!("{}", report.headline))?;
        link_files::write_private(&receipt, digest.as_bytes())?;
    }
    remote::copy_here(read)?;
    let env =
        here::files_dir(&read.file.name).join(read.path.file_name().within("the file's name")?);
    let mut session = remote::Session::with_log(
        None,
        &read.file.name,
        Some(Path::new(env.file_name().within("the file's name")?)),
        here::Action::Start,
        values::of_yours(read)?,
        None,
    )?;
    let ended = session.wait(None, &|| false)?;
    ensure!(
        ended.code == Some(0),
        "starting environment {} failed",
        read.file.name
    );
    ensure!(
        link_files::try_home_dir()?.join(workdir).is_dir(),
        "environment {} did not create its workdir {}",
        read.file.name,
        workdir.display()
    );
    Ok(Prepared {
        env,
        workdir: link_files::try_home_dir()?.join(workdir),
    })
}

/// A successful image build or setup is reusable by every run on this machine.
pub fn prepared(read: &file::Read) -> anyhow::Result<()> {
    let receipts = link_files::layout::Hidden::Envs.path().join("prepared");
    link_files::create_private_dir(&receipts)?;
    link_files::write_private(
        &receipts.join(&read.file.name),
        file::hash(read, read.file.platform())?.as_bytes(),
    )?;
    Ok(())
}

/// An incoming environment is bounded and staged before anything is
/// installed. A computer accepts only its own reviewed copy; a box can
/// take the environment sent by the person bringing work to it.
pub fn import(input: impl std::io::Read) -> anyhow::Result<Prepared> {
    const MAX: usize = 8 << 20;
    let mut bytes = Vec::new();
    input.take((MAX + 1) as u64).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX, "an environment's files exceed 8 MiB");
    let packed: Packed = environment::unpack(&bytes)?;
    file::check_path("", Path::new(&packed.file), "the environment file")?;
    ensure!(
        Path::new(&packed.file).components().count() == 1,
        "the environment file is a filename"
    );
    ensure!(packed.files.len() <= 1024, "too many environment files");
    struct Staging(PathBuf);
    impl Drop for Staging {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let staging = Staging(link_files::private_temp_dir("env-arriving")?);
    for (name, content) in &packed.files {
        file::check_path("", Path::new(name), "an environment input")?;
        let path = staging.0.join(name);
        link_files::create_private_dir(path.parent().within("the input's folder")?)?;
        link_files::write_private(&path, content.as_bytes())?;
    }
    let path = staging.0.join(&packed.file);
    let read = file::read(&path)?;
    let expected: std::collections::BTreeSet<PathBuf> = read
        .file
        .named_files()
        .into_iter()
        .chain(std::iter::once(PathBuf::from(&packed.file)))
        .collect();
    ensure!(
        expected.len() == packed.files.len()
            && packed.files.keys().all(|p| expected.contains(Path::new(p))),
        "the environment carries only the files it names"
    );
    ensure!(
        file::hash(&read, read.file.platform())? == packed.hash,
        "the environment's hash differs"
    );
    let kept = here::files_dir(&read.file.name).join(&packed.file);
    if !link_machine::machine::kind()?.is_box() {
        let reviewed = file::read(&kept).and_then(|kept| export(&kept));
        ensure!(
            reviewed.is_ok_and(|known| known == packed),
            "environment {} came from another machine and has no matching reviewed copy here: \
             set up your reviewed file with lnk env prepare --spec <file> before moving it here",
            read.file.name
        );
    }
    remote::copy_here(&read)?;
    prepare(&file::read(&kept)?)
}
