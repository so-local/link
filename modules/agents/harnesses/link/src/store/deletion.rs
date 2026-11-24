use super::*;
use link_interactions::control::DeleteScope;

impl Files {
    pub(super) fn delete_retained(
        &self,
        agent: &Agent,
        thread: &Thread,
        scope: &DeleteScope,
    ) -> anyhow::Result<()> {
        match scope {
            DeleteScope::TaskRecord => {
                anyhow::ensure!(
                    thread.name().starts_with("task-"),
                    "selected work is not a child task"
                );
                // The host's logical record is tombstoned only after this source acknowledgement.
                // Its transcript is retained unless Conversation is explicitly selected.
                Ok(())
            }
            DeleteScope::Conversation => {
                let folder = self.folder(agent, thread, false)?;
                if let Some(Way::Driver(name)) = self.kept(agent, folder.as_ref())? {
                    let (driver, whose) = self.driver(agent, &name)?;
                    driver.remove(&whose, thread.name())?;
                }
                if let Some(parent) = self.conversations(agent, false)? {
                    match parent.dir.remove_all(OsStr::new(&thread.folder())) {
                        Ok(()) => {}
                        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                        Err(err) => return Err(err.into()),
                    }
                }
                Ok(())
            }
            DeleteScope::Artifacts { ids } => {
                anyhow::ensure!(!ids.is_empty(), "no artifact IDs were selected");
                let known: std::collections::BTreeSet<_> = self
                    .read(agent, thread)?
                    .into_iter()
                    .flat_map(|e| e.parts.into_iter().map(|p| p.file))
                    .collect();
                for id in ids {
                    anyhow::ensure!(known.contains(id), "unknown artifact ID");
                    part_name(id)?;
                }
                if let Some(folder) = self.folder(agent, thread, false)?
                    && let Some(parts) = folder.folder(PARTS, false)?
                {
                    for id in ids {
                        match parts.remove(part_name(id)?) {
                            Ok(()) => {}
                            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                            Err(err) => return Err(err.into()),
                        }
                    }
                }
                Ok(())
            }
        }
    }
}
