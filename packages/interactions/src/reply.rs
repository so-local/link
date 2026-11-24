//! One targeting and answer grammar shared by terminal and chat adapters.
use crate::{constants::*, *};
use anyhow::{Context, ensure};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeReply<'a> {
    pub code: String,
    pub body: &'a str,
    pub deny: bool,
    pub dismiss: bool,
}

pub fn is_code(value: &str) -> bool {
    value.len() == CODE_LENGTH
        && value
            .to_ascii_lowercase()
            .bytes()
            .all(|b| CODE_ALPHABET.contains(&b))
}

/// Syntax recognition only. Possession of a code supplies no authentication.
pub fn code_reply(text: &str) -> Option<CodeReply<'_>> {
    let (first, rest) = split_word(text.trim_start());
    let (deny, dismiss, code, body) =
        if first.eq_ignore_ascii_case("no") || first.eq_ignore_ascii_case("cancel") {
            let (code, body) = split_word(rest);
            (
                first.eq_ignore_ascii_case("no"),
                first.eq_ignore_ascii_case("cancel"),
                code,
                body,
            )
        } else {
            (false, false, first, rest)
        };
    is_code(code).then(|| CodeReply {
        code: code.to_ascii_lowercase(),
        body,
        deny,
        dismiss,
    })
}

fn split_word(text: &str) -> (&str, &str) {
    text.find(char::is_whitespace).map_or((text, ""), |at| {
        let skip = text[at..].chars().next().expect("whitespace").len_utf8();
        (&text[..at], &text[at + skip..])
    })
}

pub fn parse(
    q: &QuestionSet,
    body: &str,
    code_alone: bool,
    deny: bool,
    dismiss: bool,
) -> anyhow::Result<ResponseValue> {
    let comment = |text: &str| (!text.trim().is_empty()).then(|| text.to_string());
    if dismiss {
        return Ok(ResponseValue::Dismiss {
            comment: comment(body),
        });
    }
    if deny || code_alone {
        ensure!(
            q.questions.len() == 1
                && matches!(q.questions[0].answer_shape, AnswerShape::Confirmation),
            "a code alone can only answer confirmation"
        );
        ensure!(
            body.trim().is_empty(),
            "unexpected text after confirmation code"
        );
        return Ok(single(
            &q.questions[0],
            ItemAnswer::Confirmation { yes: !deny },
        ));
    }
    if q.questions.len() == 1 {
        let item = &q.questions[0];
        match &item.answer_shape {
            AnswerShape::Text { .. } => {
                return Ok(single(item, ItemAnswer::Text { text: body.into() }));
            }
            AnswerShape::Confirmation
                if matches!(body.trim().to_ascii_lowercase().as_str(), "yes" | "no") =>
            {
                return Ok(single(
                    item,
                    ItemAnswer::Confirmation {
                        yes: body.trim().eq_ignore_ascii_case("yes"),
                    },
                ));
            }
            AnswerShape::Choice { .. } if body.trim().starts_with(|c: char| c.is_ascii_digit()) => {
                return Ok(single(item, choices(item, body.trim())?));
            }
            AnswerShape::Choice { .. } if body.starts_with("other ") => {
                return Ok(single(
                    item,
                    ItemAnswer::Choice {
                        selected: Vec::new(),
                        other_text: Some(body[6..].into()),
                        comment: None,
                    },
                ));
            }
            _ => {}
        }
    }
    if !body
        .lines()
        .next()
        .is_some_and(|line| line.starts_with('q') && line.contains(':'))
    {
        return Ok(ResponseValue::Submit(SetSubmission {
            answers: BTreeMap::new(),
            comment: comment(body),
        }));
    }
    structured(q, body)
}

fn single(item: &QuestionItem, answer: ItemAnswer) -> ResponseValue {
    ResponseValue::Submit(SetSubmission {
        answers: BTreeMap::from([(item.id.clone(), answer)]),
        comment: None,
    })
}

fn choices(item: &QuestionItem, text: &str) -> anyhow::Result<ItemAnswer> {
    let AnswerShape::Choice { options, .. } = &item.answer_shape else {
        anyhow::bail!("not a choice item");
    };
    let selected = text
        .split(',')
        .map(|n| {
            let n: usize = n.trim().parse().context("invalid option number")?;
            options
                .get(n.checked_sub(1).context("option numbers start at 1")?)
                .map(|o| o.id.clone())
                .context("unknown option number")
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(ItemAnswer::Choice {
        selected,
        other_text: None,
        comment: None,
    })
}

fn structured(q: &QuestionSet, body: &str) -> anyhow::Result<ResponseValue> {
    let mut submission = SetSubmission::default();
    let mut offset = 0;
    let mut used = std::collections::BTreeSet::new();
    for line in body.split_inclusive('\n') {
        let directive = line.trim_end_matches(['\n', '\r']);
        if directive == "--" {
            submission.comment = Some(body[offset + line.len()..].into());
            break;
        }
        let (label, value) = directive
            .split_once(':')
            .context("invalid item directive")?;
        let n: usize = label
            .strip_prefix('q')
            .context("expected q number")?
            .parse()?;
        let item = q
            .questions
            .get(n.checked_sub(1).context("item numbers start at 1")?)
            .context("unknown item number")?;
        let (kind, value) = split_word(value.trim_start());
        ensure!(
            used.insert((item.id.clone(), kind.to_string())),
            "duplicate item directive"
        );
        match kind {
            "choice" => {
                let answer = choices(item, value)?;
                if let Some(ItemAnswer::Choice { selected, .. }) =
                    submission.answers.get_mut(&item.id)
                {
                    let ItemAnswer::Choice {
                        selected: picked, ..
                    } = answer
                    else {
                        unreachable!()
                    };
                    *selected = picked;
                } else {
                    submission.answers.insert(item.id.clone(), answer);
                }
            }
            "other" | "note" => {
                ensure!(
                    matches!(item.answer_shape, AnswerShape::Choice { .. }),
                    "Other/note requires choice"
                );
                let answer =
                    submission
                        .answers
                        .entry(item.id.clone())
                        .or_insert(ItemAnswer::Choice {
                            selected: Vec::new(),
                            other_text: None,
                            comment: None,
                        });
                let ItemAnswer::Choice {
                    other_text,
                    comment,
                    ..
                } = answer
                else {
                    anyhow::bail!("conflicting answer directives");
                };
                if kind == "other" {
                    *other_text = Some(value.into());
                } else {
                    *comment = Some(value.into());
                }
            }
            "text" => {
                ensure!(
                    !submission.answers.contains_key(&item.id),
                    "duplicate item answer"
                );
                submission
                    .answers
                    .insert(item.id.clone(), ItemAnswer::Text { text: value.into() });
            }
            "yes" | "no" => {
                ensure!(
                    value.is_empty() && !submission.answers.contains_key(&item.id),
                    "duplicate or malformed confirmation"
                );
                submission.answers.insert(
                    item.id.clone(),
                    ItemAnswer::Confirmation { yes: kind == "yes" },
                );
            }
            _ => anyhow::bail!("unknown item directive"),
        }
        offset += line.len();
    }
    Ok(ResponseValue::Submit(submission))
}
