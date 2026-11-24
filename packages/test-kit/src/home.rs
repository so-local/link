//! A home of its own for each test program, so that nothing a test does
//! reads or writes the real `~/.lnk` or `~/Link`.
//!
//! Link finds its folders from `HOME`. A test program whose root holds
//! [`program!`](crate::program!) starts with `HOME` at a folder the kit makes
//! new for it, whenever `HOME` would otherwise be the account's own home:
//! code a test calls in-process, and a program it starts without
//! [`env::command`](crate::env::command), find Link's folders there. The
//! folder is removed when the program exits. The repository checks refuse
//! a test program whose root lacks it.
//!
//! Its runtime folder, where Link keeps its sockets, is that home's `.run`,
//! as [`env::clean`](crate::env::clean)
//! gives a child's: never the machine-wide one (`/run/user/<uid>`, else a
//! folder in the system's temp folder), which every test program would
//! share with each other and with the Link this account runs.
//!
//! The toolchain stays the account's: `CARGO_HOME` and `RUSTUP_HOME`, when
//! unset, name the account's own, so a `cargo` a test starts finds them.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The home [`own`] made, removed when the program exits.
static MADE: Mutex<Option<crate::TempDir>> = Mutex::new(None);

/// Sets `HOME` to a folder made new for this program when it is unset,
/// relative or the account's own ([`account`]); else, as in a program a
/// test started with its own home, leaves it, even when it's the own home
/// of a user a test made in a test folder, and the rest of the
/// environment the test gave it. A home made here is given its runtime
/// folder too, `XDG_RUNTIME_DIR` its `.run`. Called once, by
/// [`program!`](crate::program!), before `main`.
pub fn own() {
    let made = give_home();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if made && let Some(home) = home.filter(|home| home.is_absolute()) {
        let run = crate::env::made_runtime(&home);
        // SAFETY: before `main`, with no other thread to read it.
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", run) };
    }
}

/// `HOME`, made new for this program unless it was given one of its own:
/// whether it was made.
fn give_home() -> bool {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let account = account();
    if home.as_deref().is_some_and(|home| {
        home.is_absolute()
            && (!is_own(home, account.as_deref()) || crate::temp::in_test_folder(home))
    }) {
        return false;
    }
    if let Some(account) = &account {
        for (name, folder) in [("CARGO_HOME", ".cargo"), ("RUSTUP_HOME", ".rustup")] {
            let folder = account.join(folder);
            if std::env::var_os(name).is_none() && folder.is_dir() {
                // SAFETY: before `main`, with no other thread to read it.
                unsafe { std::env::set_var(name, folder) };
            }
        }
    }
    let made = crate::TempDir::short_out_of_tmp("home");
    // SAFETY: before `main`, with no other thread to read it.
    unsafe { std::env::set_var("HOME", made.path()) };
    *MADE.lock().unwrap_or_else(|e| e.into_inner()) = Some(made);
    extern "C" fn remove() {
        drop(MADE.lock().unwrap_or_else(|e| e.into_inner()).take());
    }
    // SAFETY: `remove` is an `extern "C"` function with no arguments.
    unsafe { libc::atexit(remove) };
    true
}

/// Whether `home` is the account's home `account`, however either is
/// spelled. With no account home known, any home might be it.
fn is_own(home: &Path, account: Option<&Path>) -> bool {
    let real = |path: &Path| path.canonicalize().unwrap_or(path.to_path_buf());
    account.is_none_or(|account| real(account) == real(home))
}

/// The home the system keeps for the account this program runs as,
/// whatever `HOME` says.
#[cfg(unix)]
pub fn account() -> Option<PathBuf> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;
    let mut buffer = vec![0u8; 4096];
    let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut found = std::ptr::null_mut();
    // SAFETY: each pointer is to memory of its size, owned here, and the
    // entry is read only when the call found one.
    let done = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            entry.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut found,
        )
    };
    if done != 0 || found.is_null() {
        return None;
    }
    // SAFETY: the call found an entry, whose `pw_dir` points into `buffer`.
    let dir = unsafe { CStr::from_ptr((*found).pw_dir) };
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(dir.to_bytes())))
}

/// The account's home, on Windows its profile folder.
#[cfg(windows)]
pub fn account() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn this_program_has_a_home_of_its_own() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        assert!(!is_own(&home, account().as_deref()), "{}", home.display());
        assert!(home.is_dir());
        // A system program's runner gave this test's process its home;
        // started any other way, the program made it.
        if std::env::var_os(crate::program::ONE).is_none() {
            let made = MADE.lock().unwrap();
            assert_eq!(made.as_ref().map(|dir| dir.path()), Some(home.as_path()));
        }
        let run = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
        assert_eq!(run, crate::env::runtime(&home));
    }

    #[test]
    fn only_the_accounts_own_home_is_set_aside() {
        let dir = crate::TempDir::new("home");
        assert!(is_own(dir.path(), Some(dir.path())));
        assert!(!is_own(dir.path(), Some(&dir.path().join("other"))));
        assert!(is_own(dir.path(), None));
    }

    #[test]
    fn a_test_users_home_in_a_test_folder_is_its_own() {
        let dir = crate::TempDir::new("home");
        assert!(crate::temp::in_test_folder(&dir.path().join("users/a")));
        assert!(!crate::temp::in_test_folder(Path::new("/home/me")));
        assert!(!crate::temp::in_test_folder(Path::new("/home/lnk-tools")));
    }

    #[test]
    fn a_program_started_with_its_own_home_keeps_it() {
        let home = crate::TempDir::new("home");
        let output = crate::env::command(std::env::current_exe().unwrap(), &home)
            .args(["--exact", "home::tests::prints_its_home", "--nocapture"])
            .env("LNK_TEST_PRINT_HOME", "1")
            .output()
            .unwrap();
        let printed = String::from_utf8_lossy(&output.stdout);
        let line = format!("home={}", home.path().display());
        assert!(printed.contains(&line), "{printed}");
    }

    /// What [`a_program_started_with_its_own_home_keeps_it`] runs.
    #[test]
    fn prints_its_home() {
        if std::env::var_os("LNK_TEST_PRINT_HOME").is_some() {
            println!("home={}", std::env::var("HOME").unwrap());
        }
    }
}
