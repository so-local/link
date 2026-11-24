//! Native schedule commands, behind the adapter boundary.
use super::*;

impl Adapter {
    /// Read native definitions before changing harness. The adapter alone knows
    /// the file shape below the native home named by its traces.
    pub fn native_schedules(
        &self,
        root: &Path,
    ) -> anyhow::Result<link_harness_contract::asp::schedules::Schedules> {
        let output = link_secretive::process::command(&self.program)
            .args(["state", "schedules"])
            .arg(root)
            .env_clear()
            .stdin(Stdio::null())
            .output()?;
        ensure!(
            output.status.success(),
            "native schedule export failed: {}",
            link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
        );
        Ok(serde_json::from_slice(&output.stdout)?)
    }

    pub fn clear_schedules(
        &self,
        boxed: &Boxed,
        root: &Path,
        revision: &str,
        context: Option<&str>,
    ) -> anyhow::Result<()> {
        ensure!(
            root.starts_with(&boxed.dir),
            "only the managed copy's schedules can be cleared"
        );
        let root = root.to_str().within("native schedule root is not UTF-8")?;
        let mut arguments = vec!["state", "clear-schedules", revision, root];
        if let Some(context) = context {
            arguments.push(context);
        }
        self.of_files(boxed, &arguments, false, &[])?;
        Ok(())
    }
}
