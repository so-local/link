//! Verified `lnk` downloads for `lnk upgrade` and `lnk module add`.
//!
//! Every release carries `SHA256SUMS` and `SHA256SUMS.sig`, an SSH signature
//! (`ssh-keygen -Y sign -n link-release`) made by the signer its `lnk` was
//! compiled to trust ([`crate::Build`]). A prod build (`LNK_BUILD=prod`)
//! trusts exactly the signer named in `LNK_PROD_SIGNER` as it's compiled:
//! Link's public release is the prod build compiled with Link's signer,
//! `link`, whose public half is `scripts/release/link-signers`, so a
//! release that someone else published, even with access to the releases
//! repo, doesn't verify and isn't installed. A staging build
//! (`LNK_BUILD=staging`) trusts Link's signer and the staging signer in
//! `scripts/release/staging-signers`, a dev build (`LNK_BUILD=dev`) Link's
//! signer and the contributor's own, named in `LNK_DEV_SIGNER` as it's
//! compiled, and a build from source Link's signer alone.
//! Where a release is read from ([`Source`]: an address, a private
//! bucket, a folder) changes nothing of this: its signature is checked
//! the same way wherever it's stored.

use anyhow::{bail, ensure};
use base64::Engine;
use link_format::WithinExt;
use ring::{digest, signature};

mod source;
pub use source::{Bucket, Cloud, Folder, Followed, RELEASES_VAR, Source};

/// Signature namespace, so a release signature can't be reused for (or
/// confused with) anything else signed with the same key.
pub const NAMESPACE: &str = "link-release";

/// Link's signer, `link`, in `ssh-keygen`'s allowed-signers format: what
/// Link's public release is compiled to trust, and what a build from
/// source, a dev build and a staging build trust beside their own. The
/// relay's updater and `install.sh` trust the same signer.
pub const LINK_SIGNER: &str = include_str!("../../../scripts/release/link-signers");

/// Link's signer and the staging signer, which staging builds are signed
/// with: what a staging build trusts, so it installs staging builds and
/// releases alike. Private, and read only where [`TRUSTED_SIGNERS`] is
/// chosen as `lnk` is compiled, so a release's binary holds none of the
/// staging signer's bytes.
const STAGING_BUILD_SIGNERS: &str = concat!(
    include_str!("../../../scripts/release/link-signers"),
    include_str!("../../../scripts/release/staging-signers")
);

/// The key a dev build is signed with, its one allowed-signers line, read
/// from `LNK_DEV_SIGNER` as a dev build is compiled
/// (`lnk bundle create --build dev` derives it from the contributor's
/// signer, whose secret is never committed). A dev build without one doesn't
/// compile, rather than trust nothing or anything; no other build reads
/// it.
const DEV_SIGNER: &str = match (crate::BUILD, option_env!("LNK_DEV_SIGNER")) {
    (crate::Build::Dev, Some(line)) => signer_line(line),
    (crate::Build::Dev, None) => panic!(
        "LNK_BUILD=dev needs LNK_DEV_SIGNER: the allowed-signers line of the key the dev build \
         is signed with (lnk bundle create --build dev sets it)"
    ),
    _ => "",
};

/// The signer a prod build is signed with, its one allowed-signers line,
/// read from `LNK_PROD_SIGNER` as a prod build is compiled (`lnk bundle`
/// derives it from the bundle's signer: Link's for Link's public release,
/// yours for a production bundle of your own). A prod build trusts it
/// alone; without one it doesn't compile, and no other build reads it.
const PROD_SIGNER: &str = match (crate::BUILD, option_env!("LNK_PROD_SIGNER")) {
    (crate::Build::Prod, Some(line)) => signer_line(line),
    (crate::Build::Prod, None) => panic!(
        "LNK_BUILD=prod needs LNK_PROD_SIGNER: the allowed-signers line of the key the prod \
         build is signed with (lnk bundle sets it)"
    ),
    _ => "",
};

/// `line`, when it's one allowed-signers line for a release's namespace
/// and an Ed25519 key, as `link-signers` holds; anything else doesn't
/// compile.
const fn signer_line(line: &str) -> &str {
    const PREFIX: &[u8] = b"link-release namespaces=\"link-release\" ssh-ed25519 AAAA";
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' || bytes[i] == b'\r' || (i < PREFIX.len() && bytes[i] != PREFIX[i]) {
            panic!(
                "the build's signer isn't one line: \
                 link-release namespaces=\"link-release\" ssh-ed25519 <key> [comment]"
            );
        }
        i += 1;
    }
    if bytes.len() <= PREFIX.len() {
        panic!("the build's signer holds no key");
    }
    line
}

/// Link's signer and a dev build's own: what a dev build trusts, so it
/// installs dev builds signed with its own and releases alike.
const DEV_BUILD_SIGNERS: &str = {
    const LEN: usize = LINK_SIGNER.len() + DEV_SIGNER.len() + 1;
    const BYTES: [u8; LEN] = {
        let (release, dev) = (LINK_SIGNER.as_bytes(), DEV_SIGNER.as_bytes());
        let mut out = [b'\n'; LEN];
        let mut i = 0;
        while i < release.len() {
            out[i] = release[i];
            i += 1;
        }
        while i < release.len() + dev.len() {
            out[i] = dev[i - release.len()];
            i += 1;
        }
        out
    };
    match std::str::from_utf8(&BYTES) {
        Ok(signers) => signers,
        Err(_) => panic!("LNK_DEV_SIGNER isn't text"),
    }
};

/// The keys this `lnk` trusts to sign what it installs, chosen once when
/// it's built ([`crate::BUILD`]): a prod build's, the signer it was
/// compiled with alone; a staging build's, Link's signer and the staging
/// signer; a dev build's, Link's signer and its own; a build from source,
/// Link's signer alone.
pub const TRUSTED_SIGNERS: &str = match crate::BUILD {
    crate::Build::Prod => PROD_SIGNER,
    crate::Build::Staging => STAGING_BUILD_SIGNERS,
    crate::Build::Dev => DEV_BUILD_SIGNERS,
    crate::Build::Source => LINK_SIGNER,
};

/// An allowed-signers file a debug build trusts as well, for tests of a
/// release of their own: never read by a release build.
pub const TEST_SIGNERS: &str = "LNK_TEST_RELEASE_SIGNERS";

/// The keys this `lnk` trusts to sign what it installs: [`TRUSTED_SIGNERS`],
/// and in a debug build, a test's own ([`TEST_SIGNERS`]).
pub mod advisories;

pub fn trusted_signers() -> String {
    // Compiled into a debug build alone: a release never reads it.
    #[cfg(debug_assertions)]
    if let Some(test) =
        std::env::var_os(TEST_SIGNERS).and_then(|file| std::fs::read_to_string(file).ok())
    {
        return format!("{TRUSTED_SIGNERS}{test}");
    }
    TRUSTED_SIGNERS.to_string()
}

/// Link's own releases, where `lnk` installs from unless `LNK_RELEASES_URL`
/// names others ([`Source`]).
pub const LINK_RELEASES: &str = "https://github.com/so-local/link-releases/releases";

/// Where, in a bundle `lnk bundle create` made, the version it holds is:
/// the file that marks a folder as a bundle its `bin/` programs follow.
pub const BUNDLE_VERSION: &str = "latest/download/VERSION";

/// Where release `version` of `source` is (`<source>/download/v<version>`),
/// or the latest (`<source>/latest/download`).
pub fn base(source: &Source, version: Option<&link_version::Version>) -> anyhow::Result<Source> {
    match version {
        Some(version) => source.join(&format!("download/v{version}")),
        None => source.join("latest/download"),
    }
}

/// The one rule for what may be installed, from a release whose signed
/// `VERSION` says `said`: every release must say which it is; one asked
/// for by version must be it; the latest must not come before `floor`
/// (this `lnk`'s version: always for `lnk upgrade`, for a box when this
/// `lnk` is a released one), since someone with the releases' host could
/// put an old signed release with a known hole in the latest's place.
/// What it is, when it may, which the signed program it holds must then
/// report (every caller checks).
pub fn check_installable(
    said: Option<link_version::Version>,
    asked: Option<&link_version::Version>,
    floor: Option<&link_version::Version>,
) -> anyhow::Result<link_version::Version> {
    let Some(said) = said else {
        bail!("the release doesn't say which version it is (no VERSION): not installing it");
    };
    match asked {
        Some(asked) => {
            ensure!(
                said == *asked,
                "the release under v{asked} is lnk {said}; not installing it"
            );
            Ok(said)
        }
        None => {
            if let Some(floor) = floor {
                ensure!(
                    said >= *floor,
                    "the latest release is lnk {said}, behind this lnk {floor}: name its \
                     version to install it anyway"
                );
            }
            Ok(said)
        }
    }
}

/// The Ed25519 keys in an allowed-signers file.
pub fn signer_keys(signers: &str) -> anyhow::Result<Vec<[u8; 32]>> {
    let mut keys = Vec::new();
    for line in signers.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace().skip_while(|f| *f != "ssh-ed25519");
        let (Some(_), Some(key)) = (fields.next(), fields.next()) else {
            continue;
        };
        let blob = base64::engine::general_purpose::STANDARD.decode(key)?;
        keys.push(parse_public_key(&mut Reader(&blob))?);
    }
    ensure!(
        !keys.is_empty(),
        "no ssh-ed25519 keys among the release signers"
    );
    Ok(keys)
}

/// Checks `armored` (an `SSH SIGNATURE` block) over `message` against
/// `keys`, in the [`NAMESPACE`] namespace.
pub fn verify(message: &[u8], armored: &str, keys: &[[u8; 32]]) -> anyhow::Result<()> {
    let body: String = armored
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("-----"))
        .collect();
    let blob = base64::engine::general_purpose::STANDARD
        .decode(body)
        .within("the release signature is not valid base64")?;

    // PROTOCOL.sshsig: "SSHSIG", version, public key, namespace, reserved,
    // hash algorithm, signature.
    let mut r = Reader(&blob);
    ensure!(r.take(6)? == b"SSHSIG", "not an SSH signature");
    ensure!(r.u32()? == 1, "unsupported SSH signature version");
    let key = parse_public_key(&mut Reader(r.string()?))?;
    let namespace = r.string()?;
    let reserved = r.string()?;
    let hash_alg = r.string()?;
    let mut sig = Reader(r.string()?);
    ensure!(sig.string()? == b"ssh-ed25519", "not an Ed25519 signature");
    let sig = sig.string()?;

    ensure!(
        namespace == NAMESPACE.as_bytes(),
        "the signature is for {:?}, not {NAMESPACE:?}",
        String::from_utf8_lossy(namespace)
    );
    ensure!(
        keys.contains(&key),
        "the release is signed by a key lnk doesn't trust"
    );
    let hashed = match hash_alg {
        b"sha512" => digest::digest(&digest::SHA512, message),
        b"sha256" => digest::digest(&digest::SHA256, message),
        other => bail!(
            "unsupported signature hash {:?}",
            String::from_utf8_lossy(other)
        ),
    };
    let mut signed = b"SSHSIG".to_vec();
    for field in [NAMESPACE.as_bytes(), reserved, hash_alg, hashed.as_ref()] {
        signed.extend_from_slice(&(field.len() as u32).to_be_bytes());
        signed.extend_from_slice(field);
    }
    signature::UnparsedPublicKey::new(&signature::ED25519, key)
        .verify(&signed, sig)
        .map_err(|_| anyhow::anyhow!("the release signature doesn't match"))
}

/// The largest `SHA256SUMS`, `SHA256SUMS.sig` or `VERSION` downloaded.
pub const MAX_SMALL_FILE: u64 = 64 * 1024;

/// The largest release archive downloaded.
pub const MAX_ARCHIVE: u64 = 200 * 1024 * 1024;

/// Giving up on a release server: connecting, and waiting for the next
/// bytes of a download (not the whole download, so a large archive on a
/// slow line still arrives).
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// A downloaded, verified `lnk` release archive.
#[derive(Debug)]
pub struct Download {
    pub archive: Vec<u8>,
    pub asset: String,
}

/// This machine's release target, as `install.sh` names them.
pub fn target() -> anyhow::Result<&'static str> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        (os, arch) => bail!("no prebuilt lnk for {os} {arch}; build from source instead"),
    })
}

/// The archive with one program, for this machine: `core` (`lnk` itself)
/// or a module (`tunnel` holds `lnk-example`).
pub fn program_asset(program: &str) -> anyhow::Result<String> {
    Ok(format!("lnk-{program}-{}.tar.gz", target()?))
}

/// A release whose checksum list is signed by a trusted key: every archive
/// fetched from it is checked against that list.
pub struct Release {
    at: Source,
    http: link_fetch::Client,
    /// Asset name -> SHA-256 (lowercase hex).
    sums: Vec<(String, String)>,
    /// `SHA256SUMS` and `SHA256SUMS.sig`, as read and verified.
    signed: (Vec<u8>, Vec<u8>),
}

impl Release {
    /// `SHA256SUMS` and its signature from `base` (a release download URL
    /// such as `https://github.com/so-local/link-releases/releases/latest/download`),
    /// verified against `signers`.
    pub async fn fetch(base: &str, signers: &str) -> anyhow::Result<Self> {
        Self::open(&web(base)?, signers).await
    }

    /// [`Release::fetch`] from `at`, a release's folder ([`base`]) of any
    /// [`Source`].
    pub async fn open(at: &Source, signers: &str) -> anyhow::Result<Self> {
        let keys = signer_keys(signers)?;
        let release = Self {
            at: at.clone(),
            http: client()?,
            sums: Vec::new(),
            signed: Default::default(),
        };
        let sums = release.get_raw("SHA256SUMS", MAX_SMALL_FILE).await?;
        let sig = release
            .get_raw("SHA256SUMS.sig", MAX_SMALL_FILE)
            .await
            .within("the release has no signature; not installing it")?;
        verify(&sums, &String::from_utf8_lossy(&sig), &keys)?;
        let text = std::str::from_utf8(&sums).within("SHA256SUMS is not text")?;
        Ok(Self {
            sums: parse_sums(text),
            signed: (sums, sig),
            ..release
        })
    }

    /// A copy of it in `dir`, a release's folder another `lnk` or
    /// `install.sh` reads as this one was read: its signed `SHA256SUMS`
    /// and signature as they were verified, and each of `assets`, its
    /// checksum the signed one. The files written, those two first.
    pub async fn save(
        &self,
        dir: &std::path::Path,
        assets: &[&str],
    ) -> anyhow::Result<Vec<std::path::PathBuf>> {
        let mut files = Vec::new();
        let mut write = |name: &str, bytes: &[u8]| -> anyhow::Result<()> {
            let path = dir.join(name);
            std::fs::write(&path, bytes).within(format!("writing {}", path.display()))?;
            files.push(path);
            Ok(())
        };
        write("SHA256SUMS", &self.signed.0)?;
        write("SHA256SUMS.sig", &self.signed.1)?;
        for asset in assets {
            write(asset, &self.get(asset).await?)?;
        }
        Ok(files)
    }

    /// Whether the release lists `asset`.
    pub fn has(&self, asset: &str) -> bool {
        self.sums.iter().any(|(name, _)| name == asset)
    }

    /// The names of the assets its signed checksum list holds.
    pub fn assets(&self) -> impl Iterator<Item = &str> {
        self.sums.iter().map(|(name, _)| name.as_str())
    }

    /// A small file of the release that its checksum list doesn't vouch
    /// for, at most [`MAX_SMALL_FILE`] bytes: an outside module's public
    /// key ([`KEY_ASSET`]), which `lnk module add` shows before trusting
    /// it. `at` is the release's folder, of any [`Source`].
    pub async fn unsigned(at: &Source, name: &str) -> anyhow::Result<Vec<u8>> {
        let release = Self {
            at: at.clone(),
            http: client()?,
            sums: Vec::new(),
            signed: Default::default(),
        };
        release.get_raw(name, MAX_SMALL_FILE).await
    }

    /// The version in the release's signed `VERSION` file (`0.12.0`), so a
    /// release can't be passed off under another version's tag
    /// ([`check_installable`]). `None` for a release without one.
    pub async fn version(&self) -> anyhow::Result<Option<link_version::Version>> {
        if !self.has("VERSION") {
            return Ok(None);
        }
        let text = self.get_capped("VERSION", MAX_SMALL_FILE).await?;
        let text = String::from_utf8(text).within("the release's VERSION is not text")?;
        let version = text
            .trim()
            .parse()
            .within("the release's VERSION is not a version")?;
        Ok(Some(version))
    }

    /// `asset`, if its checksum is the signed one.
    pub async fn get(&self, asset: &str) -> anyhow::Result<Vec<u8>> {
        self.get_capped(asset, MAX_ARCHIVE).await
    }

    async fn get_capped(&self, asset: &str, max: u64) -> anyhow::Result<Vec<u8>> {
        let expected = self
            .sums
            .iter()
            .find(|(name, _)| name == asset)
            .map(|(_, hash)| hash.clone())
            .within(format!("{asset} is not in the release"))?;
        let archive = self.get_raw(asset, max).await?;
        let actual = link_digest::sha256(&archive);
        ensure!(
            actual == expected,
            "checksum mismatch for {asset}; not installing it"
        );
        Ok(archive)
    }

    /// `name` from the release, refused once it is over `max` bytes (as
    /// announced, or as it arrives).
    async fn get_raw(&self, name: &str, max: u64) -> anyhow::Result<Vec<u8>> {
        self.at.read(&self.http, name, max).await
    }
}

/// `base`, a release's address.
fn web(base: &str) -> anyhow::Result<Source> {
    Ok(Source::Web(
        base.parse().map_err(|e: String| anyhow::anyhow!(e))?,
    ))
}

/// The (name, SHA-256) pairs of a `SHA256SUMS` list. A line whose name
/// holds anything but `[A-Za-z0-9._-]` is dropped: no asset of a release
/// is named so, and a name is printed before its key is trusted.
fn parse_sums(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| l.split_once(char::is_whitespace))
        .map(|(hash, name)| {
            (
                name.trim().trim_start_matches('*').to_string(),
                hash.to_ascii_lowercase(),
            )
        })
        .filter(|(name, _)| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
        .collect()
}

/// The client for release servers: a release's files are public, and a
/// host's (GitHub's) move to another, so its redirects are followed, to
/// `https://` alone. Each file is read as long as it takes, each read
/// within [`READ_TIMEOUT`].
fn client() -> anyhow::Result<link_fetch::Client> {
    Ok(link_fetch::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .streaming(READ_TIMEOUT)
        .outside()
        .follow_redirects(10)
        .build()?)
}

/// The asset of a module's release from outside Link holding the public
/// half of the key its `SHA256SUMS` is signed with: an OpenSSH public key
/// (`ssh-ed25519 AAAA... comment`), or an allowed-signers line.
pub const KEY_ASSET: &str = "release-key.pub";

/// The one Ed25519 key in `text` (a public key or an allowed-signers
/// line), as `ssh-ed25519 <base64>`, without a comment: how `lnk` pins it.
pub fn one_key(text: &str) -> anyhow::Result<String> {
    let keys = signer_keys(text)?;
    ensure!(
        keys.len() == 1,
        "{} keys where one was expected",
        keys.len()
    );
    let mut blob = Vec::new();
    for field in [b"ssh-ed25519".as_slice(), keys[0].as_slice()] {
        blob.extend_from_slice(&(field.len() as u32).to_be_bytes());
        blob.extend_from_slice(field);
    }
    Ok(format!(
        "ssh-ed25519 {}",
        base64::engine::general_purpose::STANDARD.encode(blob)
    ))
}

/// A key's fingerprint as `ssh-keygen -l` shows it (`SHA256:...`), from
/// `key` as [`one_key`] makes it.
pub fn fingerprint(key: &str) -> anyhow::Result<String> {
    let blob = key
        .split_whitespace()
        .nth(1)
        .within("not an SSH public key")?;
    let blob = base64::engine::general_purpose::STANDARD.decode(blob)?;
    let hash = link_digest::sha256_bytes(&blob);
    Ok(format!(
        "SHA256:{}",
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(hash)
    ))
}

/// Downloads `asset` from `base`, and returns it only if `SHA256SUMS` is
/// signed by one of `signers` and lists the asset's checksum.
pub async fn download(base: &str, asset: &str, signers: &str) -> anyhow::Result<Download> {
    let release = Release::fetch(base, signers).await?;
    Ok(Download {
        archive: release.get(asset).await?,
        asset: asset.to_string(),
    })
}

/// Unpacks a `.tar.gz` archive into `dir`.
pub fn unpack(archive: &[u8], dir: &std::path::Path) -> anyhow::Result<()> {
    let path = dir.join("archive.tar.gz");
    std::fs::write(&path, archive)?;
    let status = link_secretive::process::command("tar")
        .arg("-xzf")
        .arg(&path)
        .arg("-C")
        .arg(dir)
        .status()
        .within("running tar")?;
    let _ = std::fs::remove_file(&path);
    ensure!(status.success(), "could not unpack the archive");
    Ok(())
}

/// Moves the unpacked program `new` over `path`: a rename, atomic and safe
/// while the old one runs.
pub fn put(new: &std::path::Path, path: &std::path::Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    std::fs::set_permissions(new, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
    std::fs::rename(new, path).within(format!("replacing {}", path.display()))
}

/// Reads SSH wire-format fields.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> anyhow::Result<&'a [u8]> {
        ensure!(self.0.len() >= n, "truncated signature data");
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn u32(&mut self) -> anyhow::Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into()?))
    }

    fn string(&mut self) -> anyhow::Result<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }
}

fn parse_public_key(r: &mut Reader<'_>) -> anyhow::Result<[u8; 32]> {
    ensure!(r.string()? == b"ssh-ed25519", "not an Ed25519 key");
    Ok(r.string()?.try_into()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `packages/test-kit/fixtures/test-release-key.pub`, a key used only in tests.
    const TEST_SIGNERS: &str = "link-release namespaces=\"link-release\" ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIH9+OiZ/SeNGnTeS94BYhtoALcCIHMNqITeJujSzZeHW test\n";
    const MESSAGE: &[u8] = b"SHA-256 checksums\n";
    /// `ssh-keygen -Y sign -f test-release-key -n link-release` of MESSAGE.
    const SIGNATURE: &str = "-----BEGIN SSH SIGNATURE-----
U1NIU0lHAAAAAQAAADMAAAALc3NoLWVkMjU1MTkAAAAgf346Jn9J40adN5L3gFiG2gAtwI
gcw2ohN4m6NLNl4dYAAAAMbGluay1yZWxlYXNlAAAAAAAAAAZzaGE1MTIAAABTAAAAC3Nz
aC1lZDI1NTE5AAAAQMcdubyAB5+7ODRBNcRJBtaVuUZcZTkjA0DyEX+yMVEhHrobpdvkwJ
flBddgU1LTE7+JiJlfowe/XCDkLa6dFwA=
-----END SSH SIGNATURE-----
";
    /// The same, in the namespace `other-namespace`.
    const OTHER_NAMESPACE: &str = "-----BEGIN SSH SIGNATURE-----
U1NIU0lHAAAAAQAAADMAAAALc3NoLWVkMjU1MTkAAAAgf346Jn9J40adN5L3gFiG2gAtwI
gcw2ohN4m6NLNl4dYAAAAPb3RoZXItbmFtZXNwYWNlAAAAAAAAAAZzaGE1MTIAAABTAAAA
C3NzaC1lZDI1NTE5AAAAQPR5gIFwwuroqtkcBLNbpcrMHeLRWQIyn5S67V1uUynJmgfREY
yMBPHfVbN3oJI4DCn0jxwsJt7ik8IX9gNdvQ4=
-----END SSH SIGNATURE-----
";

    #[test]
    fn accepts_a_good_signature() {
        let keys = signer_keys(TEST_SIGNERS).unwrap();
        verify(MESSAGE, SIGNATURE, &keys).unwrap();
    }

    #[test]
    fn rejects_tampering_other_keys_and_other_namespaces() {
        let keys = signer_keys(TEST_SIGNERS).unwrap();
        let err = verify(b"SHA-256 checksums (edited)\n", SIGNATURE, &keys).unwrap_err();
        assert!(err.to_string().contains("doesn't match"), "{err}");

        let err = verify(MESSAGE, OTHER_NAMESPACE, &keys).unwrap_err();
        assert!(err.to_string().contains("not \"link-release\""), "{err}");

        let err = verify(MESSAGE, SIGNATURE, &[[7; 32]]).unwrap_err();
        assert!(err.to_string().contains("doesn't trust"), "{err}");

        assert!(verify(MESSAGE, "garbage", &keys).is_err());
        assert!(verify(MESSAGE, "", &keys).is_err());
    }

    /// Serves `body` once on a local port, with or without a
    /// Content-Length; returns the base URL.
    fn serve_once(body: Vec<u8>, content_length: bool) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let head = if content_length {
                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len())
            } else {
                "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_string()
            };
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        });
        format!("http://127.0.0.1:{port}")
    }

    #[tokio::test]
    async fn oversized_downloads_are_refused() {
        for content_length in [true, false] {
            let base = serve_once(vec![b'a'; MAX_SMALL_FILE as usize + 1], content_length);
            let err = Release::fetch(&base, TEST_SIGNERS).await.err().unwrap();
            assert!(
                format!("{err:#}").contains("SHA256SUMS is larger than"),
                "{err:#}"
            );
        }
    }

    #[test]
    fn a_key_is_pinned_without_its_comment_and_shown_by_fingerprint() {
        let key = one_key(TEST_SIGNERS).unwrap();
        assert_eq!(
            key,
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIH9+OiZ/SeNGnTeS94BYhtoALcCIHMNqITeJujSzZeHW"
        );
        assert_eq!(one_key(&format!("{key} me@example")).unwrap(), key);
        // `ssh-keygen -lf test-release-key.pub`.
        assert_eq!(
            fingerprint(&key).unwrap(),
            "SHA256:GWLqop+1eDDRfEFL37pyGjhDFEdvXsA+U/v7JGxNtgU"
        );
        assert!(one_key(&format!("{key}\n{key}\n")).is_err());
        assert!(one_key("nothing").is_err());
    }

    #[test]
    fn sum_lines_with_odd_names_are_dropped() {
        let sums = parse_sums(
            "AB12  lnk-a-x.tar.gz\n\
             cd34 *SHA256SUMS.sig\n\
             ef56  lnk-\u{1b}]0;title\u{7}-x.tar.gz\n\
             0000  ../up\n\
             1111  with space\n",
        );
        assert_eq!(
            sums,
            vec![
                ("lnk-a-x.tar.gz".to_string(), "ab12".to_string()),
                ("SHA256SUMS.sig".to_string(), "cd34".to_string()),
            ]
        );
    }

    /// A test's signers are trusted by a debug build alone: a release
    /// (`cargo test --release`) ignores `LNK_TEST_RELEASE_SIGNERS`.
    #[test]
    fn a_test_signer_is_trusted_only_in_a_debug_build() {
        let dir = link_test_kit::TempDir::new("release-test-signers");
        let file = dir.join("signers");
        std::fs::write(&file, TEST_SIGNERS).unwrap();
        // SAFETY: the only test that sets or reads this variable.
        unsafe { std::env::set_var(super::TEST_SIGNERS, &file) };
        let trusted = trusted_signers();
        assert!(trusted.starts_with(TRUSTED_SIGNERS));
        assert_eq!(trusted.contains(TEST_SIGNERS), cfg!(debug_assertions));
    }

    /// One rule for what's installed: it says its version, the one asked
    /// for, and the latest not before a released lnk.
    #[test]
    fn what_may_be_installed() {
        let v = |t: &str| t.parse::<link_version::Version>().unwrap();
        let ok = |said: Option<&str>, asked: Option<&str>, floor: Option<&str>| {
            check_installable(said.map(v), asked.map(v).as_ref(), floor.map(v).as_ref())
        };
        assert_eq!(
            ok(Some("0.14.0"), Some("0.14.0"), None).unwrap(),
            v("0.14.0")
        );
        assert!(
            ok(
                Some("0.14.0-staging.1"),
                Some("0.14.0-staging.1"),
                Some("0.15.0")
            )
            .is_ok()
        );
        assert!(ok(Some("0.13.0"), Some("0.14.0"), None).is_err());
        // One that doesn't say, asked for by version or not.
        for asked in [Some("0.14.0"), None] {
            let err = ok(None, asked, None).unwrap_err();
            assert!(err.to_string().contains("no VERSION"), "{err}");
        }
        assert!(ok(Some("0.14.0"), None, Some("0.14.0")).is_ok());
        assert!(ok(Some("0.15.0-staging.1"), None, Some("0.14.0")).is_ok());
        let err = ok(Some("0.13.9"), None, Some("0.14.0")).unwrap_err();
        assert!(err.to_string().contains("behind this lnk"), "{err}");
        assert!(ok(Some("0.14.0-staging.1"), None, Some("0.14.0")).is_err());
        // A source build's lnk names no floor: any release it says.
        assert!(ok(Some("0.13.0"), None, None).is_ok());
        let url = "https://builds.example/staging".parse().unwrap();
        assert_eq!(
            base(&url, None).unwrap().to_string(),
            "https://builds.example/staging/latest/download"
        );
        assert_eq!(
            base(&url, Some(&v("0.14.0-staging.1")))
                .unwrap()
                .to_string(),
            "https://builds.example/staging/download/v0.14.0-staging.1"
        );
        let bucket = "s3://my-builds/dev/0123".parse().unwrap();
        assert_eq!(
            base(&bucket, Some(&v("0.14.0-dev.1"))).unwrap().to_string(),
            "s3://my-builds/dev/0123/download/v0.14.0-dev.1"
        );
    }

    #[test]
    fn the_shipped_signers_file_parses() {
        assert!(!signer_keys(TRUSTED_SIGNERS).unwrap().is_empty());
        assert!(signer_keys("# nothing\n").is_err());
    }

    /// `LNK_BUILD` is `dev`, `staging`, `prod` or unset; anything else,
    /// `release` among them, is refused (as the program compiles).
    #[test]
    fn a_build_is_read_once_from_lnk_build() {
        use crate::Build;
        assert_eq!(Build::from(None), Build::Source);
        assert_eq!(Build::from(Some("staging")), Build::Staging);
        assert_eq!(Build::from(Some("dev")), Build::Dev);
        assert_eq!(Build::from(Some("prod")), Build::Prod);
        for other in [
            "", "1", "Staging", "release", "qa", "staging1", "Dev", "dev ",
        ] {
            assert!(
                std::panic::catch_unwind(|| Build::from(Some(other))).is_err(),
                "{other:?}"
            );
        }
    }

    /// A build from source trusts Link's signer alone; a staging build,
    /// Link's signer and the staging signer's keys, none of which is
    /// Link's.
    #[test]
    fn a_source_build_trusts_only_links_signer() {
        let release = signer_keys(LINK_SIGNER).unwrap();
        assert_eq!(release.len(), 1);
        if crate::BUILD == crate::Build::Source {
            assert_eq!(TRUSTED_SIGNERS, LINK_SIGNER);
        }
        // Its line ends, so the staging signer's starts a line of its own.
        assert!(LINK_SIGNER.ends_with('\n'));
        let staging = signer_keys(STAGING_BUILD_SIGNERS).unwrap();
        assert_eq!(staging[0], release[0]);
        assert!(
            !staging[1..].contains(&release[0]),
            "the staging signer is Link's"
        );
        // Every line of staging-signers but a comment is a key that's read.
        let staging_lines = include_str!("../../../scripts/release/staging-signers")
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
            .count();
        assert_eq!(staging.len(), 1 + staging_lines);
    }

    /// A prod build trusts the one line of `LNK_PROD_SIGNER` alone, which
    /// is Link's signer only for Link's public release; any other build
    /// reads none of it.
    #[test]
    fn a_prod_build_trusts_only_its_own() {
        if crate::BUILD == crate::Build::Prod {
            assert_eq!(TRUSTED_SIGNERS, PROD_SIGNER);
            let keys = signer_keys(PROD_SIGNER).unwrap();
            assert_eq!(keys.len(), 1);
        } else {
            assert_eq!(PROD_SIGNER, "");
        }
    }

    /// A dev build trusts Link's signer and the one line of
    /// `LNK_DEV_SIGNER`, which isn't Link's; any other build reads
    /// none of it. A line that isn't one signer is refused (as the program
    /// compiles).
    #[test]
    fn a_dev_build_trusts_links_signer_and_its_own() {
        let release = signer_keys(LINK_SIGNER).unwrap();
        if crate::BUILD == crate::Build::Dev {
            assert_eq!(TRUSTED_SIGNERS, DEV_BUILD_SIGNERS);
            let dev = signer_keys(DEV_BUILD_SIGNERS).unwrap();
            assert_eq!(dev.len(), 2);
            assert_eq!(dev[0], release[0]);
            assert_ne!(dev[1], release[0], "the dev signer is Link's");
        } else {
            assert_eq!(DEV_SIGNER, "");
            assert_eq!(DEV_BUILD_SIGNERS, format!("{LINK_SIGNER}\n"));
        }
        let line = LINK_SIGNER.trim_end();
        assert_eq!(signer_line(line), line);
        for broken in [
            "",
            "link-release namespaces=\"link-release\" ssh-ed25519 AAAA",
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIH9+OiZ/SeNGnTeS94BYhtoALcCIHMNqITeJujSzZeHW",
            "link-release namespaces=\"other\" ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIH9+",
            &format!("{line}\n{line}"),
            &format!("{line}\n"),
        ] {
            assert!(
                std::panic::catch_unwind(|| signer_line(broken)).is_err(),
                "{broken:?}"
            );
        }
    }
}
