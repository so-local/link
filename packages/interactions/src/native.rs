//! Native review and explicit text-form submission share the normal delivery and decision store.
use crate::{constants::*, delivery::*, *};
use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub trait Dialog {
    /// None means unavailable, dismissed or closed by another surface. No answer is inferred.
    fn show(
        &self,
        store: &Store,
        review: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> anyhow::Result<Option<String>>;
}

pub fn capabilities() -> SurfaceCapabilities {
    crate::capabilities::surface(
        "native",
        crate::capabilities::current_host().unwrap_or(crate::capabilities::Host::Linux),
    )
    .expect("native capability declaration")
    .form()
}

/// Unsupported review sizes use the full CLI/chat fallback rather than a reduced native review.
pub fn present(store: &Store, id: &RequestId, dialog: &dyn Dialog) -> anyhow::Result<bool> {
    present_until(store, id, dialog, &|| false)
}

pub fn present_until(
    store: &Store,
    id: &RequestId,
    dialog: &dyn Dialog,
    stopping: &dyn Fn() -> bool,
) -> anyhow::Result<bool> {
    let envelope =
        match store.plan_delivery(id, "native:owner", "native", &capabilities(), None, now()) {
            Ok(envelope) => envelope,
            Err(_) => return Ok(false),
        };
    let cancelled = || {
        stopping()
            || store
                .snapshot(id, now())
                .map_or(true, |s| s.resolution.is_some())
    };
    let review = format!(
        "{}\n\nIn this dialog omit the response code. For confirmation enter yes or no. Continue opens an empty multiline answer form; Submit sends it once.",
        render::delivery(&envelope)
    );
    let Some(text) = dialog.show(store, &review, &cancelled)? else {
        return Ok(false);
    };
    if cancelled() {
        return Ok(false);
    }
    let value = reply::parse(
        &envelope.question,
        &text,
        false,
        false,
        text.trim() == "/dismiss",
    )?;
    store.delivered(&envelope.delivery.id, None)?;
    let ingress = Ingress {
        actor: sources::owner(),
        route: envelope.delivery.route,
        surface: envelope.delivery.surface,
        input_kind: InputKind::Typed,
    };
    store.submit(
        &ingress,
        ResponseCandidate {
            request: id.clone(),
            revision: envelope.question.revision,
            delivery: envelope.delivery.id,
            value,
            transport_event: TransportEventId::new(link_files::random_token()?)
                .map_err(anyhow::Error::msg)?,
        },
        now(),
    )?;
    Ok(true)
}

pub struct Desktop;
impl Desktop {
    pub fn available() -> bool {
        cfg!(target_os = "macos")
            || (cfg!(target_os = "linux")
                && DESKTOP_ENV[..2]
                    .iter()
                    .any(|name| std::env::var_os(name).is_some_and(|s| !s.is_empty())))
    }
}

struct ReviewFile(std::path::PathBuf);
impl Drop for ReviewFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl Dialog for Desktop {
    fn show(
        &self,
        store: &Store,
        review: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> anyhow::Result<Option<String>> {
        let env: Vec<_> =
            link_secretive::process::variables(|name| DESKTOP_ENV.contains(&name)).collect();
        Session(&env).show(store, review, cancelled)
    }
}

/// Desktop environment captured by a trusted caller, rather than inherited from a daemon.
pub struct Session<'a>(pub &'a [(String, String)]);
impl Dialog for Session<'_> {
    fn show(
        &self,
        store: &Store,
        review: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> anyhow::Result<Option<String>> {
        let available = cfg!(target_os = "macos")
            || (cfg!(target_os = "linux")
                && self.0.iter().any(|(name, value)| {
                    DESKTOP_ENV[..2].contains(&name.as_str()) && !value.is_empty()
                }));
        if !available || review.len() > capabilities().max_review_bytes || cancelled() {
            return Ok(None);
        }
        let instruction = "Enter your answer using the form shown in the review. Nothing is selected automatically. Use /dismiss to close the question. Submit sends the whole answer once.";
        if cfg!(target_os = "macos") {
            let script = "on run argv\nset reviewed to display dialog (item 1 of argv) with title \"Link question\" buttons {\"Cancel\", \"Continue\"} default button \"Continue\" cancel button \"Cancel\"\nset submitted to display dialog (item 2 of argv) with title \"Link answer\" default answer \"\" buttons {\"Cancel\", \"Submit\"} default button \"Submit\" cancel button \"Cancel\"\nreturn text returned of submitted\nend run";
            let mut command = command("/usr/bin/osascript", self.0);
            command.args(["-e", script, "--", review, instruction]);
            return run(command, cancelled);
        }
        let file = ReviewFile(
            store
                .dir()
                .join(format!("native-{}.txt", link_files::random_token()?)),
        );
        link_files::write_private(&file.0, review.as_bytes())?;
        let mut view = command("zenity", self.0);
        view.args([
            "--text-info",
            "--no-markup",
            "--title=Link question",
            "--ok-label=Continue",
            "--filename",
        ]);
        view.arg(&file.0);
        if run(view, cancelled)?.is_some() {
            // An empty editable document supports multiline sets, per-item notes and overall text.
            // The separate frozen review is never edited into new permission scope.
            let mut answer = command("zenity", self.0);
            answer.args([
                "--text-info",
                "--editable",
                "--title=Link answer — Submit sends your whole answer",
                "--ok-label=Submit",
            ]);
            return run(answer, cancelled);
        }
        if cancelled() {
            return Ok(None);
        }
        let mut view = command("kdialog", self.0);
        view.args(["--title", "Link question", "--textbox"]);
        view.arg(&file.0);
        if run(view, cancelled)?.is_none() {
            return Ok(None);
        }
        let mut answer = command("kdialog", self.0);
        answer.args(["--title", "Link answer", "--textinputbox", instruction, ""]);
        run(answer, cancelled)
    }
}

fn command(program: &str, env: &[(String, String)]) -> Command {
    let mut command = link_secretive::process::command(program);
    command.env_clear().env("PATH", SYSTEM_PATH);
    command.envs(
        env.iter()
            .filter(|(name, _)| DESKTOP_ENV.contains(&name.as_str()))
            .cloned(),
    );
    command
}

struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "native programs are polled so a winning response elsewhere closes the dialog"
)]
fn run(mut command: Command, cancelled: &dyn Fn() -> bool) -> anyhow::Result<Option<String>> {
    let mut child = match command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => Child(child),
        Err(_) => return Ok(None),
    };
    let deadline = Instant::now() + Duration::from_secs(MAX_WAIT_SECS);
    loop {
        match child.0.try_wait()? {
            Some(status) => {
                if !status.success() {
                    return Ok(None);
                }
                let mut text = String::new();
                if let Some(output) = child.0.stdout.take() {
                    output
                        .take(MAX_TEXT_BYTES as u64 + 1)
                        .read_to_string(&mut text)?;
                }
                anyhow::ensure!(text.len() <= MAX_TEXT_BYTES, "native answer is too large");
                return Ok(Some(text));
            }
            None if cancelled() || Instant::now() >= deadline => {
                return Ok(None);
            }
            None => std::thread::sleep(POLL),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake;
    impl Dialog for Fake {
        fn show(
            &self,
            _: &Store,
            review: &str,
            _: &dyn Fn() -> bool,
        ) -> anyhow::Result<Option<String>> {
            assert!(review.contains("[Recommended]"));
            assert!(review.contains("Other"));
            Ok(Some("other Keep the detail precise".into()))
        }
    }
    #[test]
    fn native_forms_keep_custom_text_and_resolve_through_the_shared_engine() {
        let root = link_test_kit::TempDir::new("native-form");
        let store = Store::at(&*root);
        let q: QuestionDraft = serde_json::from_value(serde_json::json!({"title":"Report", "caller_idempotency_key":"native", "questions":[{
            "id":"format","prompt":"Which format?", "answer_shape":{"kind":"choice","options":[{"id":"brief", "label":"Brief", "recommended":true}],
            "min_selected":1,"max_selected":1,"allow_other":true}}]})).unwrap();
        let id = store
            .create(
                sources::requester("agent", "service:m1a2i3n4s5v6", "main").unwrap(),
                sources::owner(),
                q,
                now(),
            )
            .unwrap();
        assert!(present(&store, &id, &Fake).unwrap());
        let result = store.snapshot(&id, now()).unwrap().resolution.unwrap();
        assert_eq!(result.trigger, Trigger::UserSubmission);
        assert!(
            serde_json::to_string(&result)
                .unwrap()
                .contains("Keep the detail precise")
        );
        assert!(
            !present(&store, &id, &Fake).unwrap(),
            "a losing dialog cannot answer twice"
        );
    }
}
