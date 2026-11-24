//! The words the tool says and reads that a model sees, named once.

/// `answer`'s word for this chat.
pub(crate) const HERE: &str = "here";

/// `answer`'s word for keeping the answer private.
pub(crate) const NOWHERE: &str = "nowhere";

/// `answer`'s word for the owner's command line.
pub(crate) const COMMAND_LINE: &str = "cli";

/// The call that lists a conversation's jobs, as an agent's `task`
/// namespace offers it to its model.
pub(crate) const LIST_CALL: &str = "task__list";

/// The call that lists a conversation's schedules alone, as an agent's
/// `cron` namespace offers it to its model.
pub(crate) const SCHEDULES_CALL: &str = "cron__list";
