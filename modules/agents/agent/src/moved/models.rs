//! Source model metadata while a host supplies its own models. Values stay
//! in the secret keeper, so removing a secret also revokes this retention.

use crate::settings::Settings;
use anyhow::ensure;
use link_agent_identity::own::OwnSecret;
use link_format::WithinExt;
use link_harness_contract::harness::Model;
use link_secret_contract::Which;
use link_secretive::Duplicate;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Models {
    name: String,
    /// The service the agent acts as, which its retained keys are kept
    /// for.
    service: String,
    pub model: Option<Model>,
    pub fallback: Option<Model>,
    model_key: bool,
    fallback_key: bool,
    #[serde(skip)]
    pub revoked: bool,
}

link_files::specification!(Models, include_str!("models.schema.toml"));

fn path(id: &str) -> anyhow::Result<PathBuf> {
    link_machine::machine::check_id(id)?;
    Ok(super::dir().join(format!("{id}.models")))
}

pub fn keep(settings: &Settings) -> anyhow::Result<()> {
    let id = settings.agent_id().within("the agent has no stable ID")?;
    let path = path(id)?;
    let service = settings.service_id()?.to_string();
    let mut models = Models {
        name: settings.name.clone(),
        service: service.clone(),
        model: settings.model.duplicate(),
        fallback: settings.fallback.duplicate(),
        model_key: settings
            .model
            .as_ref()
            .is_some_and(|m| !m.api_key.is_empty()),
        fallback_key: settings
            .fallback
            .as_ref()
            .is_some_and(|m| !m.api_key.is_empty()),
        revoked: false,
    };
    for model in [&mut models.model, &mut models.fallback]
        .into_iter()
        .flatten()
    {
        model.api_key = Default::default();
    }
    link_files::create_private_dir(&super::dir())?;
    link_files::write_private(&path, &link_files::saved::print_json(&models)?)?;
    link_secret_contract::retain_own(
        &service,
        vec![OwnSecret::Model.to_use()?, OwnSecret::Fallback.to_use()?],
    )
}

/// The models kept for agent `id`, read as saved: none when there are none.
fn saved(id: &str) -> anyhow::Result<Option<Models>> {
    let bytes = match std::fs::read(path(id)?) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    Ok(Some(link_files::saved::parse_json(&bytes).map_err(
        |_| anyhow::anyhow!("the source's saved model settings are invalid"),
    )?))
}

pub fn load(id: &str) -> anyhow::Result<Option<Models>> {
    let Some(mut models) = saved(id)? else {
        return Ok(None);
    };
    link_machine::machine::check_name(&models.name)?;
    link_machine::machine::check_id(&models.service)?;
    ensure!(
        [&models.model, &models.fallback]
            .into_iter()
            .flatten()
            .all(|m| m.api_key.is_empty()),
        "the source's saved model metadata contains a credential"
    );
    let own = [
        (OwnSecret::Model.to_use()?, models.model.is_some()),
        (OwnSecret::Fallback.to_use()?, models.fallback.is_some()),
    ]
    .into_iter()
    .filter(|(_, exists)| *exists)
    .map(|(r#as, _)| (r#as, None))
    .collect();
    let agent = link_secret_contract::Grantee::new(
        &format!("{}:{id}", crate::settings::ids::KIND),
        &models.name,
        &models.service,
    )?;
    link_secret_contract::keep_own(&agent, own)?;
    for given in link_secret_contract::for_service(&models.service, Which::Own)? {
        let target = match OwnSecret::of(&given.r#as) {
            Some(OwnSecret::Model) => &mut models.model,
            Some(OwnSecret::Fallback) => &mut models.fallback,
            _ => continue,
        };
        if let Some(model) = target {
            model.api_key = given.value;
        }
    }
    models.revoked = models.model_key && models.model.as_ref().is_none_or(|m| m.api_key.is_empty());
    if models.fallback_key
        && models
            .fallback
            .as_ref()
            .is_none_or(|m| m.api_key.is_empty())
    {
        models.fallback = None;
    }
    Ok(Some(models))
}

/// An ordinary transfer after a failed managed move cancels its retention.
pub fn release(id: &str) -> anyhow::Result<()> {
    if let Some(models) = saved(id)? {
        link_secret_contract::retain_own(&models.service, Vec::new())?;
        remove(id)?;
    }
    Ok(())
}

pub fn remove(id: &str) -> anyhow::Result<()> {
    match std::fs::remove_file(path(id)?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
