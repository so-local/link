//! Runtime context goes to one child on a pipe, never in argv or a file.
use crate::RunWith;
use anyhow::{Context, ensure};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};

const MOST: u64 = 1024 * 1024;

pub struct RunHandoff {
    reader: std::io::PipeReader,
    writer: std::io::PipeWriter,
    bytes: Vec<u8>,
}
impl RunWith {
    /// Read and close a descriptor handed to this child alone.
    pub fn read_fd(fd: i32) -> anyhow::Result<Self> {
        ensure!(fd > 2, "run context needs a descriptor past stderr");
        // SAFETY: the launcher gives this descriptor to this child alone.
        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        let mut bytes = Vec::new();
        file.take(MOST + 1)
            .read_to_end(&mut bytes)
            .context("reading run context")?;
        ensure!(bytes.len() as u64 <= MOST, "run context exceeds 1 MiB");
        serde_json::from_slice(&bytes).context("reading the typed run context")
    }
    /// Prepare a private pipe on `fd`; call `send` after the child starts.
    pub fn hand_to(
        &self,
        command: &mut std::process::Command,
        fd: i32,
    ) -> anyhow::Result<RunHandoff> {
        use std::os::unix::process::CommandExt;
        ensure!(fd > 2, "run context needs a descriptor past stderr");
        if let Some(named) = &self.named {
            named.check()?;
        }
        let bytes = serde_json::to_vec(self).context("writing run context")?;
        ensure!(bytes.len() as u64 <= MOST, "run context exceeds 1 MiB");
        let (reader, writer) = std::io::pipe().context("a pipe for run context")?;
        let source = reader.as_raw_fd();
        // SAFETY: only async-signal-safe descriptor operations before exec.
        unsafe {
            command.pre_exec(move || {
                let result = if source == fd {
                    libc::fcntl(fd, libc::F_SETFD, 0)
                } else {
                    libc::dup2(source, fd)
                };
                if result < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Ok(RunHandoff {
            reader,
            writer,
            bytes,
        })
    }
}
impl RunHandoff {
    /// The child is reading now. Close our read end before writing, so an
    /// early child exit yields a broken pipe rather than retaining a reader.
    pub fn send(self) -> anyhow::Result<()> {
        let Self {
            reader,
            mut writer,
            bytes,
        } = self;
        drop(reader);
        writer
            .write_all(&bytes)
            .context("handing run context to its child")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_child_reads_and_closes_its_private_context() {
        if std::env::var_os("LNK_TEST_RUN_HANDOFF").is_none() {
            return;
        }
        let context = RunWith::read_fd(4).unwrap();
        assert_eq!(context.env["MARK"], "context-only");
        // SAFETY: only probes whether the consumed descriptor is closed.
        assert_eq!(unsafe { libc::fcntl(4, libc::F_GETFD) }, -1);
    }

    #[test]
    fn a_context_crosses_only_the_selected_childs_pipe() {
        let home = link_test_kit::TempDir::new("run-handoff-child");
        let mut command = link_test_kit::env::command(std::env::current_exe().unwrap(), &home);
        command
            .args([
                "--exact",
                "handoff::tests::the_child_reads_and_closes_its_private_context",
            ])
            .env("LNK_TEST_RUN_HANDOFF", "1");
        let with = RunWith {
            env: [("MARK".into(), "context-only".into())].into(),
            ..Default::default()
        };
        let handoff = with.hand_to(&mut command, 4).unwrap();
        let mut child = command.spawn().unwrap();
        handoff.send().unwrap();
        assert!(child.wait().unwrap().success());
    }

    #[test]
    fn an_early_child_exit_closes_the_pipe_instead_of_retaining_a_reader() {
        let home = link_test_kit::TempDir::new("run-handoff-exit");
        // Other libtest threads may fork while a pipe is open. Their children
        // briefly inherit its reader until exec, even with close-on-exec set.
        // Create this pipe inside a process whose only child is its recipient.
        let output = link_test_kit::env::command(std::env::current_exe().unwrap(), &home)
            .args([
                "--exact",
                "handoff::tests::the_exited_child_cannot_read_its_private_context",
            ])
            .env("LNK_TEST_EXITED_HANDOFF", "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }

    #[test]
    fn the_exited_child_cannot_read_its_private_context() {
        if std::env::var_os("LNK_TEST_EXITED_HANDOFF").is_none() {
            return;
        }
        let home = link_test_kit::TempDir::new("run-handoff-exited-child");
        let mut command = link_test_kit::env::command("/bin/true", &home);
        let handoff = RunWith::default().hand_to(&mut command, 4).unwrap();
        assert!(command.spawn().unwrap().wait().unwrap().success());
        let error = handoff.send().unwrap_err();
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::BrokenPipe,
        );
    }
}
