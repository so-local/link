//! The spec's tables and their keys: the [`Spec`] itself, its model,
//! window, turns, run and instructions, and what each one reads back.

use super::*;

/// The agent spec and its tables, as `types.toml` declares them.
pub use crate::declared::{
    Agent as Spec, AgentCommands as Commands, AgentCompactAt as CompactAt, AgentExtends as Extends,
    AgentGoals as Goals, AgentHarness as Harness, AgentInstructions as Instructions,
    AgentIsolation as Isolation, AgentModel as Model, AgentRun as Run,
    AgentSectionsMax as SectionsMax, AgentTurns as Turns, AgentWindow as Window,
};
pub const INTERACTION_WAIT_MAX: u64 = 24 * 60 * 60;

impl Isolation {
    /// Whether it says nothing: no VM.
    pub fn is_empty(&self) -> bool {
        self.vm.is_none()
    }

    /// Refuses a VM, which this build has no provider for.
    pub fn check_vm(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.vm.is_none(),
            "{}",
            link_sandbox_contract::isolation::VM_REFUSED
        );
        Ok(())
    }
}

/// What an agent spec's `[access]` takes: who may message it, chats
/// alone.
pub const ACCESS: &[crate::access::Verb] = &[crate::access::Verb::Message];

pub use link_model_contract::chat::Thinking;

impl CompactAt {
    /// The share it compacts at; none for never.
    pub fn share(&self) -> Option<f64> {
        match self {
            Self::Number(share) => Some(*share),
            Self::Word(_) => None,
        }
    }
}

/// What a message sent while a turn runs does: the harness's wire
/// vocabulary owns it, and the spec names it.
pub use link_harness_wire::runtime::Busy;

/// `[run] idle_after` and `[connections] compact_after`: never.
pub const NEVER: &str = "never";

/// `[connections] cache` and `compact_after`: as what it costs chooses.
pub const AUTO: &str = "auto";

/// When a quiet conversation is compacted (`[connections] compact_after`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quiet {
    /// Just before its cache ends, when that costs less than reading it
    /// again.
    Auto,
    /// After this long with no message.
    After(std::time::Duration),
}

impl Run {
    /// How long it may be idle before it stops, as [`Self::idle_after`]
    /// says: none with [`NEVER`], or when it doesn't say.
    pub fn idle_after(&self) -> Option<std::time::Duration> {
        let after = self.idle_after.as_deref().filter(|a| *a != NEVER)?;
        duration_ms(after)
            .ok()
            .map(std::time::Duration::from_millis)
    }
}

/// Where a section of the prompt comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// Link's own.
    Link,
    /// Nothing: the section is left out.
    None,
    /// A file in the agent's files folder, by its path there.
    File(String),
}

impl Source {
    /// `text` as a source: `link`, `none`, or a markdown file's path in
    /// the files folder (relative, plain names, none starting with `.`,
    /// ending `.md`, at most 256 bytes).
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        Ok(match text {
            "link" => Self::Link,
            "none" => Self::None,
            path => {
                let plain = (1..=256).contains(&path.len())
                    && path.ends_with(".md")
                    && path.split('/').all(|part| {
                        !part.is_empty()
                            && !part.starts_with('.')
                            && part
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b"-_. ".contains(&b))
                    });
                ensure!(
                    plain,
                    "{:?} is link, none, or a markdown file in the agent's files folder \
                     (\"instructions/base.md\")",
                    link_format::clean(path)
                );
                Self::File(path.to_string())
            }
        })
    }
}

/// The most a section may be: the store's guard on what it reads.
pub const SECTION_MAX: u64 = 64 << 10;

impl Instructions {
    /// Section `name`'s source and most, in bytes, as set.
    pub fn section(&self, name: &str) -> (Option<Source>, Option<u64>) {
        let max = self.max.clone().unwrap_or_default();
        let (source, most) = match name {
            "base" => (&self.base, &max.base),
            "model" => (&None, &max.model),
            "tools" => (&None, &max.tools),
            "environment" => (&self.environment, &max.environment),
            "repository" => (&self.repository, &max.repository),
            "skills" => (&self.skills, &max.skills),
            "persona" => (&self.persona, &max.persona),
            "owner" => (&self.owner, &max.owner),
            "standing" => (&self.standing, &max.standing),
            "now" => (&self.now, &max.now),
            "notes" => (&self.notes, &max.notes),
            _ => (&None, &None),
        };
        (
            source.as_deref().and_then(|source| {
                // The old default names the same notes after their move;
                // an agent's spec stays as its owner wrote it.
                let source = match (name, source) {
                    ("notes", "instructions/notes.md") => crate::harness::layout::NOTES,
                    _ => source,
                };
                Source::parse(source).ok()
            }),
            most.as_deref().and_then(|m| size(m).ok()),
        )
    }
}

impl Spec {
    /// Its `[access]`, as the identity contract reads it: the chats that
    /// may message the agent, each parsed; none when it says none. A
    /// spec read is checked whole, its access among it ([`check`]).
    pub fn message_access(&self) -> crate::access::MessageAccess {
        self.access
            .as_ref()
            .and_then(|access| super::message_access(access).ok())
            .unwrap_or_default()
    }

    /// The whole prompt's source. Specs written before this setting
    /// retain Link's assembly; section settings apply only to that source.
    pub fn instruction_source(&self) -> Source {
        let of = |spec: &Spec| {
            spec.instructions
                .as_ref()
                .and_then(|i| i.source.as_deref())
                .and_then(|source| Source::parse(source).ok())
        };
        of(self).or_else(|| of(&link())).unwrap_or(Source::None)
    }

    /// The model's opinions for `provider`: the provider's table over
    /// the default one. `[model] default` names who serves it
    /// (`openai/gpt-6.1-sol`), so it's this provider's default only when
    /// it names this provider, as the id that provider knows it by.
    pub fn model_for(&self, provider: &str) -> Model {
        let mut model = self.model.clone().unwrap_or_default();
        model.default = model.default.and_then(|name| {
            let route = link_model_contract::selection::Route::parse(&name).ok()?;
            route.may_be(provider).then_some(route.id)
        });
        let own = match provider {
            "anthropic" => &model.anthropic,
            "openai" => &model.openai,
            "openrouter" => &model.openrouter,
            "ollama" => &model.ollama,
            "local" => &model.local,
            _ => &None,
        }
        .clone();
        let Some(own) = own else {
            return Model {
                anthropic: None,
                openai: None,
                openrouter: None,
                ollama: None,
                local: None,
                ..model
            };
        };
        Model {
            default: own.default.or(model.default),
            allow: own.allow.or(model.allow),
            answer: own.answer.or(model.answer),
            thinking: own.thinking.or(model.thinking),
            thinking_budget: own.thinking_budget.or(model.thinking_budget),
            transcribe: own.transcribe.or(model.transcribe),
            aliases: model.aliases,
            ..Model::default()
        }
    }

    /// The model `name` names: the one an alias of `[model.aliases]`
    /// stands for, else itself.
    pub fn model_named(&self, name: &str) -> String {
        self.model
            .as_ref()
            .and_then(|model| model.aliases.get(name))
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    /// The tools a stranger's message may use (`[tools] strangers`): none
    /// unless it says.
    pub fn strangers(&self) -> Vec<String> {
        self.tools
            .as_ref()
            .and_then(|tools| tools.strangers.clone())
            .unwrap_or_default()
    }

    /// Tool `name`'s opinions: its table over `[tools]`.
    pub fn tool(&self, name: &str) -> Tool {
        let all = self.tools.clone().unwrap_or_default();
        let own = self.tool.get(name).cloned().unwrap_or_default();
        Tool {
            answer: own.answer.or(all.answer),
            timeout: own.timeout.or(all.timeout),
            keep: own.keep,
            max_age: own.max_age,
            tokens: own.tokens,
            model: own.model,
            notes: own.notes,
            dimensions: own.dimensions,
            memory: own.memory,
            reserve: own.reserve,
            file: own.file,
            threads: own.threads,
            deadline: own.deadline,
            folders: own.folders,
            kept_off: own.kept_off,
            per_day: own.per_day,
            subagents: own.subagents,
            hops: own.hops,
            late: own.late,
            ask: own.ask,
            recurring: own.recurring,
            zone: own.zone,
            interval: own.interval,
            firing: own.firing,
            enabled: own.enabled,
            driver: own.driver,
            find: own.find,
            diffs: own.diffs,
            fetch: own.fetch,
            hosts: own.hosts,
            program: own.program,
            output: own.output,
            background: own.background,
            background_timeout: own.background_timeout,
            background_output: own.background_output,
            background_keep: own.background_keep,
            background_per_day: own.background_per_day,
            servers: own.servers,
            strangers: None,
            others: own.others,
        }
    }

    /// Connection `name`'s opinions: its table over `[connections]`.
    pub fn connection(&self, name: &str) -> Connection {
        let all = self.connections.clone().unwrap_or_default();
        let own = self.connection.get(name).cloned().unwrap_or_default();
        // Empty: none.
        let some = |reaction: Option<String>| reaction.filter(|r| !r.is_empty());
        Connection {
            working: some(own.working.or(all.working)),
            steered: some(own.steered.or(all.steered)),
            stream: own.stream.or(all.stream),
            activity: own.activity.or(all.activity),
            activity_mark: own.activity_mark.or(all.activity_mark),
            activity_separator: own.activity_separator.or(all.activity_separator),
            pace: own.pace.or(all.pace),
            groups: own.groups.or(all.groups),
            images: own.images.or(all.images),
            documents: own.documents.or(all.documents),
            voice: own.voice.or(all.voice),
            ask_wait: all.ask_wait,
            cache: own.cache.or(all.cache),
            compact_after: own.compact_after.or(all.compact_after),
            pauses: own.pauses.or(all.pauses),
        }
    }

    /// When a quiet conversation on connection `name` is compacted, as
    /// its `compact_after` says, its own over `[connections]`: none with
    /// [`NEVER`], or where neither says.
    pub fn compact_after(&self, name: &str) -> Option<Quiet> {
        match self.connection(name).compact_after.as_deref()? {
            NEVER => None,
            AUTO => Some(Quiet::Auto),
            after => duration_ms(after)
                .ok()
                .map(|ms| Quiet::After(std::time::Duration::from_millis(ms))),
        }
    }

    /// The lifetime a conversation on connection `name` asks of its
    /// provider's cache, as its `cache` says: none for `"auto"`, or where
    /// neither says, for the cheapest to be chosen.
    pub fn cache_for(&self, name: &str) -> Option<String> {
        self.connection(name).cache.filter(|cache| cache != AUTO)
    }

    /// What's assumed of a new conversation's pauses on connection
    /// `name`, as its `pauses` say.
    pub fn pauses(&self, name: &str) -> Vec<std::time::Duration> {
        self.connection(name)
            .pauses
            .unwrap_or_default()
            .into_iter()
            .map(link_actions::value::Duration::get)
            .collect()
    }
}
