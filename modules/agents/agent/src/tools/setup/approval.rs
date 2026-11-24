use super::*;
use link_interactions::{approval::*, sources, *};

pub(super) struct GrantsApproval {
    store: Store,
    id: RequestId,
    delivery: delivery::DeliveryEnvelope,
    operation: BoundOperation,
}

impl GrantsApproval {
    pub fn new(
        agent: &Settings,
        tool: &str,
        expected: &Sandbox,
        replacement: &Sandbox,
    ) -> anyhow::Result<Self> {
        let store = Store::machine()?;
        let path = sandbox_file::path(&agent.name).display().to_string();
        let previous = expected.text(&agent.name)?;
        let next = replacement.text(&agent.name)?;
        let review = PermissionReview {capability: format!("Save the sandbox grants needed by tool {tool}"),
            targets: vec![path.clone()], lifetime: "Kept in the agent's sandbox spec until the owner changes it".into(),
            consequences: vec!["Later runs and harnesses use these grants. This does not widen an already running immutable sandbox.".into()],
            requester_reason: None, exact_operation: format!("Before:\n{previous}\nAfter:\n{next}")};
        let requester = sources::requester("owner", link_interactions::OWNER, &agent.name)?;
        let spec = agent.spec()?.spec;
        let wait = spec
            .connections
            .as_ref()
            .and_then(|c| c.ask_wait)
            .within("the agent spec has no owner wait")?
            .get();
        let operation = sources::operation(
            requester.source.clone(),
            next.as_bytes(),
            &review,
            Scope::File {
                target: path,
                expected_revision: link_digest::sha256(previous.as_bytes()),
            },
            None,
        )?;
        let id = store.create_bound(
            requester,
            sources::owner(),
            DomainId::new("configuration.change").map_err(anyhow::Error::msg)?,
            sources::confirmation(
                "Save sandbox configuration".into(),
                "Save these persistent grants?".into(),
                operation.id.to_string(),
                wait,
            ),
            Effect::Authorize(Box::new(operation.clone())),
            Some(review),
            now(),
        )?;
        let delivery = store.plan_delivery(
            &id,
            &format!("cli:{}", agent.name),
            "cli",
            &render::text_capabilities(),
            None,
            now(),
        )?;
        Ok(Self {
            store,
            id,
            delivery,
            operation,
        })
    }

    pub fn review(&self) -> String {
        render::delivery(&self.delivery)
    }

    pub fn decide(&self, approved: bool) -> anyhow::Result<()> {
        self.store.submit(
            &Ingress {
                actor: sources::owner(),
                route: self.delivery.delivery.route.clone(),
                surface: "cli".into(),
                input_kind: InputKind::Control,
            },
            ResponseCandidate {
                request: self.id.clone(),
                revision: self.delivery.question.revision,
                delivery: self.delivery.delivery.id.clone(),
                value: reply::parse(
                    &self.delivery.question,
                    if approved { "yes" } else { "no" },
                    false,
                    false,
                    false,
                )?,
                transport_event: TransportEventId::new(link_files::random_token()?)
                    .map_err(anyhow::Error::msg)?,
            },
            now(),
        )?;
        Ok(())
    }

    pub fn apply(
        self,
        agent: &Settings,
        expected: &Sandbox,
        replacement: &Sandbox,
    ) -> anyhow::Result<()> {
        let mut claim = None;
        let result = sandbox_file::save_if(&agent.name, expected, replacement, || {
            claim = Some(self.store.claim(
                &self.id,
                &SourceChecks {
                    operation: self.operation.clone(),
                    authority_current: true,
                    generation_current: true,
                    control_fence_clear: true,
                },
                now(),
            )?);
            Ok(())
        });
        if let Some(claim) = claim {
            self.store.report(
                &self.id,
                &self.operation.source,
                &claim,
                match &result {
                    Ok(()) => EffectOutcome::Applied {
                        detail: "Saved the exact reviewed persistent sandbox grants.".into(),
                    },
                    Err(err) => EffectOutcome::Unknown {
                        reason: format!(
                            "The sandbox spec's write did not confirm completion: {err:#}"
                        ),
                    },
                },
                now(),
            )?;
        }
        result
    }
}
