//! How a module learns what the modules built on it have, without naming
//! them: it asks a [`Question`], and each installed module whose manifest
//! lists it in `answers` answers, through its program's one hidden entry
//! point, `lnk-<module> --link-answer <question>`. The asker's layer defines the
//! question, both its types and the version of its wire; only a module
//! built on it can answer.
//!
//! On the wire, the asker writes one JSON line, its version of the
//! question's protocol and the question ([`Asked`]), and the answerer one
//! back, its version and the answer ([`Answered`]). Each side refuses a
//! peer below the oldest version it accepts, by name
//! ([`link_version::Protocol::accept`]).

use std::marker::PhantomData;
use std::process::Stdio;

use link_format::WithinExt;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// The hidden root flag every answering module's program takes:
/// `lnk-<module> --link-answer <question>`, outside its command space.
pub const LINK_ANSWER: &str = "--link-answer";

/// A question, by its name (`<module>.<what>`: the module that asks it,
/// and what it asks): asked with an `I` and answered with an `O`, each in
/// its envelope, in the protocol its declaration states.
pub struct Question<I, O> {
    pub name: &'static str,
    /// Its protocol's declaration, the TOML file beside the question
    /// ([`link_version::Protocol`]).
    declaration: &'static str,
    types: PhantomData<fn(I) -> O>,
}

/// A question as it's sent, a line of JSON: the asker's protocol and the
/// question.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asked<T> {
    pub protocol: u32,
    pub question: T,
}

/// An answer as it's sent, a line of JSON: the answerer's protocol and
/// the answer.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answered<T> {
    pub protocol: u32,
    pub answer: T,
}

impl<I, O> Question<I, O> {
    /// The question `name`, its protocol declared by `declaration`, the
    /// text of its TOML file: `include_str!("<what>.toml")`.
    pub const fn new(name: &'static str, declaration: &'static str) -> Self {
        Question {
            name,
            declaration,
            types: PhantomData,
        }
    }

    /// Its protocol, as its declaration states it.
    pub fn protocol(&self) -> link_version::Protocol {
        link_version::Protocol::shipped(self.declaration)
    }

    /// `input` as this release asks it.
    pub fn asked<'a>(&self, input: &'a I) -> Asked<&'a I> {
        Asked {
            protocol: self.protocol().current,
            question: input,
        }
    }

    /// The question `asked` carries: an error naming the asker's version
    /// when it's below the oldest this release accepts.
    pub fn question_of(&self, asked: Asked<I>) -> anyhow::Result<I> {
        self.protocol()
            .accept(&format!("the module asking {}", self.name), asked.protocol)?;
        Ok(asked.question)
    }

    /// `answer` as this release answers it.
    pub fn answered<'a>(&self, answer: &'a O) -> Answered<&'a O> {
        Answered {
            protocol: self.protocol().current,
            answer,
        }
    }

    /// The answer `answered` carries from `module`: an error naming it
    /// when its version is below the oldest this release accepts.
    pub fn answer_of(&self, module: &str, answered: Answered<O>) -> anyhow::Result<O> {
        self.protocol()
            .accept(&format!("the {module} module"), answered.protocol)?;
        Ok(answered.answer)
    }

    /// The installed modules that answer it.
    pub fn answerers(&self) -> Vec<Answerer> {
        self.answerers_in(&crate::modules::list())
    }

    /// The answerers in a listing the caller already obtained. Several
    /// questions can share one discovery without another command launch.
    pub fn answerers_in(&self, listed: &[crate::modules::Listed]) -> Vec<Answerer> {
        answerers_in(self.name, listed)
    }

    /// Asked of every module that answers it, what each says on stderr
    /// kept for its error: each one's answer, an error for one that
    /// failed (ending with what it last said) or answered what isn't an
    /// `O`. None when no installed module answers it.
    pub fn ask(&self, input: &I) -> Vec<Answer<O>>
    where
        I: Serialize,
        O: DeserializeOwned,
    {
        self.answerers()
            .into_iter()
            .map(|answerer| Answer {
                reply: answerer.reply(self, input, Stdio::piped()),
                module: answerer.module,
            })
            .collect()
    }

    /// What the module running `--link-answer <question>` was asked: its stdin's
    /// question, its version checked. Refused, naming this module, when
    /// it was built from other sources than the asker
    /// ([`crate::source::answering`]).
    pub fn input(&self) -> anyhow::Result<I>
    where
        I: DeserializeOwned,
    {
        crate::source::answering()?;
        let asked: Asked<I> = serde_json::from_reader(std::io::stdin().lock())
            .within(format!("reading {}'s question", self.name))?;
        self.question_of(asked)
    }

    /// `answer` as this command's answer, in its envelope on stdout.
    pub fn answer(&self, answer: &O) -> anyhow::Result<()>
    where
        O: Serialize,
    {
        use std::io::Write;
        let mut out = link_format::streams::protocol();
        serde_json::to_writer(&mut out, &self.answered(answer))?;
        writeln!(out)?;
        Ok(())
    }
}

/// A module that answers a question.
#[derive(Clone, Debug, PartialEq)]
pub struct Answerer {
    pub module: String,
    question: String,
}

impl Answerer {
    /// The command that answers, `lnk-<module> --link-answer <question>`, for a
    /// caller that exchanges with it in a protocol of its own, such as a
    /// bounded exchange with a deadline: its input and output are the
    /// caller's to connect, its envelopes the caller's to write and read
    /// ([`Question::asked`], [`Question::answer_of`]).
    pub fn command(&self) -> std::process::Command {
        let mut command = crate::module_program(&self.module);
        command.args([LINK_ANSWER, &self.question]);
        // The answerer checks it was built with this program, unasked.
        match crate::source::this() {
            Some(source) => command.env(crate::source::ASKER, source),
            None => command.env_remove(crate::source::ASKER),
        };
        command
    }

    /// Its answer to `question` asked with `input`, what it says on
    /// stderr going to `stderr`. Given [`Stdio::piped`], its stderr is
    /// kept, and when it fails, its last lines ([`Said`]) end the
    /// error.
    pub fn reply<I: Serialize, O: DeserializeOwned>(
        &self,
        question: &Question<I, O>,
        input: &I,
        stderr: Stdio,
    ) -> anyhow::Result<O> {
        use std::io::Write;
        let asking = format!("asking the {} module {}", self.module, question.name);
        let mut child = self
            .command()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .spawn()
            .within(&asking)?;
        let mut stdin = child.stdin.take().within("no stdin")?;
        let written = serde_json::to_writer(&mut stdin, &question.asked(input))
            .map_err(anyhow::Error::from)
            .and_then(|()| Ok(stdin.write_all(b"\n")?));
        drop(stdin);
        let output = child.wait_with_output().within(&asking)?;
        self.answer_in(question, &output)?;
        written.within(&asking)?;
        let answered: Answered<O> = serde_json::from_slice(&output.stdout).within(format!(
            "reading the {} module's {}",
            self.module, question.name
        ))?;
        question.answer_of(&self.module, answered)
    }

    /// Whether `output`, its answering command's, answered `question`:
    /// an error when it failed, ending with what it last said on stderr
    /// when that was kept ([`Said`]).
    fn answer_in<I, O>(
        &self,
        question: &Question<I, O>,
        output: &std::process::Output,
    ) -> anyhow::Result<()> {
        if output.status.success() {
            return Ok(());
        }
        let said = Said::shipped().last(&output.stderr);
        match said.is_empty() {
            true => anyhow::bail!("the {} module didn't answer {}", self.module, question.name),
            false => anyhow::bail!(
                "the {} module didn't answer {}: {said}",
                self.module,
                question.name
            ),
        }
    }
}

/// How much of a failed answerer's stderr its error ends with, as
/// `said.toml` sets it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Said {
    /// Of its stderr, at most this many bytes read; the rest is drained
    /// unread.
    pub bytes: u64,
    /// Its last non-empty lines, at most this many.
    pub lines: usize,
    /// Of them, at most this many characters, the end kept.
    pub chars: usize,
}

impl Said {
    /// As `said.toml` sets it, read once.
    pub fn shipped() -> &'static Said {
        static SAID: std::sync::LazyLock<Said> = std::sync::LazyLock::new(|| {
            toml::from_str(include_str!("answers/said.toml")).expect("the built-in said.toml")
        });
        &SAID
    }

    /// What `pipe`, a program's stderr, says: its first [`Said::bytes`]
    /// read, and the rest drained unread so the program never waits on a
    /// full pipe.
    pub fn read(&self, pipe: impl std::io::Read) -> Vec<u8> {
        read_at_most(pipe, self.bytes)
    }

    /// `command` run to its end, as [`std::process::Command::output`]
    /// runs it, but with its stderr read as [`Said::read`] reads it.
    pub fn output(
        &self,
        command: &mut std::process::Command,
    ) -> std::io::Result<std::process::Output> {
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stderr = child.stderr.take().map(|pipe| {
            let bytes = self.bytes;
            std::thread::spawn(move || read_at_most(pipe, bytes))
        });
        let mut stdout = Vec::new();
        if let Some(mut pipe) = child.stdout.take() {
            use std::io::Read;
            pipe.read_to_end(&mut stdout)?;
        }
        let status = child.wait()?;
        let stderr = stderr
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default();
        Ok(std::process::Output {
            status,
            stdout,
            stderr,
        })
    }

    /// What a failed answerer last said on `stderr`: its last non-empty
    /// lines, cleaned of control and invisible characters, joined by
    /// ` / ` and bounded. Empty when it said nothing.
    pub fn last(&self, stderr: &[u8]) -> String {
        let text = String::from_utf8_lossy(stderr);
        let lines: Vec<String> = text
            .lines()
            .map(|line| link_format::clean(line).trim().to_string())
            .filter(|line| !line.is_empty())
            .collect();
        let last = lines[lines.len().saturating_sub(self.lines)..].join(" / ");
        let count = last.chars().count();
        match count > self.chars {
            true => format!(
                "...{}",
                last.chars().skip(count - self.chars).collect::<String>()
            ),
            false => last,
        }
    }
}

/// `pipe`'s first `bytes`, the rest drained unread.
fn read_at_most(mut pipe: impl std::io::Read, bytes: u64) -> Vec<u8> {
    use std::io::Read;
    let mut said = Vec::new();
    let _ = pipe.by_ref().take(bytes).read_to_end(&mut said);
    let _ = std::io::copy(&mut pipe, &mut std::io::sink());
    said
}

/// One module's answer.
#[derive(Debug)]
pub struct Answer<T> {
    pub module: String,
    pub reply: anyhow::Result<T>,
}

/// Of `listed`, the installed modules that answer `question`, each a
/// module's name a program can be named by. None is run to choose them:
/// one built from other sources than this program refuses the question
/// itself, by name ([`Question::input`]).
fn answerers_in(question: &str, listed: &[crate::modules::Listed]) -> Vec<Answerer> {
    listed
        .iter()
        .filter(|p| p.installed && p.answers.iter().any(|q| q == question))
        .filter(|p| link_files::layout::check_owner(&p.name).is_ok())
        .map(|p| Answerer {
            module: p.name.clone(),
            question: question.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests;
