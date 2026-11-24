//! Who a test runs as, and running a program as another user.

#[cfg(windows)]
compile_error!("link-test-kit: no Windows implementation of users, which EO1 adds");

/// Whether the test runs as root (its effective user).
#[cfg(unix)]
pub fn root() -> bool {
    // SAFETY: a plain getter.
    unsafe { libc::geteuid() == 0 }
}

/// The name of the user the test runs as, by its user ID, never `USER`,
/// which can name another or none. Fails the test where no entry names it.
#[cfg(unix)]
pub fn me() -> String {
    // SAFETY: a plain getter.
    let uid = unsafe { libc::geteuid() };
    named(|buffer, size| {
        // SAFETY: zeroed is a valid passwd; getpwuid_r fills it, pointing
        // only into `buffer`, of `size` bytes, which outlives it.
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut found = std::ptr::null_mut();
        let done = unsafe { libc::getpwuid_r(uid, &mut entry, buffer, size, &mut found) };
        (done, (!found.is_null()).then_some(entry.pw_name))
    })
    .unwrap_or_else(|| panic!("no user entry names this test's user, uid {uid}"))
}

/// The name of the test's group (its effective group), by its ID. Fails
/// the test where no entry names it.
#[cfg(unix)]
pub fn my_group() -> String {
    // SAFETY: a plain getter.
    let gid = unsafe { libc::getegid() };
    named(|buffer, size| {
        // SAFETY: as in `me`, for a group.
        let mut entry: libc::group = unsafe { std::mem::zeroed() };
        let mut found = std::ptr::null_mut();
        let done = unsafe { libc::getgrgid_r(gid, &mut entry, buffer, size, &mut found) };
        (done, (!found.is_null()).then_some(entry.gr_name))
    })
    .unwrap_or_else(|| panic!("no group entry names this test's group, gid {gid}"))
}

/// The name a `getpwuid_r`-like `lookup` finds, into a buffer of its own
/// grown until it fits: never one `getpwuid` shares with every thread.
#[cfg(unix)]
fn named(
    lookup: impl Fn(*mut libc::c_char, usize) -> (libc::c_int, Option<*mut libc::c_char>),
) -> Option<String> {
    let mut size = 1024;
    loop {
        let mut buffer = vec![0 as libc::c_char; size];
        match lookup(buffer.as_mut_ptr(), size) {
            (libc::ERANGE, _) if size < 1 << 20 => size *= 4,
            (0, Some(name)) if !name.is_null() => {
                // SAFETY: a NUL-ended name in `buffer`, still alive.
                let name = unsafe { std::ffi::CStr::from_ptr(name) }.to_string_lossy();
                return (!name.is_empty()).then(|| name.into_owned());
            }
            _ => return None,
        }
    }
}

/// `program`, run as the user `uid` in group `gid` and the groups
/// `groups` (`setpriv`), with the kit's environment, `home` its `HOME`
/// ([`crate::env`]). Only root may; `home` and its `.tmp` are the
/// caller's to give that user.
#[cfg(target_os = "linux")]
pub fn command_as(
    uid: u32,
    gid: u32,
    groups: &[u32],
    program: impl AsRef<std::ffi::OsStr>,
    home: &std::path::Path,
) -> std::process::Command {
    let groups: Vec<String> = groups.iter().map(u32::to_string).collect();
    let mut command = crate::env::command("/usr/bin/setpriv", home);
    command
        .args(["--reuid", &uid.to_string(), "--regid", &gid.to_string()])
        .args(match groups.is_empty() {
            true => vec!["--clear-groups".to_string()],
            false => vec!["--groups".to_string(), groups.join(",")],
        })
        .arg(program);
    command
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn the_test_is_who_the_system_says() {
        let said = |args: &[&str]| {
            let output = crate::env::command("id", &crate::TempDir::new("users"))
                .args(args)
                .output()
                .unwrap();
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        assert_eq!(me(), said(&["-un"]));
        assert_eq!(my_group(), said(&["-gn"]));
        assert_eq!(root(), said(&["-u"]) == "0");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn root_runs_a_program_as_another_user() {
        if !crate::gate::root() {
            return;
        }
        let home = crate::TempDir::new("users-as");
        let output = command_as(65534, 65534, &[], "id", &home)
            .arg("-u")
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "65534",
            "{output:?}"
        );
    }
}
