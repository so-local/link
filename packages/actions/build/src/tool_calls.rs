//! A program's check of its tool calls: each call's arguments read as
//! its action's arguments struct and run through its `Validate`, before
//! its handler, as a flag is.

use std::fmt::Write;

use crate::names::{method, pascal};

/// `check_call` for a module offering tool calls, by each call's name;
/// nothing for one that offers none.
pub(crate) fn check_call(module: &link_actions::Module) -> String {
    let mut arms = String::new();
    for action in module.actions.values() {
        if let Some(tool) = &action.tool {
            let stem = pascal(&method(&action.id));
            let _ = writeln!(
                arms,
                "        {:?} => ::link_actions::tools::check_arguments::<{stem}Args>(arguments),",
                tool.name
            );
        }
    }
    if arms.is_empty() {
        return String::new();
    }
    format!(
        "/// A model's call refused by its declaration before its handler,\n\
         /// as a flag is: its arguments read as its action's, each pattern,\n\
         /// bound and choice held. A name no action calls is refused.\n\
         pub fn check_call(name: &str, arguments: &::serde_json::Value) -> Result<(), ::link_actions::value::Refusal> {{\n    \
         match name {{\n{arms}        _ => Err(::link_actions::tools::no_call(name)),\n    }}\n}}\n\n"
    )
}
