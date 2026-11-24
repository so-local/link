//! Validate the selected provider ID before saving settings or restarting.
use link_format::{Exit, Name, Report, Sentence, WithinExt, sentence};
use link_harness_contract::harness::Model;
use link_model_contract::{chat::client, selection};

/// An explicit selection needs no spec read. Bare provider selections use
/// the same snapshot supplied to first-run prompts and harness naming.
pub(super) fn name(
    model: &Model,
    spec: Option<&link_agent_spec::Resolved>,
) -> Result<Option<String>, Report> {
    if model.name.is_some() {
        return Ok(model.name.clone());
    }
    Ok(link_agent::settings::Settings::model_name(
        model,
        spec.within("the effective spec is needed to name an unpinned model")?,
    ))
}

pub(super) async fn selected(
    model: &Model,
    spec: Option<&link_agent_spec::Resolved>,
) -> Result<(), Report> {
    if model.is_local() {
        return Ok(());
    }
    let name = name(model, spec)?.unwrap_or_default();
    selection::name(&name).map_err(|why| Report::error(Sentence::adopt(&why)).exit(Exit::Usage))?;
    // Built-in provider IDs are already known to exist. Only IDs newer than
    // this catalogue need discovery; facts alone never prove account access.
    if link_model_contract::link().model.contains_key(&name) {
        return Ok(());
    }
    link_fetch::install_crypto_provider();
    let keyed = client::keyed().within("making the client that checks a model with its key")?;
    let offered = client::listed(
        &keyed,
        &client::Model {
            provider: model.provider.clone(),
            name: Some(name.clone()),
            base_url: model.base_url.clone(),
            api_key: model.api_key.duplicate(),
        },
    )
    .await
    .map_err(|why| {
        Report::error(sentence!(
            "Couldn't verify {} with its provider.",
            Name::new("model", &name)
        ))
        .why(Sentence::adopt(&format!("{why:#}")))
    })?;
    selection::offered(&name, &offered)
        .map_err(|why| Report::error(Sentence::adopt(&why)).exit(Exit::Usage))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selected_name_uses_the_effective_opinion_without_changing_the_saved_model() {
        let mut model = Model {
            provider: "anthropic".into(),
            ..Default::default()
        };
        let spec = link_agent_spec::resolve(
            Some(
                &link_agent_spec::parse(
                    "schema = 1\n[model.anthropic]\ndefault='claude-sonnet-5-5'\n",
                )
                .unwrap(),
            ),
            &[],
            &link_agent_spec::Known {
                name: "main".into(),
            },
        )
        .unwrap();
        assert_eq!(
            name(&model, Some(&spec)).unwrap().as_deref(),
            Some("claude-sonnet-5-5")
        );
        assert!(model.name.is_none());
        assert!(
            name(&model, None).is_err(),
            "never substitute a built-in for a missing effective snapshot"
        );
        model.name = Some("claude-opus-5-5".into());
        assert_eq!(
            name(&model, None).unwrap().as_deref(),
            Some("claude-opus-5-5")
        );
    }
}
