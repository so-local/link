//! Bubblewrap's report of the sandbox it made (its `--info-fd`): JSON
//! naming the sandbox's first process, `{"child-pid": 123, ...}`, as
//! this machine's processes see it, written once its namespaces are
//! made. What the proxy joins for its DNS, and where the stage in front
//! of bubblewrap sends a stop.

use std::io::Read;

use crate::constants::MOST_INFO;

/// Bubblewrap's report on `info`, to the closing brace of its JSON or
/// its end, and at most [`MOST_INFO`]. It comes in several writes, each
/// read before `info` is closed: a write to a closed pipe would kill
/// bubblewrap (SIGPIPE) before its sandbox starts.
pub fn read_info(info: impl Read) -> std::io::Result<String> {
    let mut info = info.take(MOST_INFO);
    let mut text = String::new();
    let mut chunk = [0u8; 512];
    loop {
        let n = match info.read(&mut chunk) {
            Ok(n) => n,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        if n == 0 {
            return Ok(text);
        }
        text.push_str(&String::from_utf8_lossy(&chunk[..n]));
        if text.trim_end().ends_with('}') {
            return Ok(text);
        }
    }
}

/// The `child-pid` bubblewrap's report names: the sandbox's first
/// process, as this machine's processes see it.
pub fn child_pid(text: &str) -> Option<u32> {
    let (_, rest) = text.split_once("\"child-pid\":")?;
    let digits: String = rest
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bubblewraps_info_is_read_whole() {
        // Its info comes in several writes; each is read before the pipe
        // closes, or bubblewrap dies writing the rest.
        struct Writes(Vec<&'static [u8]>, usize);
        impl std::io::Read for Writes {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let Some(next) = self.0.get(self.1) else {
                    return Ok(0);
                };
                self.1 += 1;
                buf[..next.len()].copy_from_slice(next);
                Ok(next.len())
            }
        }
        let mut info = Writes(
            vec![
                b"{\n    \"child-pid\": 24448",
                b",\n    \"cgroup-namespace\": 4026531835",
                b",\n    \"ipc-namespace\": 4026532",
                b",\n    \"mnt-namespace\": 4026532",
                b",\n    \"net-namespace\": 4026532",
                b",\n    \"pid-namespace\": 4026532",
                b",\n    \"uts-namespace\": 4026532",
                b"\n}\n",
            ],
            0,
        );
        let text = read_info(&mut info).unwrap();
        assert_eq!(info.1, 8, "every write read");
        assert_eq!(child_pid(&text), Some(24448));
        assert_eq!(child_pid("{\"child-pid\": x}"), None);
    }
}
