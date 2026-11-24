//! The rules every name and ID Link makes follows, and what the machine
//! is, a box or your computer ([`kind`], `/etc/lnk/kind.toml`, root's).
//!
//! A name is what people type (`gcp-1`, `main`); an ID is random, never
//! changes, and is what Link keys things on, so two machines' or two
//! users' `main` never collide.

use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

mod constants;
pub use constants::BOX_KIND;

/// A box's installation ID as output and saved state write it,
/// `box:<its installation ID>` ([`BOX_KIND`]).
pub fn box_resource(machine_id: &str) -> String {
    format!("{BOX_KIND}:{machine_id}")
}

/// The installation ID `text` writes as `box:<id>`: none when it's bare, of
/// another kind, or not an ID.
pub fn box_of(text: &str) -> Option<&str> {
    link_id::candidate_of(BOX_KIND, text)
}

/// A box's installation ID, when there is one, kept as `box:<id>` and read
/// back refusing any other form: `#[serde(with = "...::box_field")]`
/// on an `Option<String>`.
pub mod box_field {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        machine_id: &Option<String>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match machine_id {
            Some(id) => serializer.serialize_str(&super::box_resource(id)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<String>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(|text| {
                super::box_of(&text)
                    .map(str::to_string)
                    .ok_or_else(|| serde::de::Error::custom(format!("{text:?} isn't box:<id>")))
            })
            .transpose()
    }
}

/// A machine's processor, core's shared shape (`link-types`).
pub use link_types::Arch;

/// A machine's OS: a box and an environment each name one, core's shared
/// shape (`link-types`). Its provider separately checks which OSes it
/// supports.
pub use link_types::Os;

/// What this machine is, as root says in `/etc/lnk/kind.toml`
/// ([`kind`]): a box, which `lnk box create` made and wrote it on at its
/// first boot, or your computer, where no file says otherwise. Only a
/// box runs what needs root for its setup and has an image saved from
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Kind {
    /// Yours alone: never an image, and nothing Link sets up runs as root.
    Computer {},
    /// Made by `lnk box create`, as its box spec's OS and processor.
    Box { os: Os, arch: Arch },
}

impl Kind {
    pub fn is_box(self) -> bool {
        matches!(self, Kind::Box { .. })
    }
}

/// A file [`kind`] reads in place of root's machine-kind file, owned by whoever runs
/// the test rather than root, for tests of a stand-in box. Read by a
/// debug build only ([`link_secretive::test_var`]).
pub const TEST_KIND_FILE: &str = "LNK_TEST_MACHINE_KIND";

/// What this machine is: `System::MachineKind`'s, a computer without it. The
/// file, and every folder on its way, must be root's and written by no
/// one else (a folder others write only with its sticky bit, as `/tmp`):
/// one that isn't is refused, so no program of a user, an agent or a
/// sandbox makes a computer a box.
pub fn kind() -> anyhow::Result<Kind> {
    match link_secretive::test_var(TEST_KIND_FILE) {
        // SAFETY: geteuid has no failure and touches no memory.
        Some(path) => kind_at(Path::new(&path), unsafe { libc::geteuid() }),
        None => kind_at(&link_files::layout::System::MachineKind.path(), 0),
    }
}

/// [`kind`], by the file at `path`, which `owner` (or root) must own. In
/// a user namespace of its own (`unshare -Urm`), where anyone can mount
/// a file over `/etc/lnk` that seems root's, a computer whatever it says,
/// as best it can tell: the same namespace can mount over `/proc` too.
/// What holds there is that a root run goes through `sudo`, which gains
/// no root in a user namespace.
pub fn kind_at(path: &Path, owner: u32) -> anyhow::Result<Kind> {
    if cfg!(target_os = "linux") {
        let map = std::fs::read_to_string("/proc/self/uid_map").unwrap_or_default();
        if !whole_uid_map(&map) {
            return Ok(Kind::Computer {});
        }
    }
    let owners = [0, owner];
    let trust = link_files::fs::Trust::control(&owners);
    let read = match link_files::fs::read_trusted(path, trust, 4096, "what a machine is") {
        Ok(read) => read,
        Err(err) if link_files::fs::missing(&err) => return Ok(Kind::Computer {}),
        Err(err) => {
            return Err(err.context(format!(
                "{} says what this machine is, and anyone who writes it could make this \
                 machine a box: it's root's alone",
                path.display()
            )));
        }
    };
    link_spec::parse(&read.text).with_context(|| format!("in {}", path.display()))
}

/// The name of the user this program runs as, by its user ID: `USER`
/// can name another user, or none (a cloud container, a unit of
/// systemd's sets none). None where no entry names it, or it can't be
/// read. Read into a buffer of its own (`getpwuid_r`), never the one
/// `getpwuid` shares with every other thread's lookup.
pub fn user_name() -> Option<String> {
    // SAFETY: geteuid has no failure.
    user_name_of(unsafe { libc::geteuid() })
}

/// The name of the user whose ID is `uid`, as [`user_name`] reads it:
/// none where no entry names it, or it can't be read.
pub fn user_name_of(uid: u32) -> Option<String> {
    let mut size = 1024;
    loop {
        let mut buffer = vec![0 as libc::c_char; size];
        // SAFETY: zeroed is a valid passwd; getpwuid_r fills it, pointing
        // only into `buffer`, which outlives every read of it below.
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut found: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: the pointers are to the locals above and `buffer` of
        // `size` bytes.
        let done =
            unsafe { libc::getpwuid_r(uid, &mut entry, buffer.as_mut_ptr(), size, &mut found) };
        match done {
            libc::ERANGE if size < 1 << 20 => size *= 4,
            0 if !found.is_null() && !entry.pw_name.is_null() => {
                // SAFETY: a NUL-ended name in `buffer`, still alive.
                let name = unsafe { std::ffi::CStr::from_ptr(entry.pw_name) }.to_string_lossy();
                return (!name.is_empty()).then(|| name.into_owned());
            }
            _ => return None,
        }
    }
}

/// Whether `uid_map` (`/proc/self/uid_map`'s) is the machine's own: every
/// user ID as itself, as outside any user namespace.
pub fn whole_uid_map(uid_map: &str) -> bool {
    let lines: Vec<Vec<&str>> = uid_map
        .lines()
        .map(|l| l.split_whitespace().collect())
        .filter(|f: &Vec<&str>| !f.is_empty())
        .collect();
    lines == [vec!["0", "0", "4294967295"]]
}

/// The longest a name may be.
pub use crate::names::NAME_MAX;

pub use crate::names::{WORDS, check_id, check_name, check_shape, name_from_host};

/// This machine's host name, or nothing when it can't be read: asked of
/// the system, which starts no program, where it can be.
pub(crate) fn host_name() -> String {
    #[cfg(unix)]
    {
        let mut name = [0u8; 256];
        // The buffer is writable for its whole length, which is all the
        // system may write; the name ends at its first NUL, or the end.
        if unsafe { libc::gethostname(name.as_mut_ptr().cast(), name.len()) } == 0 {
            let end = name.iter().position(|b| *b == 0).unwrap_or(name.len());
            let name = String::from_utf8_lossy(&name[..end]).trim().to_string();
            if !name.is_empty() {
                return name;
            }
        }
    }
    link_secretive::process::command("hostname")
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This user's name stays this user's while another thread looks up
    /// another's: never read from the buffer `getpwuid` shares.
    #[test]
    fn this_users_name_is_its_own_whatever_others_look_up() {
        let mine = user_name();
        // Another user than this one: root's lookup, or nobody's as root.
        // SAFETY: geteuid has no failure.
        let other: libc::uid_t = if unsafe { libc::geteuid() } == 0 {
            65534
        } else {
            0
        };
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let looking = {
            let stop = stop.clone();
            std::thread::spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    // SAFETY: the shared entry isn't read here.
                    unsafe { libc::getpwuid(other) };
                }
            })
        };
        let wrong = (0..20_000).filter(|_| user_name() != mine).count();
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        looking.join().unwrap();
        assert_eq!(wrong, 0, "{wrong} lookups weren't {mine:?}");
    }

    /// A machine is a box only where a file no one else writes says so:
    /// a computer with no file; a box as written; and refused when the
    /// file or its folder is written by others, a link, or holds what no
    /// box has.
    #[test]
    fn a_machine_is_a_box_only_where_a_file_no_one_else_writes_says_so() {
        use std::os::unix::fs::PermissionsExt;
        let temp = link_test_kit::TempDir::new("machine-kind");
        let dir = temp.join("etc");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = dir.join("machine.toml");
        // SAFETY: geteuid has no failure and touches no memory.
        let me = unsafe { libc::geteuid() };
        assert_eq!(kind_at(&path, me).unwrap(), Kind::Computer {});
        let made = Kind::Box {
            os: Os::Ubuntu2404,
            arch: Arch::Arm64,
        };
        let text = toml::to_string(&made).unwrap();
        assert_eq!(
            text,
            "kind = \"box\"\nos = \"ubuntu-24.04\"\narch = \"arm64\"\n"
        );
        std::fs::write(&path, &text).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(kind_at(&path, me).unwrap(), made);
        assert!(made.is_box() && !Kind::Computer {}.is_box());
        // Root's alone, when root isn't who runs it: this test's file isn't.
        if me != 0 {
            assert!(kind_at(&path, 0).is_err());
        }
        // Written by others: the file, then its folder.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(kind_at(&path, me).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o775)).unwrap();
        let err = kind_at(&path, me).unwrap_err();
        let refused = err
            .chain()
            .find_map(|e| e.downcast_ref::<link_files::fs::Untrusted>())
            .unwrap_or_else(|| panic!("{err:#}"));
        assert_eq!(refused.why, link_files::fs::Distrust::Written, "{err:#}");
        assert_eq!(refused.folder.file_name(), dir.file_name(), "{err:#}");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Through a link in a folder others write: refused too.
        let open = temp.join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
        let link = open.join("machine.toml");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(kind_at(&link, me).is_err());
        std::fs::remove_file(&link).unwrap();
        std::fs::write(&link, &text).unwrap();
        std::fs::set_permissions(&link, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(kind_at(&link, me).is_err());
        // What no box has.
        for wrong in [
            "kind = \"box\"\n",
            "kind = \"server\"\n",
            "kind = \"computer\"\nos = \"ubuntu-24.04\"\n",
            &format!("{text}root = true\n"),
        ] {
            std::fs::write(&path, wrong).unwrap();
            assert!(kind_at(&path, me).is_err(), "{wrong}");
        }
        std::fs::write(&path, "kind = \"computer\"\n").unwrap();
        assert_eq!(kind_at(&path, me).unwrap(), Kind::Computer {});
    }

    /// Only the machine's own user IDs read the file: in a plain user
    /// namespace of its own, where a file mounted over it seems root's,
    /// no machine is a box.
    #[test]
    fn a_user_namespace_of_its_own_is_never_a_box() {
        assert!(whole_uid_map("         0          0 4294967295\n"));
        assert!(!whole_uid_map("         0       1000          1\n"));
        assert!(!whole_uid_map("0 100000 65536\n"));
        assert!(!whole_uid_map("0 0 4294967295\n1 1 1\n"));
        assert!(!whole_uid_map(""));
    }

    #[test]
    fn ids_are_random_and_plain() {
        let (a, b) = (link_id::candidate().unwrap(), link_id::candidate().unwrap());
        assert_ne!(a, b);
        check_id(&a).unwrap();
        for bad in ["", "ABCDEFGHIJKL", "abc", "abcdefghijk-", "abcdefghijklm"] {
            assert!(check_id(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn names() {
        for good in ["main", "moltbot-2", "gcp-1", "a"] {
            check_name(good).unwrap();
        }
        for bad in [
            "",
            "here",
            "all",
            "1box",
            "-a",
            "Main",
            "a_b",
            "a@b",
            "a.b",
            &"a".repeat(31),
        ] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn host_names_are_made_to_fit() {
        assert_eq!(name_from_host("Danas-MBP.local"), "danas-mbp");
        assert_eq!(name_from_host("ip-10-0-0-1"), "ip-10-0-0-1");
        assert_eq!(name_from_host("my_box--2\n"), "my-box-2");
        assert_eq!(name_from_host("42-server"), "server");
        assert_eq!(name_from_host(&"x".repeat(40)), "x".repeat(30));
        assert_eq!(name_from_host(""), "computer");
        assert_eq!(name_from_host("here"), "computer");
        assert_eq!(name_from_host("Home.local"), "home");
        assert_eq!(name_from_host("All.local"), "computer");
        assert_eq!(name_from_host("1234"), "computer");
    }
}
