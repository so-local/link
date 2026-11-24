//! JSON examples read by their owning wire types, never a second field list.

use std::path::Path;

use link_docs_check::{Finding, Tree, fail, site};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

mod catalog;
mod unique;

/// A JSON example is associated with one registered contract in a hidden
/// `<!-- json: fully::qualified::Type -->` comment immediately before its fence.
pub fn check(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    if !site::is_page(tree, file) {
        return Vec::new();
    }
    examples(text, |id, body| match id.strip_prefix("website:") {
        Some(list) => catalog::check(&apps(tree), list, body),
        None => validate(id, body),
    })
    .into_iter()
    .map(|(line, why)| fail(format!("{}:{line}: {why}", tree.shown(file))))
    .collect()
}

/// The apps of the workspace's websites, where the lists' schemas are.
fn apps(tree: &Tree) -> Vec<std::path::PathBuf> {
    tree.websites.iter().map(|web| web.app.clone()).collect()
}

fn fence(line: &str) -> Option<(char, usize, &str)> {
    let line = line.trim_start();
    let ch = line.chars().next()?;
    if !matches!(ch, '`' | '~') {
        return None;
    }
    let size = line.chars().take_while(|c| *c == ch).count();
    (size >= 3).then(|| (ch, size, line[size..].trim()))
}

fn examples(
    text: &str,
    mut validate: impl FnMut(&str, &str) -> Result<(), String>,
) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut binding = None;
    let mut lines = text.lines().enumerate();
    while let Some((n, line)) = lines.next() {
        let trimmed = line.trim();
        if let Some(marker) = trimmed.strip_prefix("<!-- json:") {
            if binding.is_some() {
                found.push((n + 1, "duplicate JSON contract association".into()));
            }
            binding = marker
                .strip_suffix("-->")
                .map(str::trim)
                .filter(|id| !id.is_empty());
            if binding.is_none() {
                found.push((n + 1, "malformed JSON contract association".into()));
            }
            continue;
        }
        let Some((ch, size, info)) = fence(line) else {
            if !trimmed.is_empty() && binding.take().is_some() {
                found.push((
                    n + 1,
                    "JSON association must immediately precede its fence".into(),
                ));
            }
            continue;
        };
        let json = info.split_whitespace().next() == Some("json");
        let associated = binding.take();
        let mut body = String::new();
        let mut closed = false;
        for (_, next) in lines.by_ref() {
            if fence(next)
                .is_some_and(|(end, length, info)| end == ch && length >= size && info.is_empty())
            {
                closed = true;
                break;
            }
            body.push_str(next);
            body.push('\n');
        }
        if !json {
            if associated.is_some() {
                found.push((n + 1, "JSON association precedes a non-JSON fence".into()));
            }
            continue;
        }
        let result = if !closed {
            Err("unclosed JSON fence".into())
        } else if let Some(id) = associated {
            validate(id, &body)
        } else {
            Err("JSON example has no owning contract; add <!-- json: registered::Type -->".into())
        };
        if let Err(why) = result {
            found.push((n + 1, why));
        }
    }
    if binding.is_some() {
        found.push((text.lines().count(), "JSON association has no fence".into()));
    }
    found
}

/// Serde's owner checks required fields and values. Its ignored-field
/// callback closes types that deliberately accept future wire extensions.
fn read<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let mut ignored = Vec::new();
    let parsed = serde_ignored::deserialize(&mut deserializer, |path| {
        ignored.push(path.to_string());
    })
    .map_err(|err| err.to_string())?;
    deserializer.end().map_err(|err| err.to_string())?;
    if ignored.is_empty() {
        Ok(parsed)
    } else {
        Err(format!("unknown JSON fields: {}", ignored.join(", ")))
    }
}

/// ACP tolerates malformed optional fields and drops them. Published
/// examples must retain every supplied value when the SDK writes them back.
fn retained(before: &Value, after: &Value, path: &str) -> Result<(), String> {
    match (before, after) {
        (Value::Object(before), Value::Object(after)) => {
            for (key, value) in before {
                let path = format!("{path}/{key}");
                let written = after
                    .get(key)
                    .ok_or_else(|| format!("{path} is discarded by its owning type"))?;
                retained(value, written, &path)?;
            }
            Ok(())
        }
        (Value::Array(before), Value::Array(after)) if before.len() == after.len() => {
            for (n, (value, written)) in before.iter().zip(after).enumerate() {
                retained(value, written, &format!("{path}/{n}"))?;
            }
            Ok(())
        }
        _ if before == after => Ok(()),
        _ => Err(format!("{path} changes when its owning type writes it")),
    }
}

fn acp<T: DeserializeOwned + Serialize>(text: &str) -> Result<T, String> {
    let parsed = read::<T>(text)?;
    let before = serde_json::from_str(text).map_err(|err| err.to_string())?;
    let written = serde_json::to_value(&parsed).map_err(|err| err.to_string())?;
    retained(&before, &written, "")?;
    Ok(parsed)
}

fn prompt(text: &str) -> Result<(), String> {
    use link_agent_state_protocol::acp::schema::v1::PromptRequest;
    use link_connection_contract::acp::Said;

    let prompt: PromptRequest = acp(text)?;
    Said::of(prompt.meta.as_ref()).map_err(|err| err.to_string())?;
    if let Some(asp) = prompt
        .meta
        .as_ref()
        .and_then(|meta| meta.get(link_agent_state_protocol::META))
    {
        let busy = link_agent_state_protocol::steering::asked(prompt.meta.as_ref())?
            .ok_or("_meta.asp does not contain a steering request")?;
        let written = link_agent_state_protocol::steering::asking(busy, Default::default());
        retained(asp, &written[link_agent_state_protocol::META], "/_meta/asp")?;
    }
    Ok(())
}

fn initialize(text: &str) -> Result<(), String> {
    use link_agent_state_protocol::acp::schema::v1::InitializeResponse;

    let response: InitializeResponse = acp(text)?;
    if let Some(asp) = response
        .agent_capabilities
        .meta
        .as_ref()
        .and_then(|meta| meta.get(link_agent_state_protocol::META))
    {
        read::<link_agent_state_protocol::Advertised>(&asp.to_string())?;
    }
    Ok(())
}

fn validate(id: &str, text: &str) -> Result<(), String> {
    unique::check(text)?;
    macro_rules! typed {
        ($($ty:path),+ $(,)?) => {
            match id {
                $(stringify!($ty) => read::<$ty>(text).map(|_| ()),)+
                "link_agent_state_protocol::acp::schema::v1::PromptRequest" => prompt(text),
                "link_agent_state_protocol::acp::schema::v1::InitializeResponse" => initialize(text),
                "link_test_timing::PhaseMeasurements" => {
                    read::<link_test_timing::PhaseMeasurements>(text)?;
                    link_test_timing::PhaseMeasurements::parse(text).map(|_| ())
                },
                _ => Err(format!("unregistered JSON contract {id:?}")),
            }
        };
    }
    typed!(
        link_api::types::StartDevice,
        link_model_contract::api::ContextOf,
        link_api::types::DeviceLogin,
        link_api::types::Account,
        link_api::types::Installations,
        link_api::types::GrantInstallation,
        link_api::types::TokenRequest,
        link_api::types::AccessToken,
        link_api::types::SigningKeys,
        link_api::types::User,
        link_api::types::Report,
        link_api::types::Error,
        link_core::uninstall::Kept,
        link_archive_contract::Thing,
        link_format::log::Line,
        link_agent_state_protocol::methods::Imported,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROMPT: &str = r#"{"sessionId":"chat:42","prompt":[{"type":"text","text":"Hello"}],"_meta":{"asp":{"busy":"interrupt"}}}"#;

    #[test]
    fn published_examples_use_their_registered_shipping_contracts() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        let tree = Tree::read(
            &here.join("../../.."),
            true,
            &here.join("../large-files.toml"),
        );
        let rule = link_docs_check::Rule {
            id: "json",
            scope: link_docs_check::Scope::Site,
            fix: "associate the example with its owning wire type or released schema",
            check: link_docs_check::Check::Each(check),
        };
        link_docs_check::run(&tree, &[rule]).assert();
    }

    #[test]
    fn owner_rejects_changed_field_type_missing_field_and_unknown_field() {
        let valid =
            r#"{"name":"laptop","public_key":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}"#;
        assert!(read::<link_api::types::GrantInstallation>(valid).is_ok());
        for invalid in [
            valid.replace("\"name\"", "\"nane\""),
            valid.replace("\"laptop\"", "42"),
            valid.replace("\"name\":\"laptop\",", ""),
            valid.replace("\"laptop\"", "\"laptop\",\"unknown\":true"),
            valid.replace("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "not a key"),
        ] {
            assert!(
                read::<link_api::types::GrantInstallation>(&invalid).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn extensible_wire_types_and_their_metadata_still_reject_doc_typos() {
        assert!(prompt(PROMPT).is_ok());
        for invalid in [
            PROMPT.replace("sessionId", "sesionId"),
            PROMPT.replace("interrupt", "teleport"),
            PROMPT.replace("\"busy\"", "\"buxy\""),
            PROMPT.replace("\"busy\":", "\"extra\":true,\"busy\":"),
            PROMPT.replace("\"text\":\"Hello\"", "\"text\":42"),
            PROMPT.replace("\"text\":\"Hello\"", "\"text\":\"Hello\",\"typo\":true"),
            PROMPT.replace("\"asp\":{", "\"link\":{\"from\":\"nobody\"},\"asp\":{"),
        ] {
            assert!(prompt(&invalid).is_err(), "{invalid}");
        }
        assert!(
            read::<link_agent_state_protocol::methods::Imported>(r#"{"taken":[],"typo":1}"#)
                .is_err()
        );
    }

    #[test]
    fn every_json_fence_needs_one_known_binding_even_with_other_fences_around_it() {
        let valid = "<!-- json: link_api::types::Error -->\n\n```json\n{\"error\":\"invalid_token\"}\n```\n";
        assert!(examples(valid, validate).is_empty());
        assert!(examples(&valid.replace('`', "~"), validate).is_empty());
        assert_eq!(examples("```json\n{}\n```\n", validate).len(), 1);
        assert_eq!(
            examples(
                &valid.replace("link_api::types::Error", "unknown::Type"),
                validate
            )
            .len(),
            1
        );
        assert_eq!(
            examples(&valid.replace("\n\n", "\nUnrelated prose\n"), validate).len(),
            2
        );
        assert_eq!(
            examples("````text\n```json\n{}\n```\n````\n", validate).len(),
            0
        );
        assert_eq!(
            examples("<!-- json: link_api::types::Error -->", validate).len(),
            1
        );
        assert_eq!(
            examples("<!-- json: -->\n```json\n{}\n```", validate).len(),
            2
        );
    }

    #[test]
    fn timing_examples_use_the_owning_phase_validation() {
        let id = "link_test_timing::PhaseMeasurements";
        let valid = r#"{"schema":1,"phases":[{"name":"fmt","wall_seconds":1,"status":0}]}"#;
        assert!(validate(id, valid).is_ok());
        for invalid in [
            valid.replace("\"schema\":1", "\"schema\":2"),
            valid.replace("\"wall_seconds\":1", "\"wall_seconds\":-1"),
            valid.replace("\"wall_seconds\":1", "\"wall_second\":1"),
            valid.replace("\"name\":\"fmt\"", "\"name\":\" \""),
        ] {
            assert!(validate(id, &invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn typed_reader_checks_the_whole_json_document() {
        assert!(read::<link_api::types::Error>(r#"{"error":"invalid_token"} {}"#).is_err());
        assert!(
            read::<link_api::types::Error>(r#"{"error":"invalid_token","error":"invalid_grant"}"#)
                .is_err()
        );
        assert!(initialize(r#"{"agentCapabilities":{}}"#).is_err());
        assert!(
            initialize(r#"{"protocolVersion":1,"agentCapabilities":{"promptCapabilities":"bad"}}"#)
                .is_err()
        );
        let initialize = r#"{"protocolVersion":1,"agentCapabilities":{"_meta":{"asp":{"agentState":{"export":["sessions"],"import":["instructions"]}}}}}"#;
        assert!(super::initialize(initialize).is_ok());
        assert!(super::initialize(&initialize.replace("export", "exports")).is_err());
        assert!(super::initialize(&initialize.replace("sessions", "unknown-kind")).is_err());
    }
}
