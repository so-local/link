//! Owner-only CLI forms, including delivery-local draft editing and explicit atomic submission.
use anyhow::{Context, ensure};
use link_agent::{cli::QuestionListArgs as Questions, settings::Settings};
use link_format::{Done, Out, Report, Text, Verbatim, prose};
use link_interactions::{forms, *};
use std::io::Read;

pub(crate) fn run(out: &mut Out, named: Option<&str>, args: &Questions) -> Result<Done, Report> {
    let agent = Settings::resolve_without_keys(named)?;
    let store = Store::machine()?;
    let route = format!("cli:{}", agent.name);
    let ingress = Ingress {
        actor: sources::owner(),
        route: route.clone(),
        surface: "cli".into(),
        input_kind: InputKind::Typed,
    };
    let Some(code) = &args.code else {
        for snapshot in store.pending(now())?.into_iter().filter(|s| {
            s.question
                .requester
                .routing_agent
                .as_ref()
                .is_none_or(|a| *a == agent.name)
        }) {
            let e = store.plan_delivery(
                &snapshot.question.id,
                &route,
                "cli",
                &render::text_capabilities(),
                None,
                now(),
            )?;
            store.delivered(&e.delivery.id, None)?;
            if args.json {
                link_actions::answer::line::<link_agent_spec::declared::LifeQuestionDelivery>(
                    out, &e,
                )?
            } else {
                show(out, &render::delivery(&e));
            }
        }
        return Ok(Done::shown());
    };
    let result = (|| -> anyhow::Result<forms::Form> {
        let id = store
            .find_code(code)?
            .context("no question has that response code")?;
        let deliveries = store.deliveries(&id)?;
        let delivery = if let Some(delivery) = deliveries
            .into_iter()
            .find(|d| d.route == route && d.surface == "cli")
        {
            delivery
        } else {
            store
                .plan_delivery(
                    &id,
                    &route,
                    "cli",
                    &render::text_capabilities(),
                    None,
                    now(),
                )?
                .delivery
        };
        let form = store.form(&ingress, &delivery.id, now())?;
        let action = if let Some(text) = &args.reply {
            Some(forms::Action::Respond {
                value: reply::parse(
                    &form.envelope.question,
                    text,
                    false,
                    false,
                    text == "/dismiss",
                )?,
            })
        } else if let Some(file) = &args.draft {
            let mut bytes = Vec::new();
            std::fs::File::open(file)?
                .take(link_spec::MAX_BYTES + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() as u64 <= link_spec::MAX_BYTES,
                "draft file is too large"
            );
            Some(forms::Action::Edit {
                expected_revision: form.draft.revision,
                value: serde_json::from_slice(&bytes)?,
            })
        } else {
            args.submit
                .map(u64::try_from)
                .transpose()
                .context("--submit is a draft's revision, a whole number")?
                .map(|draft_revision| forms::Action::Submit { draft_revision })
        };
        if let Some(action) = action {
            store.form_control(
                &ingress,
                forms::Control {
                    delivery: delivery.id.clone(),
                    question_revision: form.envelope.question.revision,
                    anchor: delivery.anchor,
                    transport_event: TransportEventId::new(link_files::random_token()?)
                        .map_err(anyhow::Error::msg)?,
                    action,
                },
                now(),
            )?;
        }
        store.form(&ingress, &delivery.id, now())
    })()?;
    if args.json {
        out.reply_line(&result)
    } else {
        show(out, &render::delivery(&result.envelope));
        if result.resolution.is_none() {
            out.block(Text(prose!(
                "Draft revision {}. Saving a draft leaves the question open until you submit it.",
                result.draft.revision
            )));
        }
        if let Some(resolution) = result.resolution {
            out.reply_line(&resolution);
        }
    }
    Ok(Done::shown())
}

fn show(out: &mut Out, text: &str) {
    out.block(
        text.lines()
            .fold(Verbatim::new(), |block, line| block.line(line)),
    );
}
