//! Recognize actual skill reads only at a registered read boundary.

use super::*;
use link_files::reading::{ContentDigest, ReadReceipt};
use link_harness_wire::evidence::{
    CallRef, ReadAuthority, ReadProvider, SkillAnchor, SkillLoad, SkillRoute, SkillSource,
};

pub(crate) fn loaded(
    answer: &Answered,
    call: CallRef,
    model_text: &str,
) -> anyhow::Result<Option<SkillLoad>> {
    let (Observation::Reply(result), Some(authority)) =
        (&answer.observation, &answer.read_authority)
    else {
        return Ok(None);
    };
    let document = &result["structuredContent"];
    let Some(value) = document.get("read") else {
        return Ok(None);
    };
    let receipt: ReadReceipt = serde_json::from_value(value.clone())?;
    let provider = match authority {
        ReadAuthority::Managed { source } => Some(source.provider),
        _ => None,
    };
    let Some(source) = skill_source(&receipt.path, provider)? else {
        return Ok(None);
    };
    // What the model was given, as it reads it plainly: the read's text
    // is there alone, inside its fence, never in the structured content.
    let displayed = result["content"]
        .as_array()
        .filter(|blocks| blocks.len() == 1)
        .and_then(|blocks| blocks.first())
        .and_then(|block| block["text"].as_str())
        .map(link_untrusted::unfenced)
        .ok_or_else(|| anyhow::anyhow!("a skill read has no returned text"))?;
    // File gives the text itself; memory the document as JSON, its text
    // in `text`, the rest as the structured content has it.
    let mut presented = document.clone();
    if let Some(object) = presented.as_object_mut() {
        object.remove("read");
    }
    let memory = serde_json::from_str::<Value>(&displayed)
        .ok()
        .filter(|shown| shown.get("read").is_none() && within(&presented, shown));
    let memory_text = memory.as_ref().and_then(|shown| shown["text"].as_str());
    let candidates: &[(Option<&str>, SkillRoute)] = &[
        (Some(&displayed), SkillRoute::File),
        (memory_text, SkillRoute::Memory),
    ];
    let route = candidates
        .iter()
        .filter(|(_, route)| match provider {
            Some(ReadProvider::File) => *route == SkillRoute::File,
            Some(ReadProvider::Memory) => *route == SkillRoute::Memory,
            None => true,
        })
        .find_map(|(text, route)| {
            text.filter(|text| receipt.validate(text).is_ok())
                .map(|_| *route)
        })
        .ok_or_else(|| anyhow::anyhow!("a skill read's receipt doesn't match its returned text"))?;
    let mut omissions = Vec::new();
    if answer.cut_text_bytes > 0 {
        omissions.push("model-facing skill result was clipped".into());
    }
    if !receipt.covers_source() {
        omissions.push("skill source was only partially or lossily read".into());
    }
    if !matches!(authority, ReadAuthority::Managed { .. }) {
        omissions.push("read source is service-asserted".into());
    }
    let supplied = (answer.cut_text_bytes == 0).then(|| receipt.text.clone());
    let load = SkillLoad {
        anchor: SkillAnchor::Call { call },
        source,
        route,
        authority: authority.clone(),
        read: receipt,
        supplied,
        model_view: ContentDigest::of(model_text.as_bytes()),
        complete: omissions.is_empty(),
        omissions,
        original_input: None,
    };
    load.validate()?;
    Ok(Some(load))
}

/// Whether every field of `presented` is in `shown` as it is.
fn within(presented: &Value, shown: &Value) -> bool {
    match (presented.as_object(), shown.as_object()) {
        (Some(presented), Some(shown)) => presented
            .iter()
            .all(|(key, value)| shown.get(key) == Some(value)),
        _ => false,
    }
}

fn skill_source(path: &str, provider: Option<ReadProvider>) -> anyhow::Result<Option<SkillSource>> {
    let under = |root: &str| {
        path.strip_prefix(root)
            .and_then(|path| path.strip_prefix('/'))
            .and_then(|path| path.strip_suffix(link_harness_wire::skill::FILE))
            .and_then(|path| path.strip_suffix('/'))
    };
    if let Some(name) = under(link_harness_contract::layout::SKILLS) {
        link_harness_wire::skill::check_name(name)?;
        return Ok(Some(SkillSource::Owned { path: path.into() }));
    }
    if provider != Some(ReadProvider::Memory) {
        for root in crate::repository::SKILLS {
            if let Some(name) = under(root) {
                link_harness_wire::skill::check_name(name)?;
                return Ok(Some(SkillSource::Repository { path: path.into() }));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_files::reading::{ByteSpan, ReadSnapshot};
    use link_harness_wire::evidence::ManagedReadSource;

    /// A file read's answer as file gives it: its text fenced in
    /// `content`, and only the receipt beside it.
    fn result(text: &str) -> Value {
        let receipt = ReadReceipt {
            path: "skills/example/SKILL.md".into(),
            source: ReadSnapshot::Whole {
                content: ContentDigest::of(text.as_bytes()),
            },
            span: ByteSpan {
                start: 0,
                end: text.len() as u64,
            },
            text: ContentDigest::of(text.as_bytes()),
            text_prefix_bytes: 0,
            transformations: vec![],
        };
        let source = link_untrusted::Source {
            kind: link_untrusted::Kind::File,
            name: "skills/example/SKILL.md",
        };
        let fenced = link_untrusted::fence(source, text).unwrap();
        json!({"content":[{"type":"text","text":fenced}],"structuredContent":{"path":"skills/example/SKILL.md","read":receipt}})
    }

    fn managed() -> ReadAuthority {
        ReadAuthority::Managed {
            source: ManagedReadSource {
                provider: ReadProvider::File,
                owner: "owner".into(),
                agent: "agent".into(),
                definition: "file".into(),
                argv_sha256: link_digest::sha256(b"argv"),
            },
        }
    }

    #[test]
    fn whole_reader_receipt_is_not_whole_model_delivery_after_clipping() {
        let source = result("a complete skill body");
        let mut answer = answered(&source, 3);
        answer.read_authority = Some(managed());
        let load = loaded(
            &answer,
            CallRef {
                proposal: 2,
                ordinal: 0,
            },
            &answer.text,
        )
        .unwrap()
        .unwrap();
        assert!(!load.complete);
        assert!(load.supplied.is_none());
        let mut answer = answered(&source, 4096);
        answer.read_authority = Some(managed());
        assert!(
            loaded(
                &answer,
                CallRef {
                    proposal: 2,
                    ordinal: 0
                },
                &answer.text
            )
            .unwrap()
            .unwrap()
            .complete
        );
    }

    #[test]
    fn changed_returned_text_and_unregistered_sources_never_get_full_credit() {
        let mut source = result("skill body");
        source["content"][0]["text"] = "changed".into();
        let mut answer = answered(&source, 4096);
        answer.read_authority = Some(managed());
        assert!(
            loaded(
                &answer,
                CallRef {
                    proposal: 2,
                    ordinal: 0
                },
                &answer.text
            )
            .is_err()
        );
        let mut answer = answered(&result("skill body"), 4096);
        answer.read_authority = Some(ReadAuthority::ServiceAsserted {
            server: "file".into(),
            operation: "read".into(),
        });
        assert!(
            !loaded(
                &answer,
                CallRef {
                    proposal: 2,
                    ordinal: 0
                },
                &answer.text
            )
            .unwrap()
            .unwrap()
            .complete
        );
    }

    /// The receipt is checked against what the model was given, unfenced:
    /// memory's document JSON, its text in `text`, with nothing of the
    /// text in the structured content; a document that says otherwise
    /// than the structured content isn't credited.
    #[test]
    fn a_receipt_is_checked_against_the_model_facing_text() {
        let file = result("skill body");
        assert!(file["structuredContent"].get("text").is_none());
        let call = CallRef {
            proposal: 2,
            ordinal: 0,
        };
        let mut answer = answered(&file, 4096);
        answer.read_authority = Some(managed());
        assert_eq!(
            loaded(&answer, call, &answer.text).unwrap().unwrap().route,
            SkillRoute::File
        );
        let mut memory = result("skill body");
        let receipt = memory["structuredContent"]["read"].clone();
        let shown = json!({"path": "skills/example/SKILL.md", "text": "skill body"});
        memory["content"][0]["text"] = shown.to_string().into();
        memory["structuredContent"] = json!({"path": "skills/example/SKILL.md", "read": receipt});
        let mut answer = answered(&memory, 4096);
        let ReadAuthority::Managed { mut source } = managed() else {
            unreachable!()
        };
        source.provider = ReadProvider::Memory;
        answer.read_authority = Some(ReadAuthority::Managed { source });
        let load = loaded(&answer, call, &answer.text).unwrap().unwrap();
        assert_eq!(load.route, SkillRoute::Memory);
        assert!(load.complete, "{:?}", load.omissions);
        // The document names another path than the structured content.
        memory["content"][0]["text"] = json!({"path": "elsewhere.md", "text": "skill body"})
            .to_string()
            .into();
        let mut answer = answered(&memory, 4096);
        answer.read_authority = Some(ReadAuthority::ServiceAsserted {
            server: "memory".into(),
            operation: "read".into(),
        });
        assert!(loaded(&answer, call, &answer.text).is_err());
    }

    #[test]
    fn ordinary_file_reads_cover_both_repository_skill_roots_without_a_second_catalogue() {
        for root in crate::repository::SKILLS {
            let path = format!("{root}/example/{}", link_harness_wire::skill::FILE);
            let mut source = result("repository skill body");
            source["structuredContent"]["read"]["path"] = path.clone().into();
            let mut answer = answered(&source, 4096);
            answer.read_authority = Some(managed());
            let load = loaded(
                &answer,
                CallRef {
                    proposal: 2,
                    ordinal: 0,
                },
                &answer.text,
            )
            .unwrap()
            .unwrap();
            assert_eq!(load.source, SkillSource::Repository { path: path.clone() });
            assert!(load.complete);
            assert!(
                skill_source(&path, Some(ReadProvider::Memory))
                    .unwrap()
                    .is_none()
            );
        }
    }
}
