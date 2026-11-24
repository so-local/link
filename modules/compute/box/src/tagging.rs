//! What a box is tagged with: links checked, tagged resources listed,
//! one deleted by its id.

use super::*;
use link_box::said::{Asks, usage};
use link_format::{Question, Refuser, Table};

/// A tag, `KEY=VALUE`.
fn split(text: &str) -> anyhow::Result<(&str, &str)> {
    text.split_once('=').ok_or_else(|| {
        usage(
            sentence!("Couldn't read the tag {}.", Value::from(text.to_string())),
            prose!("A tag is KEY=VALUE."),
            cmd!("lnk box list --tag KEY=VALUE"),
            "list boxes by a tag",
        )
    })
}

/// Link-owned tags are a separate internal interface from a person's tags.
pub(super) fn link_tag(text: &str) -> anyhow::Result<(String, String)> {
    let (key, value) = split(text)?;
    if !key.starts_with("lnk-")
        || matches!(
            key,
            "lnk-box" | "lnk-machine" | "lnk-owner" | "lnk-deadline"
        )
    {
        return Err(usage(
            sentence!("Couldn't use the tag {}.", Value::from(key.to_string())),
            prose!("A Link tag starts with lnk-, and the tags of the box itself are reserved."),
            cmd!("lnk box spec show"),
            "see what a spec holds",
        ));
    }
    for word in [key, value] {
        if word.is_empty()
            || word.len() > 63
            || !word
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
        {
            return Err(usage(
                sentence!("Couldn't use the tag {}.", Value::from(text.to_string())),
                prose!("A Link tag uses lowercase letters, digits, underscores and dashes."),
                cmd!("lnk box spec show"),
                "see what a spec holds",
            ));
        }
    }
    Ok((key.to_string(), value.to_string()))
}

/// `lnk box check`: the attachments of the spec `path` and `fields`
/// make, looked up in account `name`, nothing made.
pub(super) fn check_attachments_on(
    out: &mut Out,
    path: Option<&std::path::Path>,
    fields: &link_spec::FieldFlags,
    name: &str,
    zone: Option<&str>,
) -> anyhow::Result<Done> {
    let account = account(name)?;
    let kind = account.kind.clone();
    let spec = BoxSpec::made(path, fields, zone.map(|zone| (kind.as_str(), zone)))?.spec;
    let place = spec.want(&account.kind).region.map(|p| p.to_string());
    if !spec.attach().is_empty() {
        let checked = cloud::call(
            &account.kind.clone(),
            "defaults",
            &Request {
                account,
                place,
                // A request for its attachments alone: every other part
                // of a machine its declared default.
                spec: Some(serde_json::from_value(serde_json::json!({
                    "name": "",
                    "size": "",
                    "user_data": "",
                    "attach": spec.attach(),
                }))?),
                ..Default::default()
            },
        )?;
        if checked.attachments != Some(true) {
            return Err(said(
                Report::error(sentence!(
                    "Couldn't check the attachments, since this cloud's adapter does not check them."
                ))
                .next(cmd!("lnk upgrade"), "update Link's own adapters"),
            ));
        }
    }
    out.reply(&link_actions::answer::output::<link_box::cli::Checked>(
        &serde_json::json!({"kind": "attachments", "attachments": "present"}),
    )?);
    Ok(Done::success(sentence!(
        "The attachments the spec names are present in {}.",
        Name::new("account", name)
    )))
}

pub(super) fn list_tagged(
    out: &mut Out,
    tags: &[String],
    on: Option<&str>,
) -> anyhow::Result<Done> {
    let tags = tags
        .iter()
        .map(|s| {
            let (k, v) = split(s)?;
            Ok((k.to_string(), v.to_string()))
        })
        .collect::<anyhow::Result<std::collections::BTreeMap<_, _>>>()?;
    let names = on.map(|n| vec![n.to_string()]).unwrap_or_else(accounts);
    let mut listed = Vec::new();
    for name in names {
        let account = account(&name)?;
        let kind = account.kind.clone();
        let found = cloud::call(
            &kind,
            "list",
            &Request {
                account,
                ..Default::default()
            },
        )?;
        for f in found
            .boxes
            .unwrap_or_default()
            .into_iter()
            .filter(|f| f.state != "gone" && tags.iter().all(|(k, v)| f.tags.get(k) == Some(v)))
        {
            listed.push(link_box_contract::Listed {
                tags: f.tags,
                name: f.name,
                account: name.clone(),
                kind: kind.clone(),
                place: f.place,
                id: f.id,
                machine_id: f.machine_id,
                size: f.size,
                ip: f.ip,
                state: f.state,
                deadline: f.deadline,
                found: "cloud".into(),
                ..Default::default()
            });
        }
    }
    out.reply(&link_actions::answer::output::<link_box::cli::Boxes>(
        &listed,
    )?);
    let mut table = Table::new(
        ["Account", "Place", "Id", "Name"],
        sentence!("No boxes have those tags."),
    );
    for b in &listed {
        table.row([
            Value::from(&b.account),
            Value::from(&b.place),
            Value::from(&b.id),
            Name::new("box", &b.name).into(),
        ]);
    }
    out.block(table);
    Ok(Done::shown())
}

pub(super) fn delete_id(
    out: &mut Out,
    id: &str,
    cloud_name: &str,
    place: &str,
    yes: bool,
) -> anyhow::Result<Done> {
    for value in [id, place] {
        if value.is_empty()
            || value.starts_with('-')
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        {
            return Err(usage(
                sentence!(
                    "Couldn't use {} as a cloud ID or place.",
                    Value::from(value.to_string())
                ),
                prose!("An ID or place is letters, digits, dots, underscores and dashes."),
                cmd!("lnk box list --tag KEY=VALUE"),
                "list boxes by a tag, with their IDs",
            ));
        }
    }
    if !out.asks(
        Question::confirm(
            sentence!(
                "Delete the machine {} in {}?",
                Value::from(id.to_string()),
                Value::from(format!("{cloud_name}/{place}"))
            ),
            false,
        )
        .yes(yes),
    )? {
        return Err(said(Report::refusal(
            Refuser::Person,
            sentence!("Didn't delete {}.", Value::from(id.to_string())),
        )));
    }
    let account = account(cloud_name)?;
    cloud::call(
        &account.kind.clone(),
        "remove",
        &Request {
            account,
            place: Some(place.to_string()),
            id: Some(id.to_string()),
            ..Default::default()
        },
    )?;
    Boxes::update(|boxes| {
        boxes
            .boxes
            .retain(|b| !(b.account == cloud_name && b.id == id));
    })?;
    Ok(Done::success(sentence!(
        "Deleted the machine {}.",
        Value::from(id.to_string())
    ))
    .next(cmd!("lnk box list --tag KEY=VALUE"), "list boxes by a tag"))
}
