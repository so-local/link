//! Where releases are read from ([`Source`]): Link's own over `https://`,
//! or what `LNK_RELEASES_URL` names: an `https://` address, a private
//! bucket (`s3://`, `gs://`) read with this machine's cloud CLI and its
//! login, or a folder here (`file://`). Whichever it is, what's read is
//! trusted only as its signature says ([`super::Release`]): where the
//! files are stored adds no trust and takes none away.

use std::fmt;
use std::io::Read;
use std::path::PathBuf;

use anyhow::{bail, ensure};
use link_format::WithinExt;

use link_fetch::HttpsUrl;

/// The variable naming the releases `lnk` installs from, when not Link's.
pub const RELEASES_VAR: &str = "LNK_RELEASES_URL";

/// Where releases are, read once where it comes in (`LNK_RELEASES_URL`)
/// and carried typed after: anything else is refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// Served at an address anyone may read.
    Web(HttpsUrl),
    /// A bucket of the user's own, read with its cloud's CLI as this
    /// machine is logged in to it: nothing about it need be public.
    Bucket(Bucket),
    /// A folder on this machine.
    Folder(Folder),
}

/// A folder in a bucket: `s3://<bucket>/<folder>` or
/// `gs://<bucket>/<folder>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bucket {
    pub cloud: Cloud,
    name: String,
    /// Its folder in the bucket, `/`-joined parts; empty for the top.
    folder: String,
}

/// The clouds whose buckets releases may be in, each read with its CLI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cloud {
    /// Amazon S3 (`s3://`), read with `aws s3 cp`.
    S3,
    /// Google Cloud Storage (`gs://`), read with `gcloud storage cat`.
    Gs,
}

/// A folder on this machine, by its absolute path: `file:///<path>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder(PathBuf);

impl Cloud {
    fn scheme(self) -> &'static str {
        match self {
            Cloud::S3 => "s3",
            Cloud::Gs => "gs",
        }
    }

    /// Its CLI, and the variables it reads a login from, which Link
    /// hands no other program (its settings, `AWS_PROFILE` and the like,
    /// every program gets).
    fn cli(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Cloud::S3 => (
                "aws",
                &[
                    "AWS_ACCESS_KEY_ID",
                    "AWS_SECRET_ACCESS_KEY",
                    "AWS_SESSION_TOKEN",
                ],
            ),
            Cloud::Gs => (
                "gcloud",
                &["CLOUDSDK_AUTH_ACCESS_TOKEN", "GOOGLE_OAUTH_ACCESS_TOKEN"],
            ),
        }
    }
}

impl std::str::FromStr for Source {
    type Err = String;

    fn from_str(text: &str) -> Result<Source, String> {
        let (scheme, rest) = text.split_once("://").unwrap_or(("", text));
        let cloud = match scheme {
            "https" | "http" => return text.parse().map(Source::Web),
            "file" => return folder(rest).map(Source::Folder),
            "s3" => Cloud::S3,
            "gs" => Cloud::Gs,
            _ => {
                return Err(format!(
                    "{} isn't where releases are: give an https://, s3://<bucket>/<folder>, \
                     gs://<bucket>/<folder> or file:///<folder> location",
                    link_format::clean(text)
                ));
            }
        };
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        let (name, folder) = rest.split_once('/').unwrap_or((rest, ""));
        let named = (3..=222).contains(&name.len())
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
            && name.starts_with(|c: char| c.is_ascii_alphanumeric())
            && name.ends_with(|c: char| c.is_ascii_alphanumeric());
        if !named {
            return Err(format!(
                "{} doesn't name a bucket: {scheme}://<bucket>/<folder>, the bucket's name in \
                 lowercase letters, digits, dots and dashes",
                link_format::clean(text)
            ));
        }
        if !folder.is_empty() && !folder.split('/').all(part) {
            return Err(format!(
                "{} isn't a folder in a bucket: each part of it letters, digits or ._~+_-, \
                 never . or ..",
                link_format::clean(text)
            ));
        }
        Ok(Source::Bucket(Bucket {
            cloud,
            name: name.to_string(),
            folder: folder.to_string(),
        }))
    }
}

/// The folder `file://<rest>` names: an absolute path, each part of it
/// [`part`].
fn folder(rest: &str) -> Result<Folder, String> {
    let path = rest.strip_suffix('/').unwrap_or(rest);
    match path.strip_prefix('/') {
        Some(inner) if inner.is_empty() || inner.split('/').all(part) => {
            Ok(Folder(PathBuf::from(path)))
        }
        _ => Err(format!(
            "file://{} isn't a folder here: file:///<folder>, its absolute path, each part of \
             it letters, digits or ._~+_-, never . or ..",
            link_format::clean(rest)
        )),
    }
}

/// A part of a path releases are in: nothing a shell, a URL or a cloud's
/// CLI reads apart, and never `.` or `..`.
fn part(part: &str) -> bool {
    !part.is_empty()
        && part != "."
        && part != ".."
        && part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._~+_-".contains(&b))
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Web(url) => write!(f, "{url}"),
            Source::Bucket(bucket) => write!(f, "{bucket}"),
            Source::Folder(Folder(path)) => write!(f, "file://{}", path.display()),
        }
    }
}

impl fmt::Display for Bucket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}", self.cloud.scheme(), self.name)?;
        match self.folder.is_empty() {
            true => Ok(()),
            false => write!(f, "/{}", self.folder),
        }
    }
}

/// Releases a program follows rather than Link's own, by why it follows
/// them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Followed {
    /// Those `LNK_RELEASES_URL` names.
    Named(Source),
    /// The bundle it runs from ([`Source::bundle_of`]).
    Bundle(Source),
    /// This machine's copy of the release it is ([`Source::copy_in`]),
    /// which holds that release alone: nothing later to check for.
    Copy(Source),
}

impl Followed {
    /// This program's: `LNK_RELEASES_URL`, else the bundle it runs from,
    /// else the copy of the release it is kept by the install it runs
    /// from, or by this user's installation folder, if any.
    pub fn here() -> anyhow::Result<Option<Followed>> {
        Self::of(
            std::env::var(RELEASES_VAR).ok(),
            std::env::current_exe().ok().as_deref(),
            link_files::layout::Hidden::Releases
                .try_path()
                .ok()
                .as_deref(),
            &link_version::version(),
        )
    }

    /// Those followed with `named` (`LNK_RELEASES_URL`'s value), by the
    /// program at `exe`, the program being release `version`. Its copy is
    /// the one its own install keeps ([`Source::copies_of`]), whoever runs
    /// it, as root runs a box user's `lnk`, else the one in `copies`, the
    /// running user's.
    pub fn of(
        named: Option<String>,
        exe: Option<&std::path::Path>,
        copies: Option<&std::path::Path>,
        version: &link_version::Version,
    ) -> anyhow::Result<Option<Followed>> {
        if let Some(text) = named.filter(|text| !text.is_empty()) {
            return text
                .parse()
                .map(|source| Some(Followed::Named(source)))
                .map_err(|e| anyhow::anyhow!("{RELEASES_VAR}: {e}"));
        }
        let folder = |root| Source::Folder(Folder(root));
        if let Some(root) = exe.and_then(Source::bundle_of) {
            return Ok(Some(Followed::Bundle(folder(root))));
        }
        let installed = exe.and_then(Source::copies_of);
        Ok(installed
            .as_deref()
            .into_iter()
            .chain(copies)
            .find_map(|copies| Source::copy_in(copies, version))
            .map(|root| Followed::Copy(folder(root))))
    }

    /// Whether it's a copy without release `version` (none: the latest,
    /// which a copy never holds), so an upgrade to it can't be read here.
    pub fn lacks(&self, version: Option<&link_version::Version>) -> bool {
        match (self, version) {
            (Followed::Copy(Source::Folder(Folder(root))), Some(version)) => {
                Source::copy_in(root, version).is_none()
            }
            (Followed::Copy(_), _) => true,
            (Followed::Named(_) | Followed::Bundle(_), _) => false,
        }
    }

    /// Where they are.
    pub fn into_source(self) -> Source {
        match self {
            Followed::Named(source) | Followed::Bundle(source) | Followed::Copy(source) => source,
        }
    }
}

impl Source {
    /// The releases this program follows rather than Link's own
    /// ([`Followed`]), whichever kind they are.
    pub fn followed() -> anyhow::Result<Option<Source>> {
        Ok(Followed::here()?.map(Followed::into_source))
    }

    /// `copies` (the machine's `~/.lnk/safe/releases`), when it holds a
    /// copy of release `version`, signed sums and all, at
    /// `download/v<version>` as a release's address has it. A machine
    /// given a copy of the release it was installed from, as a box made
    /// from a bucket, a folder or a bundle is, installs every later module
    /// from that copy; once its `lnk` is another version, the copy no
    /// longer matches and is not followed.
    pub fn copy_in(copies: &std::path::Path, version: &link_version::Version) -> Option<PathBuf> {
        copies
            .join(format!("download/v{version}"))
            .join("SHA256SUMS")
            .is_file()
            .then(|| copies.to_path_buf())
    }

    /// The copies of releases (`~/.lnk/safe/releases`) kept by the install
    /// `program` is in: the installation folder whose programs' folder
    /// (`~/.lnk/exec/bin`, where its map puts it) holds it. A module is
    /// found beside the `lnk` that runs it, so its release is found there
    /// too, whichever user runs that `lnk`.
    pub fn copies_of(program: &std::path::Path) -> Option<PathBuf> {
        use link_files::layout::Hidden;
        let bin = program.parent()?;
        let depth = std::path::Path::new(Hidden::Bin.relative())
            .components()
            .count();
        let hidden = bin.ancestors().nth(depth)?;
        (Hidden::Bin.at(hidden) == bin).then(|| Hidden::Releases.at(hidden))
    }

    /// The folder of the bundle `program` is part of: one `lnk bundle create` made,
    /// in the release layout, its programs for this computer in `bin/`
    /// beside `latest/download/VERSION`. A bundle's own `lnk` and modules
    /// install from it, as `lnk box create --bundle` does, so a box made
    /// or moved to by them gets the bundle's build, never Link's.
    pub fn bundle_of(program: &std::path::Path) -> Option<PathBuf> {
        let program = program.canonicalize().ok()?;
        let bin = program.parent()?;
        let root = bin.parent()?;
        (bin.file_name()? == "bin" && root.join(super::BUNDLE_VERSION).is_file())
            .then(|| root.to_path_buf())
    }

    /// The releases this `lnk` installs from: those `LNK_RELEASES_URL`
    /// names, else its bundle's, else Link's own.
    pub fn here() -> anyhow::Result<Source> {
        Ok(Self::followed()?.unwrap_or_else(|| {
            Source::Web(
                super::LINK_RELEASES
                    .parse()
                    .expect("Link's releases are an address"),
            )
        }))
    }

    /// `path` (`/`-joined parts, as [`part`] takes them) inside it.
    pub fn join(&self, path: &str) -> anyhow::Result<Source> {
        ensure!(
            path.split('/').all(part),
            "{path:?} isn't a path in releases"
        );
        Ok(match self {
            Source::Web(url) => Source::Web(
                format!("{url}/{path}")
                    .parse()
                    .map_err(|e: String| anyhow::anyhow!(e))?,
            ),
            Source::Bucket(bucket) => Source::Bucket(Bucket {
                folder: match bucket.folder.is_empty() {
                    true => path.to_string(),
                    false => format!("{}/{path}", bucket.folder),
                },
                ..bucket.clone()
            }),
            Source::Folder(Folder(dir)) => Source::Folder(Folder(dir.join(path))),
        })
    }

    /// Whether another machine reads it as this one does: an address
    /// does; a bucket (read with this machine's login) and a folder here
    /// don't, so a box is given a copy of what it installs.
    pub fn shared(&self) -> bool {
        matches!(self, Source::Web(_))
    }

    /// File `name` in it, refused once it's over `max` bytes; `http`
    /// reads an address.
    pub(super) async fn read(
        &self,
        http: &link_fetch::Client,
        name: &str,
        max: u64,
    ) -> anyhow::Result<Vec<u8>> {
        ensure!(part(name), "{name:?} isn't a file of a release");
        let too_large = || anyhow::anyhow!("{name} is larger than {max} bytes; not downloading it");
        match self {
            Source::Web(url) => {
                let url = format!("{url}/{name}");
                match http.get(&url).limit(max).bytes().await {
                    Err(link_fetch::Error::TooLarge { .. }) => Err(too_large()),
                    got => got.within(format!("downloading {url}")),
                }
            }
            Source::Bucket(bucket) => {
                let (cloud, args) = (bucket.cloud, bucket.read_command(name));
                let object = format!("{bucket}/{name}");
                let read = tokio::task::spawn_blocking(move || {
                    read_output(cloud_command(cloud, &args), max)
                })
                .await?;
                match read {
                    Ok(Some(bytes)) => Ok(bytes),
                    Ok(None) => Err(too_large()),
                    Err(err) => Err(err.context(format!("reading {object}"))),
                }
            }
            Source::Folder(Folder(dir)) => {
                let path = dir.join(name);
                let file =
                    std::fs::File::open(&path).within(format!("reading {}", path.display()))?;
                let mut bytes = Vec::new();
                file.take(max + 1)
                    .read_to_end(&mut bytes)
                    .within(format!("reading {}", path.display()))?;
                ensure!(bytes.len() as u64 <= max, too_large());
                Ok(bytes)
            }
        }
    }
}

impl Bucket {
    /// The arguments its cloud's CLI ([`Cloud::cli`]) writes object
    /// `name` of the folder to standard output with.
    fn read_command(&self, name: &str) -> Vec<String> {
        let object = format!("{self}/{name}");
        let args: &[&str] = match self.cloud {
            Cloud::S3 => &["s3", "cp", "--only-show-errors", &object, "-"],
            Cloud::Gs => &["storage", "cat", &object],
        };
        args.iter().map(|a| a.to_string()).collect()
    }
}

/// `cloud`'s CLI with `args`, given the login it reads from variables
/// ([`Cloud::cli`]), as this program was given them (in its environment,
/// or handed by the `lnk` that ran it), beside what every program Link
/// runs is given.
fn cloud_command(cloud: Cloud, args: &[String]) -> std::process::Command {
    let (program, login) = cloud.cli();
    let mut command = link_secretive::process::command(program);
    for name in login {
        if let Some(secret) = link_secretive::Secret::from_env(name) {
            link_secretive::process::give(&mut command, name, &secret);
        }
    }
    command.args(args);
    command
}

/// What `command` writes to standard output; `None` once it's over `max`
/// bytes (and it's stopped); an error with what it said when it fails.
fn read_output(mut command: std::process::Command, max: u64) -> anyhow::Result<Option<Vec<u8>>> {
    use std::process::Stdio;
    let program = command.get_program().to_string_lossy().into_owned();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .within(format!("running {program}: is it installed?"))?;
    let mut stderr = child.stderr.take().within("no output from the CLI")?;
    // Read beside the output, so a CLI saying much can't stall on it.
    let said = std::thread::spawn(move || {
        let mut said = Vec::new();
        let _ = (&mut stderr).take(64 * 1024).read_to_end(&mut said);
        said
    });
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .within("no output from the CLI")?
        .take(max + 1)
        .read_to_end(&mut bytes);
    if bytes.len() as u64 > max {
        let _ = child.kill();
        let _ = child.wait();
        return Ok(None);
    }
    let status = child.wait()?;
    let said = said.join().unwrap_or_default();
    read.within(format!("reading what {program} wrote"))?;
    if !status.success() {
        let said = String::from_utf8_lossy(&said);
        bail!(
            "{program} failed ({status}): {}",
            link_format::clean(said.trim())
        );
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use super::{Bucket, Cloud, Source, read_output};

    /// A program in a bundle's `bin/` follows its bundle; one anywhere
    /// else, such as an installed `lnk`, follows none.
    #[test]
    fn a_bundles_program_follows_its_bundle() {
        let root = link_test_kit::TempDir::new("release-bundle");
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("lnk"), "").unwrap();
        assert_eq!(Source::bundle_of(&bin.join("lnk")), None);
        let version = root.path().join(super::super::BUNDLE_VERSION);
        std::fs::create_dir_all(version.parent().unwrap()).unwrap();
        std::fs::write(&version, "0.0.4-dev.1\n").unwrap();
        let canonical = root.path().canonicalize().unwrap();
        assert_eq!(Source::bundle_of(&bin.join("lnk")), Some(canonical));
        let elsewhere = root.path().join("exec").join("bin");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("lnk"), "").unwrap();
        assert_eq!(Source::bundle_of(&elsewhere.join("lnk")), None);
    }

    /// A machine follows its copy of a release only while it holds the
    /// release its `lnk` is: the sums of that version, not another's, and
    /// not the folder alone (every machine keeps `update.json` there).
    #[test]
    fn a_copy_of_the_running_release_is_followed() {
        let copies = link_test_kit::TempDir::new("release-copy");
        let version: link_version::Version = "0.0.4-dev.202610131900".parse().unwrap();
        std::fs::write(copies.join("update.json"), "{}").unwrap();
        assert_eq!(Source::copy_in(copies.path(), &version), None);
        let other = copies.join("download/v0.0.4-dev.202610121900");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("SHA256SUMS"), "").unwrap();
        assert_eq!(Source::copy_in(copies.path(), &version), None);
        let own = copies.join(format!("download/v{version}"));
        std::fs::create_dir_all(&own).unwrap();
        assert_eq!(Source::copy_in(copies.path(), &version), None);
        std::fs::write(own.join("SHA256SUMS"), "").unwrap();
        assert_eq!(
            Source::copy_in(copies.path(), &version),
            Some(copies.path().to_path_buf())
        );
    }

    /// What's followed says why: the variable first, then a bundle's own
    /// program, then this machine's copy of the running release; nothing,
    /// for an installed `lnk` with no copy.
    #[test]
    fn what_is_followed_says_why() {
        use super::Followed;
        let root = link_test_kit::TempDir::new("release-followed");
        let version: link_version::Version = "0.0.4-dev.202610131900".parse().unwrap();
        let bin = root.join("bundle").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("lnk"), "").unwrap();
        let stamp = root.join("bundle").join(super::super::BUNDLE_VERSION);
        std::fs::create_dir_all(stamp.parent().unwrap()).unwrap();
        std::fs::write(&stamp, "0.0.4-dev.202610131900\n").unwrap();
        let installed = root.join("exec").join("bin");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::write(installed.join("lnk"), "").unwrap();
        let copies = root.join("releases");
        let own = copies.join(format!("download/v{version}"));
        std::fs::create_dir_all(&own).unwrap();
        std::fs::write(own.join("SHA256SUMS"), "").unwrap();
        let of = |named: Option<&str>, exe: &std::path::Path| {
            Followed::of(named.map(String::from), Some(exe), Some(&copies), &version).unwrap()
        };
        assert!(matches!(
            of(Some("s3://my-builds/dev"), &bin.join("lnk")),
            Some(Followed::Named(_))
        ));
        assert!(matches!(
            of(Some(""), &bin.join("lnk")),
            Some(Followed::Bundle(_))
        ));
        assert!(matches!(
            of(None, &installed.join("lnk")),
            Some(Followed::Copy(_))
        ));
        // A copy holds its own release alone: no latest, no other.
        let copy = of(None, &installed.join("lnk")).unwrap();
        assert!(!copy.lacks(Some(&version)));
        assert!(copy.lacks(None));
        let later: link_version::Version = "0.0.4-dev.202610141900".parse().unwrap();
        assert!(copy.lacks(Some(&later)));
        let bundle = of(None, &bin.join("lnk")).unwrap();
        assert!(!bundle.lacks(None) && !bundle.lacks(Some(&later)));
        let none = Followed::of(None, Some(&installed.join("lnk")), None, &version).unwrap();
        assert_eq!(none, None);
        // An install's own copy is followed whoever runs its lnk: root
        // running a box user's finds that user's, not its own.
        let user = root.join("user").join(".lnk");
        let user_bin = link_files::layout::Hidden::Bin.at_default(&user);
        std::fs::create_dir_all(&user_bin).unwrap();
        std::fs::write(user_bin.join("lnk"), "").unwrap();
        let kept = link_files::layout::Hidden::Releases.at_default(&user);
        assert_eq!(Source::copies_of(&user_bin.join("lnk")), Some(kept.clone()));
        assert_eq!(Source::copies_of(&bin.join("lnk")), None);
        let user_own = kept.join(format!("download/v{version}"));
        std::fs::create_dir_all(&user_own).unwrap();
        std::fs::write(user_own.join("SHA256SUMS"), "").unwrap();
        let roots = root.join("root-releases");
        match Followed::of(None, Some(&user_bin.join("lnk")), Some(&roots), &version).unwrap() {
            Some(Followed::Copy(Source::Folder(super::Folder(at)))) => assert_eq!(at, kept),
            other => panic!("{other:?}"),
        }
        assert!(Followed::of(Some("ftp://x".into()), None, None, &version).is_err());
    }

    #[test]
    fn each_kind_of_location_is_read_and_shown_as_it_was_given() {
        for (text, shared) in [
            ("https://builds.example/staging/0123abcd", true),
            ("s3://my-builds/dev/0123abcdef01", false),
            ("gs://my_builds.example/dev", false),
            ("s3://my-builds", false),
            ("file:///home/me/releases/dev", false),
        ] {
            let source: Source = text.parse().unwrap_or_else(|e| panic!("{text}: {e}"));
            assert_eq!(source.to_string(), text);
            assert_eq!(source.shared(), shared, "{text}");
        }
        // One trailing slash is dropped.
        let source: Source = "gs://my-builds/dev/".parse().unwrap();
        assert_eq!(source.to_string(), "gs://my-builds/dev");
        assert!(matches!(
            source,
            Source::Bucket(Bucket {
                cloud: Cloud::Gs,
                ..
            })
        ));
    }

    #[test]
    fn anything_else_is_refused() {
        for (bad, says) in [
            ("ftp://builds.example/x", "isn't where releases are"),
            ("builds.example/x", "isn't where releases are"),
            ("", "isn't where releases are"),
            ("az://builds/x", "isn't where releases are"),
            ("s3://", "doesn't name a bucket"),
            ("s3://ab/x", "doesn't name a bucket"),
            ("s3://My-Builds/x", "doesn't name a bucket"),
            ("s3://-builds/x", "doesn't name a bucket"),
            ("s3://me@builds/x", "doesn't name a bucket"),
            ("s3://builds/../x", "isn't a folder in a bucket"),
            ("s3://builds/a//b", "isn't a folder in a bucket"),
            ("gs://builds/a b", "isn't a folder in a bucket"),
            ("gs://builds/x?y=1", "isn't a folder in a bucket"),
            ("gs://builds/$(touch pwned)", "isn't a folder in a bucket"),
            ("s3://builds/x'y", "isn't a folder in a bucket"),
            ("file://relative/dir", "isn't a folder here"),
            ("file://localhost/dir", "isn't a folder here"),
            ("file:///home/../etc", "isn't a folder here"),
            ("file:///home/me/my%20dir", "isn't a folder here"),
            ("https://me:pw@builds.example/x", "holds a login"),
            ("https://builds.example/x?token=1", "has a ? or a #"),
        ] {
            let err = bad.parse::<Source>().unwrap_err();
            assert!(err.contains(says), "{bad}: {err}");
            assert!(!err.contains("pw@"), "{err}");
        }
    }

    #[test]
    fn a_path_inside_one_is_joined_by_its_kind() {
        let joined = |text: &str| {
            text.parse::<Source>()
                .unwrap()
                .join("download/v0.0.5-dev.202610171200")
                .unwrap()
                .to_string()
        };
        assert_eq!(
            joined("s3://my-builds/dev/0123"),
            "s3://my-builds/dev/0123/download/v0.0.5-dev.202610171200"
        );
        assert_eq!(
            joined("s3://my-builds"),
            "s3://my-builds/download/v0.0.5-dev.202610171200"
        );
        assert_eq!(
            joined("file:///srv/rel"),
            "file:///srv/rel/download/v0.0.5-dev.202610171200"
        );
        assert_eq!(
            joined("https://builds.example/dev"),
            "https://builds.example/dev/download/v0.0.5-dev.202610171200"
        );
        let source: Source = "s3://my-builds".parse().unwrap();
        assert!(source.join("../x").is_err());
        assert!(source.join("a/").is_err());
    }

    #[test]
    fn a_bucket_is_read_with_its_clouds_cli() {
        let read = |text: &str| match text.parse::<Source>().unwrap() {
            Source::Bucket(bucket) => bucket.read_command("SHA256SUMS"),
            other => panic!("{other}"),
        };
        assert_eq!(
            read("s3://my-builds/dev/0123"),
            [
                "s3",
                "cp",
                "--only-show-errors",
                "s3://my-builds/dev/0123/SHA256SUMS",
                "-"
            ]
        );
        assert_eq!(
            read("gs://my-builds/dev"),
            ["storage", "cat", "gs://my-builds/dev/SHA256SUMS"]
        );
        assert_eq!(Cloud::S3.cli().0, "aws");
        assert_eq!(Cloud::Gs.cli().0, "gcloud");
    }

    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn sh(script: &str) -> std::process::Command {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn a_clis_output_is_taken_whole_or_refused() {
        assert_eq!(
            read_output(sh("printf 'abc'"), 3).unwrap(),
            Some(b"abc".to_vec())
        );
        // Over the cap: refused, however much more there is.
        assert_eq!(read_output(sh("yes"), 1024).unwrap(), None);
        // A failure says what the CLI said, not what it wrote out.
        let err = read_output(sh("printf partial; echo 'NoSuchKey' >&2; exit 1"), 64).unwrap_err();
        let err = format!("{err:#}");
        assert!(
            err.contains("NoSuchKey") && !err.contains("partial"),
            "{err}"
        );
        let err = read_output(std::process::Command::new("/nonexistent/aws"), 64).unwrap_err();
        assert!(format!("{err:#}").contains("is it installed"), "{err:#}");
    }

    #[tokio::test]
    async fn a_folder_is_read_by_name_within_its_cap() {
        let dir = link_test_kit::TempDir::new("release-source-folder");
        std::fs::write(dir.join("SHA256SUMS"), "abc").unwrap();
        let source: Source = format!("file://{}", dir.display()).parse().unwrap();
        let http = super::super::client().unwrap();
        assert_eq!(source.read(&http, "SHA256SUMS", 3).await.unwrap(), b"abc");
        let err = source.read(&http, "SHA256SUMS", 2).await.unwrap_err();
        assert!(
            format!("{err:#}").contains("larger than 2 bytes"),
            "{err:#}"
        );
        let err = source.read(&http, "VERSION", 64).await.unwrap_err();
        assert!(format!("{err:#}").contains("reading"), "{err:#}");
        for bad in ["../SHA256SUMS", "a/b", "..", ""] {
            assert!(source.read(&http, bad, 64).await.is_err(), "{bad}");
        }
    }
}
