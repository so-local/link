//! `lnk-webhook-connection sift`, case by case: a post signed as its webhook's
//! sender signs, Link or GitHub, with its secret or the previous one while
//! it rolls, carrying JSON or text, is taken with the id its scheme signs;
//! every other post is dropped with its answer. And the program itself,
//! reading lines and writing a verdict for each.

use std::collections::BTreeMap;
use std::io::Write as _;

use link_connection_contract::pushed::{Sifting, UPDATE_MAX, Verdict};
use link_secretive::OpaqueSecret;
use link_secretive::Secret;
use link_secretive::inbound::Scheme;
use link_webhook_connection::sift_at;
use serde_json::json;

const NOW: u64 = 1_800_000_000;
const SECRET: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";
const PREVIOUS: &str = "a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a6978879";
const BODY: &str = r#"{"ref":"main"}"#;

/// A post to webhook `deploy`, from `from`, signed with `with` as `scheme`
/// signs at `time` with id `id`, of `body` as `content_type`.
struct Post {
    from: &'static str,
    scheme: Scheme,
    with: &'static str,
    time: u64,
    id: &'static str,
    body: String,
    content_type: Option<&'static str>,
    previous: &'static str,
}

impl Post {
    fn link() -> Self {
        Post {
            from: "link",
            scheme: Scheme::LINK,
            with: SECRET,
            time: NOW,
            id: "d-1",
            body: BODY.into(),
            content_type: Some("application/json"),
            previous: "",
        }
    }

    fn github() -> Self {
        Post {
            from: "github",
            scheme: Scheme::GITHUB,
            ..Post::link()
        }
    }

    fn sifting(&self) -> Sifting {
        let secret = OpaqueSecret::from_text(Secret::new(self.with.to_string()));
        let mut headers: BTreeMap<String, String> = self
            .scheme
            .sign(&secret, self.time, self.id, self.body.as_bytes())
            .unwrap();
        if self.from == "github" {
            // GitHub's own id, which it doesn't sign.
            headers.insert("x-github-delivery".into(), self.id.into());
        }
        if let Some(content_type) = self.content_type {
            headers.insert("content-type".into(), content_type.into());
        }
        Sifting {
            settings: json!({"name": "deploy", "from": self.from}),
            secret: Secret::new(SECRET.to_string()),
            previous: Secret::new(self.previous.to_string()),
            headers,
            body: self.body.clone(),
            ..Sifting::default()
        }
    }

    fn sifted(&self) -> Verdict {
        sift_at(&self.sifting(), NOW)
    }
}

fn taken(dedupe: &str) -> Verdict {
    Verdict::Take {
        thread: "webhook:deploy".into(),
        chat: "deploy".into(),
        dedupe: dedupe.into(),
        from: None,
    }
}

#[test]
fn a_post_signed_as_link_signs_is_taken_by_its_id() {
    assert_eq!(Post::link().sifted(), taken("d-1"));
    // Within its window, either way.
    let late = Post {
        time: NOW - 300,
        ..Post::link()
    };
    assert_eq!(late.sifted(), taken("d-1"));
}

#[test]
fn a_post_signed_as_github_signs_is_taken_by_its_bodys_hash() {
    let hash = link_digest::sha256(BODY.as_bytes());
    assert_eq!(Post::github().sifted(), taken(&hash));
    // GitHub signs no delivery id: the same body under another is the
    // same post.
    let again = Post {
        id: "d-2",
        ..Post::github()
    };
    assert_eq!(again.sifted(), taken(&hash));
    // And signs no time: a post signed long ago is its body still, which
    // the bridge keeps 30 days.
    let old = Post {
        time: NOW - 86_400,
        ..Post::github()
    };
    assert_eq!(old.sifted(), taken(&hash));
}

#[test]
fn the_previous_secret_is_taken_while_it_rolls() {
    let rolling = Post {
        with: PREVIOUS,
        previous: PREVIOUS,
        ..Post::link()
    };
    assert_eq!(rolling.sifted(), taken("d-1"));
    // Once it's dropped, it's another secret.
    let dropped = Post {
        with: PREVIOUS,
        ..Post::link()
    };
    assert_eq!(dropped.sifted(), Verdict::drop(401));
    let github = Post {
        with: PREVIOUS,
        previous: PREVIOUS,
        ..Post::github()
    };
    assert!(matches!(github.sifted(), Verdict::Take { .. }));
}

#[test]
fn every_other_post_is_dropped_with_its_answer() {
    let cases: Vec<(&str, Sifting, u16)> = vec![
        (
            "a stranger's secret",
            Post {
                with: PREVIOUS,
                ..Post::link()
            }
            .sifting(),
            401,
        ),
        (
            "outside the window",
            Post {
                time: NOW - 301,
                ..Post::link()
            }
            .sifting(),
            401,
        ),
        (
            "signed as Link signs, at GitHub's",
            Sifting {
                settings: json!({"name": "deploy", "from": "github"}),
                ..Post::link().sifting()
            },
            401,
        ),
        // Without the time Link's scheme signs: malformed.
        (
            "signed as GitHub signs, at Link's",
            Sifting {
                settings: json!({"name": "deploy", "from": "link"}),
                ..Post::github().sifting()
            },
            400,
        ),
        (
            "unsigned",
            Sifting {
                headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
                ..Post::link().sifting()
            },
            400,
        ),
        (
            "with its time and id, unsigned",
            {
                let mut sifting = Post::link().sifting();
                sifting.headers.remove("link-signature");
                sifting
            },
            401,
        ),
        (
            "unsigned, at GitHub's",
            Sifting {
                headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
                ..Post::github().sifting()
            },
            401,
        ),
        (
            "a body changed after it was signed",
            Sifting {
                body: r#"{"ref":"evil"}"#.into(),
                ..Post::link().sifting()
            },
            401,
        ),
        (
            "with no time",
            {
                let mut sifting = Post::link().sifting();
                sifting.headers.remove("link-time");
                sifting
            },
            400,
        ),
        (
            "over 1 MiB",
            Post {
                body: format!("\"{}\"", "x".repeat(UPDATE_MAX)),
                ..Post::link()
            }
            .sifting(),
            413,
        ),
        (
            "a form",
            Post {
                body: "payload=%7B%7D".into(),
                content_type: Some("application/x-www-form-urlencoded"),
                ..Post::github()
            }
            .sifting(),
            415,
        ),
        (
            "bytes",
            Post {
                content_type: Some("application/octet-stream"),
                ..Post::link()
            }
            .sifting(),
            415,
        ),
        (
            "no content type",
            Post {
                content_type: None,
                ..Post::link()
            }
            .sifting(),
            415,
        ),
        (
            "JSON that isn't",
            Post {
                body: "{not json".into(),
                ..Post::link()
            }
            .sifting(),
            400,
        ),
        (
            "a webhook with no secret",
            Sifting {
                secret: Secret::default(),
                ..Post::link().sifting()
            },
            503,
        ),
        (
            "a webhook from a sender it doesn't know",
            Sifting {
                settings: json!({"name": "deploy", "from": "gitlab"}),
                ..Post::link().sifting()
            },
            503,
        ),
        (
            "a webhook that doesn't say who signs it",
            Sifting {
                settings: json!({"name": "deploy"}),
                ..Post::link().sifting()
            },
            503,
        ),
        (
            "a webhook with no name",
            Sifting {
                settings: json!({"from": "link"}),
                ..Post::link().sifting()
            },
            503,
        ),
        (
            "a webhook whose name isn't one",
            Sifting {
                settings: json!({"name": "a\0b", "from": "link"}),
                ..Post::link().sifting()
            },
            503,
        ),
    ];
    for (case, sifting, status) in cases {
        assert_eq!(sift_at(&sifting, NOW), Verdict::drop(status), "{case}");
    }
}

#[test]
fn text_is_taken_as_json_is() {
    for content_type in [
        "text/plain",
        "text/plain; charset=utf-8",
        "Application/JSON; charset=utf-8",
        "application/vnd.github+json",
    ] {
        let post = Post {
            content_type: Some(content_type),
            ..Post::link()
        };
        assert_eq!(post.sifted(), taken("d-1"), "{content_type}");
    }
    let text = Post {
        body: "deployed main".into(),
        content_type: Some("text/plain"),
        ..Post::link()
    };
    assert_eq!(text.sifted(), taken("d-1"));
}

/// The program, as the bridge runs it: a line in, a verdict out, for each.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn sift_reads_lines_and_answers_each() {
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_lnk-webhook-connection"))
        .arg("sift")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let right = Post {
        time: now,
        ..Post::link()
    };
    let wrong = Post {
        time: now,
        with: PREVIOUS,
        ..Post::link()
    };
    // Signed an hour ago: stale, by the clock it reads.
    let stale = Post {
        time: now - 3600,
        ..Post::link()
    };
    for post in [right, wrong, stale] {
        let line = serde_json::to_string(&post.sifting()).unwrap();
        writeln!(stdin, "{line}").unwrap();
    }
    writeln!(stdin, "not a line it can read").unwrap();
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let verdicts: Vec<Verdict> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        verdicts,
        [
            taken("d-1"),
            Verdict::drop(401),
            Verdict::drop(401),
            Verdict::drop(400)
        ]
    );

    let version = std::process::Command::new(env!("CARGO_BIN_EXE_lnk-webhook-connection"))
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        format!("lnk-webhook-connection {}", env!("CARGO_PKG_VERSION"))
    );
}
