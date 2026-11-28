//! Every call answered on a store ([`Store::answer`]), the same code for
//! your own files and the keeper's.

use super::*;

impl Store {
    /// `request`, answered on this store: by your own `lnk` on yours, or
    /// by the keeper on the store of the user calling it. `kept_by` says
    /// where a value it lists is kept.
    pub fn answer(&self, request: Request, kept_by: &str) -> anyhow::Result<Reply> {
        Ok(match request {
            Request::ProtectRoute { .. } | Request::UnbindRoute { .. } => {
                bail!("secret routes require the keeper")
            }
            Request::RemoveUser { .. } => bail!("only the keeper keeps other users' secrets"),
            Request::List => Reply::Listed {
                listed: self.load()?.list(),
            },
            Request::ForService { id, which } => Reply::ForService {
                given: self.load()?.for_service(&id, which)?,
            },
            Request::ForYou { ids } => Reply::ForYou {
                given: self.load()?.for_you(&ids)?,
            },
            Request::Add {
                name,
                value,
                sealed,
                to,
            } => Reply::Replaced {
                added: Box::new(add_in(self, &name, &value, sealed, to)?),
            },
            Request::Grant {
                called,
                add,
                remove,
            } => Reply::Granted {
                changed: grant_in(self, &called, add, remove)?,
            },
            Request::KeepOwn {
                id,
                workload,
                name,
                own,
            } => {
                let own = own.into_iter().map(|o| (o.r#as, o.value)).collect();
                keep_own_in(
                    self,
                    &super::super::Grantee::new(&workload, &name, &id)?,
                    own,
                )?;
                Reply::Done
            }
            Request::Forget { id } => {
                forget_in(self, &id)?;
                Reply::Done
            }
            Request::RetainOwn { id, uses } => {
                crate::machine::check_id(&id)?;
                let _locked = self.lock()?;
                let mut secrets = self.load()?;
                let changed = secrets.retain(&id, &uses)?;
                secrets.apply(self, changed)?;
                Reply::Done
            }
            Request::Copied { id, to, without } => {
                copied_in(self, &id, to, &without)?;
                Reply::Done
            }
            Request::GoneFrom { id, from, without } => {
                gone_from_in(self, &id, &from, &without)?;
                Reply::Done
            }
            Request::Remove { what } => Reply::Removing {
                removing: remove_here(self, &what)?,
            },
            Request::Reached {
                id,
                reached: copies,
            } => {
                reached(self, &id, &copies)?;
                Reply::Done
            }
            Request::Held => Reply::Held {
                held: held_in(self, kept_by),
            },
            Request::Clear => {
                let _locked = self.lock()?;
                match std::fs::remove_file(&self.file) {
                    Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                        return Err(err)
                            .with_context(|| format!("removing {}", self.file.display()));
                    }
                    _ => Reply::Done,
                }
            }
        })
    }
}
