//! The display's cookie: an `Xauthority` file of one entry, any host,
//! the cookie `MIT-MAGIC-COOKIE-1` takes, read only by your user, made
//! when the desktop is added and renewed each time the tool starts its X
//! server. `Xvfb -auth` takes it, and a program on the display
//! finds it at `$XAUTHORITY`.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};

/// The kind of cookie.
pub const NAME: &[u8] = b"MIT-MAGIC-COOKIE-1";

/// An entry for any host (`FamilyWild`).
const WILD: u16 = 0xffff;

/// Makes the cookie's file for display `display` at `path`, unless it's
/// there.
pub fn make(path: &Path, display: u32) -> Result<()> {
    if path.exists() {
        read(path)?;
        return Ok(());
    }
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)
            .with_context(|| format!("creating {}", folder.display()))?;
    }
    write_new(path, &entry(display)?)
}

/// Gives display `display` a new cookie in the file at `path`, as each
/// X server the tool starts takes: written beside it and moved over it,
/// so a program reading it finds the old cookie or the new one whole.
pub fn renew(path: &Path, display: u32) -> Result<()> {
    let name = path
        .file_name()
        .context("a cookie's file has a name")?
        .to_string_lossy();
    let next = path.with_file_name(format!("{name}.new"));
    // One left by a renewal that stopped partway.
    match std::fs::remove_file(&next) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            return Err(err).with_context(|| format!("removing {}", next.display()));
        }
        _ => {}
    }
    write_new(&next, &entry(display)?)?;
    std::fs::rename(&next, path).with_context(|| format!("renewing {}", path.display()))
}

/// The file's one entry: display `display`'s new random cookie, for any
/// host.
fn entry(display: u32) -> Result<Vec<u8>> {
    let mut cookie = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut cookie))
        .context("a random cookie")?;
    let mut entry = Vec::new();
    let field = |entry: &mut Vec<u8>, bytes: &[u8]| {
        entry.extend((bytes.len() as u16).to_be_bytes());
        entry.extend(bytes);
    };
    entry.extend(WILD.to_be_bytes());
    field(&mut entry, b"");
    field(&mut entry, display.to_string().as_bytes());
    field(&mut entry, NAME);
    field(&mut entry, &cookie);
    Ok(entry)
}

/// Writes `entry` to a new file at `path`, read only by your user: one
/// there already, a link included, is refused.
fn write_new(path: &Path, entry: &[u8]) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(path)
        .with_context(|| format!("writing {}", path.display()))?;
    std::io::Write::write_all(&mut file, entry)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// The cookie in the file at `path`.
pub fn read(path: &Path) -> Result<Vec<u8>> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let mut at = 2usize;
    let mut field = || -> Result<&[u8]> {
        let len = bytes
            .get(at..at + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]) as usize)
            .context("a short cookie file")?;
        let value = bytes
            .get(at + 2..at + 2 + len)
            .context("a short cookie file")?;
        at += 2 + len;
        Ok(value)
    };
    let (_address, _display, name, cookie) = (field()?, field()?, field()?, field()?);
    if name != NAME {
        bail!("{} holds no MIT-MAGIC-COOKIE-1", path.display());
    }
    ensure!(
        cookie.len() == 16,
        "{}: a cookie is 16 bytes",
        path.display()
    );
    Ok(cookie.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cookie_is_kept_private_and_renewed_whole() {
        let dir = link_test_kit::TempDir::new("desktop-auth");
        let path = dir.join("desktop/auth");
        make(&path, 120).unwrap();
        let cookie = read(&path).unwrap();
        make(&path, 120).unwrap();
        assert_eq!(read(&path).unwrap(), cookie);
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        renew(&path, 120).unwrap();
        let renewed = read(&path).unwrap();
        assert_ne!(renewed, cookie);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(std::fs::read_dir(dir.join("desktop")).unwrap().count(), 1);
        std::fs::write(&path, b"junk").unwrap();
        assert!(read(&path).is_err());
    }
}
