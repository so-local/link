//! Deliberate directory permissions for owned permission-boundary fixtures.

use super::TempDir;
use std::os::unix::fs::PermissionsExt;

impl TempDir {
    /// Changes only this guard's folder, for a test of an unsafe or restricted
    /// ancestor. Programs use the kit's program helpers instead.
    pub fn set_mode(&self, mode: u32) -> std::io::Result<()> {
        std::fs::set_permissions(self.path(), std::fs::Permissions::from_mode(mode))
    }
}
