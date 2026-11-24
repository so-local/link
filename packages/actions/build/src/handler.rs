//! The trait a module's handler implements, a method for each action, and
//! the dispatch every other surface calls it through: a call's arguments
//! and spec read into their types and checked, the method called and its
//! output written back.

use std::fmt::Write as _;

use crate::{
    names::{method, pascal},
    types::Types,
};

pub(crate) fn write(types: &mut Types<'_>) -> anyhow::Result<String> {
    let module = types.module;
    let trait_name = format!("{}Actions", pascal(&module.identity.id));
    let mut methods = String::new();
    let mut arms = String::new();
    for action in module.actions.values() {
        let name = method(&action.id);
        let stem = pascal(&name);
        let output = types.rust(&action.output, &format!("{stem}Output"))?;
        let spec = types.spec_type(action)?.unwrap_or_else(|| "()".into());
        // How its spec is checked: a declared kind's `Validate`, a held
        // kind's own checks, or none.
        let check = match (&action.spec, &action.held) {
            (Some(_), _) => "|spec| ::link_actions::value::Validate::validate(spec, \"spec\")",
            (None, Some(_)) => {
                "|spec| ::link_spec::Kind::check(spec).map_err(|e| ::link_actions::value::Refusal { at: \"spec\".into(), why: format!(\"{e:#}\") })"
            }
            (None, None) => "|_: &()| Ok(())",
        };
        let _ = writeln!(
            methods,
            "    /// {}\n    fn {name}(&self, call: ::link_actions::call::Call<{stem}Args, {spec}>) -> ::link_actions::call::Outcome<{output}>;",
            action.summary()
        );
        // A bound action's spec file is the command line's alone: a call
        // carries its spec whole.
        let file = &link_actions::core().spec;
        let bound = action.spec.is_some() || action.held.is_some();
        let file = match bound && !action.args.contains_key(file) {
            true => format!("Some({file:?})"),
            false => "None".to_string(),
        };
        let _ = writeln!(
            arms,
            "        {:?} => run(call, {file}, {check}, |call| handler.{name}(call)),",
            action.id.as_str()
        );
    }
    let mut out = String::new();
    let _ = write!(
        out,
        "/// What the {id} module's handler does: a method for each action.\n\
         pub trait {trait_name}: Send + Sync {{\n{methods}}}\n\n\
         /// Reads `call`'s arguments and spec into its action's types, checks\n\
         /// them, calls the handler and writes the outcome back. A call of\n\
         /// another shape is refused, never made.\n\
         pub fn dispatch(\n    handler: &dyn {trait_name},\n    \
         call: ::link_actions::call::Call<::serde_json::Value, ::serde_json::Value>,\n) \
         -> ::link_actions::call::Outcome<::serde_json::Value> {{\n    \
         match call.action.as_str() {{\n{arms}        \
         other => ::link_actions::call::Outcome::Refused {{ reason: format!(\"{{other}} isn't an action of module {id}\") }},\n    }}\n}}\n\n\
         fn run<A, S, O>(\n    call: ::link_actions::call::Call<::serde_json::Value, ::serde_json::Value>,\n    \
         file: Option<&str>,\n    \
         check: impl FnOnce(&S) -> Result<(), ::link_actions::value::Refusal>,\n    \
         method: impl FnOnce(::link_actions::call::Call<A, S>) -> ::link_actions::call::Outcome<O>,\n) \
         -> ::link_actions::call::Outcome<::serde_json::Value>\nwhere\n    \
         A: ::serde::de::DeserializeOwned + ::link_actions::value::Validate,\n    \
         S: ::serde::de::DeserializeOwned,\n    O: ::serde::Serialize,\n{{\n    \
         let refused = |reason: String| ::link_actions::call::Outcome::Refused {{ reason }};\n    \
         if let Some(file) = file.filter(|file| call.args.get(*file).is_some()) {{\n        \
         return refused(format!(\"its spec is the call's, never a file in its arguments ({{file}})\"));\n    }}\n    \
         let args: A = match ::serde_json::from_value(call.args.clone()) {{\n        \
         Ok(args) => args,\n        Err(e) => return refused(format!(\"its arguments aren't the action's: {{e}}\")),\n    }};\n    \
         let spec: S = match ::serde_json::from_value(call.spec.clone()) {{\n        \
         Ok(spec) => spec,\n        Err(e) => return refused(format!(\"its spec isn't the action's: {{e}}\")),\n    }};\n    \
         if let Err(refusal) = ::link_actions::value::Validate::validate(&args, \"\").and_then(|()| check(&spec)) {{\n        \
         return refused(refusal.to_string());\n    }}\n    \
         match method(call.with(args, spec)) {{\n        \
         ::link_actions::call::Outcome::Completed {{ output }} => match ::serde_json::to_value(output) {{\n            \
         Ok(output) => ::link_actions::call::Outcome::Completed {{ output }},\n            \
         Err(error) => ::link_actions::call::Outcome::Failed {{ reason: error.to_string() }},\n        }},\n        \
         other => other.map(|_| ::serde_json::Value::Null),\n    }}\n}}\n",
        id = module.identity.id,
    );
    Ok(out)
}
