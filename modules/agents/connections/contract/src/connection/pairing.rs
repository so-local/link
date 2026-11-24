/// A one-time code for the owner to send the bot: 8 letters and digits,
/// which a Telegram `/start` link can carry.
pub fn pairing_code() -> anyhow::Result<String> {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let random = link_files::random_token()?;
    let bytes = random.as_bytes().chunks(2).take(8).map(|pair| {
        let byte = u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("0"), 16).unwrap_or(0);
        ALPHABET[usize::from(byte) % ALPHABET.len()] as char
    });
    Ok(bytes.collect())
}

/// A pairing code nobody sent the bot in its time:
/// `failed` says it expired, and names the command that makes a new one.
#[derive(Debug)]
pub struct Expired {
    /// The connection the code was for, as `lnk agent connect` names it.
    pub connection: String,
    /// How long the code waited.
    pub waited: std::time::Duration,
}

impl std::fmt::Display for Expired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the code didn't reach the bot in {} minutes",
            self.waited.as_secs() / 60
        )
    }
}

impl std::error::Error for Expired {}

/// The steps a person takes on a connection's own pages, from code that
/// holds no `Out`: one paragraph each.
pub fn steps_aside(steps: &[String]) {
    for step in steps {
        link_format::aside(link_format::Text(link_format::Prose::from(
            link_format::Sentence::adopt(step.trim()),
        )));
    }
}

/// A line in the log of a program that holds no `Out` (a bridge's
/// `bridge.log`): what happened along the way.
pub fn note(text: &str) {
    link_format::aside(link_format::Note(link_format::Sentence::adopt(text)));
}

/// A warning in that log: something went wrong and it goes on.
pub fn warn(text: &str) {
    link_format::aside(link_format::Report::warning(link_format::Sentence::adopt(
        text,
    )));
}

/// What a bridge posts in the chat when agent `agent` couldn't answer:
/// never why, which holds the endpoint's address or the harness's own
/// error text, and may be read by everyone in a group. Why goes to the
/// bridge's log (stderr, `bridge.log`), here.
pub fn couldnt_answer(agent: &str, err: &anyhow::Error) -> String {
    warn(&format!("{agent}: {err:#}"));
    format!(
        "Your agent couldn't answer. Its log on its computer says why: \
         ~/.lnk/safe/agents/{}/bridge.log",
        link_format::clean(agent)
    )
}

/// Whether a message's `text` is the pairing `code`, alone (any case,
/// spaces around it) or after Telegram's `/start`, as its link sends it.
/// Never a message that merely holds it: a long one would try thousands
/// of codes at once.
pub fn is_code(text: &str, code: &str) -> bool {
    let text = text.trim();
    let text = match text.split_once(char::is_whitespace) {
        Some((start, rest)) if start.eq_ignore_ascii_case("/start") => rest.trim(),
        _ => text,
    };
    !code.is_empty() && text.eq_ignore_ascii_case(code)
}

/// Whether `text` holds `code` as a word of its own, in any case (its
/// alphabet has no two letters alike but for case), compared in constant
/// time: a mention with the code (`@company 4K7P2QXM`). A word is letters
/// and digits of any script, and `_`, so `éAB23CD45` or `AB23CD45_x`
/// holds no code.
pub fn has_code(text: &str, code: &link_secretive::Secret) -> bool {
    let code = code.expose().to_ascii_uppercase();
    !code.is_empty()
        && text
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .fold(false, |found, word| {
                link_secretive::equal(word.to_ascii_uppercase().as_bytes(), code.as_bytes()) | found
            })
}

/// `number` as the harnesses take a phone number (Signal's): `+` and 8 to 15 digits
/// (`+15551234567`). Spaces, dashes, dots and brackets are dropped, and a
/// leading `00` is read as `+`.
pub fn normalize_number(number: &str) -> Option<String> {
    let kept: String = number
        .trim()
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '.' | '(' | ')'))
        .collect();
    let digits = kept
        .strip_prefix('+')
        .or_else(|| kept.strip_prefix("00"))
        .unwrap_or(&kept);
    let valid = (8..=15).contains(&digits.len())
        && digits.bytes().all(|b| b.is_ascii_digit())
        && !digits.starts_with('0');
    valid.then(|| format!("+{digits}"))
}

/// Whether `number` is one [`normalize_number`] gives: `+` and digits.
pub fn is_number(number: &str) -> bool {
    normalize_number(number).as_deref() == Some(number)
}

/// `text` as a QR code for a terminal, two rows of modules per line: how
/// a linked connection shows the link to scan.
pub fn qr(text: &str) -> anyhow::Result<String> {
    let code = qrcode::QrCode::new(text.as_bytes())
        .map_err(|err| anyhow::anyhow!("making the QR code: {err}"))?;
    Ok(code
        .render::<qrcode::render::unicode::Dense1x2>()
        .dark_color(qrcode::render::unicode::Dense1x2::Light)
        .light_color(qrcode::render::unicode::Dense1x2::Dark)
        .quiet_zone(true)
        .build())
}

/// Opens `url` in a browser, unless `LNK_NO_BROWSER` is set.
pub fn open(url: &str) {
    if std::env::var_os("LNK_NO_BROWSER").is_some() {
        return;
    }
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = link_secretive::process::command(opener)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A share's code is a word of a message, whole, and never empty.
    #[test]
    fn a_shares_code_is_a_word_of_the_message() {
        let code = link_secretive::Secret::from("AB23CD45".to_string());
        for yes in [
            "<@U0BOT> AB23CD45",
            "AB23CD45",
            "@company AB23CD45!",
            "x\nAB23CD45\n",
            // In any case, as a phone's keyboard may write it.
            "ab23cd45",
            "<@U0BOT> Ab23Cd45.",
            "«AB23CD45»",
        ] {
            assert!(has_code(yes, &code), "{yes}");
        }
        for no in [
            "AB23CD4",
            "AB23CD456",
            "<@U0BOT>",
            "",
            "XAB23CD45",
            // A letter of any script, or `_`, makes it another word.
            "éAB23CD45",
            "AB23CD45é",
            "AB23CD45_x",
            "_AB23CD45",
        ] {
            assert!(!has_code(no, &code), "{no}");
        }
        assert!(!has_code(
            "anything",
            &link_secretive::Secret::from(String::new())
        ));
    }

    #[test]
    fn phone_numbers() {
        for (typed, number) in [
            ("+1 555 123 4567", "+15551234567"),
            ("0044 (20) 7946-0958", "+442079460958"),
            ("15551234567", "+15551234567"),
            ("+31.6.12345678", "+31612345678"),
        ] {
            assert_eq!(normalize_number(typed).as_deref(), Some(number), "{typed}");
        }
        for bad in [
            "",
            "+",
            "12345",
            "+0123456789",
            "+1555abc4567",
            "+1234567890123456",
        ] {
            assert_eq!(normalize_number(bad), None, "{bad}");
        }
        assert!(is_number("+15551234567"));
        assert!(!is_number("15551234567"));
        assert!(!is_number("+1 555"));
        assert!(!is_number("+15551234567; rm -rf ~"));
    }

    #[test]
    fn a_failure_is_told_without_its_details() {
        let err = anyhow::anyhow!("reaching the agent: http://127.0.0.1:18800/v1/chat/completions");
        let said = couldnt_answer("main", &err);
        assert!(
            !said.contains("127.0.0.1") && !said.contains("18800"),
            "{said}"
        );
        assert!(said.contains("agents/main/bridge.log"), "{said}");
    }

    #[test]
    fn a_code_is_the_whole_message() {
        assert!(is_code("K7QM2XPA", "K7QM2XPA"));
        assert!(is_code(" k7qm2xpa\n", "K7QM2XPA"));
        assert!(is_code("/start K7QM2XPA", "K7QM2XPA"));
        // Every code a long message holds is never tried.
        assert!(!is_code("AAAAK7QM2XPAAAAA", "K7QM2XPA"));
        assert!(!is_code("my code is K7QM2XPA", "K7QM2XPA"));
        assert!(!is_code("/start", "K7QM2XPA"));
        assert!(!is_code("", ""));
    }

    #[test]
    fn pairing_codes() {
        let code = pairing_code().unwrap();
        assert_eq!(code.len(), 8);
        assert!(
            code.bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        );
        assert_ne!(code, pairing_code().unwrap());
    }
}
