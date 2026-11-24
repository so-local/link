//! Resolve string-valued read arguments without evaluating repository code.

use super::scan::{Name, path};
use std::collections::{BTreeMap, BTreeSet};
use syn::Expr;

/// A function of `std::env` that touches variables other than by
/// reading one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    /// Its first argument names the variable it touches so.
    Named(link_actions::Access),
    /// Every variable the process has (`vars_os()`).
    Every,
}

#[derive(Default)]
pub(super) struct Symbols {
    pub(super) modules: BTreeSet<String>,
    pub(super) by_name: BTreeMap<String, BTreeSet<Name>>,
    pub(super) constants: BTreeMap<String, Expr>,
    pub(super) functions: BTreeSet<String>,
    pub(super) callbacks: BTreeMap<String, BTreeSet<usize>>,
    pub(super) helpers: BTreeMap<String, BTreeSet<usize>>,
    pub(super) aliases: BTreeMap<String, BTreeMap<String, String>>,
}

impl Symbols {
    pub(super) fn prepare(&mut self) {
        let mut names: BTreeMap<String, BTreeSet<Name>> = BTreeMap::new();
        for (key, value) in &self.constants {
            let name = self.value(value, key.rsplit_once("::").unwrap().0, &BTreeMap::new(), 0);
            if matches!(name, Name::Static(_) | Name::Pattern(_) | Name::Values(_)) {
                names
                    .entry(key.rsplit("::").next().unwrap().into())
                    .or_default()
                    .insert(name);
            }
        }
        self.by_name = names;
    }
    pub(super) fn qualify(&self, module: &str, name: &str) -> String {
        let mut parts = name.split("::");
        let first = parts.next().unwrap_or_default();
        let tail = parts.collect::<Vec<_>>().join("::");
        let base = if first == "crate" {
            module.split("::").next().unwrap().to_owned()
        } else if first == "self" {
            module.to_owned()
        } else if first == "super" {
            module
                .rsplit_once("::")
                .map_or(module, |(m, _)| m)
                .to_owned()
        } else if let Some(alias) = self.aliases.get(module).and_then(|a| a.get(first)) {
            alias.clone()
        } else if name.contains("::") {
            first.to_owned()
        } else {
            return format!("{module}::{name}");
        };
        if tail.is_empty() {
            base
        } else {
            format!("{base}::{tail}")
        }
    }
    fn canonical(&self, mut key: String) -> String {
        for _ in 0..12 {
            let parts: Vec<_> = key.split("::").collect();
            let mut replacement = None;
            for i in 1..parts.len() {
                let owner = parts[..i].join("::");
                if let Some(alias) = self.aliases.get(&owner).and_then(|a| a.get(parts[i])) {
                    let first = alias.split("::").next().unwrap();
                    let root = owner.split("::").next().unwrap();
                    let base = if ["crate", "self", "super"].contains(&first) {
                        self.qualify(&owner, alias)
                    } else if self.modules.contains(&format!("{owner}::{first}")) {
                        format!("{owner}::{alias}")
                    } else if self.modules.contains(&format!("{root}::{first}")) {
                        format!("{root}::{alias}")
                    } else {
                        alias.clone()
                    };
                    let tail = parts[i + 1..].join("::");
                    replacement = Some(if tail.is_empty() {
                        base
                    } else {
                        format!("{base}::{tail}")
                    });
                    break;
                }
            }
            match replacement {
                Some(next) if next != key => key = next,
                _ => break,
            }
        }
        key
    }
    pub(super) fn value(
        &self,
        expr: &Expr,
        module: &str,
        local: &BTreeMap<String, Name>,
        depth: usize,
    ) -> Name {
        if depth > 12 {
            return Name::Unresolved("constant resolution exceeded 12 steps".into());
        }
        match expr {
            Expr::Lit(e) => {
                if let syn::Lit::Str(s) = &e.lit {
                    Name::Static(s.value())
                } else {
                    Name::Input
                }
            }
            Expr::Reference(e) => self.value(&e.expr, module, local, depth + 1),
            Expr::Paren(e) => self.value(&e.expr, module, local, depth + 1),
            Expr::Path(e) => {
                let name = path(&e.path);
                if let Some(value) = local.get(&name) {
                    return value.clone();
                }
                let key = self.canonical(self.qualify(module, &name));
                if let Some(value) = self.constants.get(&key) {
                    return self.value(value, key.rsplit_once("::").unwrap().0, local, depth + 1);
                }
                let last = key.rsplit("::").next().unwrap();
                if last.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                    if let Some(candidates) = self.by_name.get(last)
                        && candidates.len() == 1
                    {
                        return candidates.iter().next().unwrap().clone();
                    }
                    return Name::Unresolved(name);
                }
                Name::Input
            }
            Expr::Array(array) => Name::Values(
                array
                    .elems
                    .iter()
                    .map(|e| self.value(e, module, local, depth + 1))
                    .collect(),
            ),
            Expr::MethodCall(call)
                if ["iter", "into_iter", "copied", "cloned", "filter", "take"]
                    .iter()
                    .any(|name| call.method == *name) =>
            {
                self.value(&call.receiver, module, local, depth + 1)
            }
            Expr::MethodCall(call)
                if ["to_owned", "to_string", "as_str", "clone"]
                    .iter()
                    .any(|name| call.method == *name) =>
            {
                self.value(&call.receiver, module, local, depth + 1)
            }
            Expr::Call(call) if matches!(call.func.as_ref(), Expr::Path(p) if ["String::from", "OsString::from"].contains(&path(&p.path).as_str())) => {
                call.args
                    .first()
                    .map_or(Name::Input, |arg| self.value(arg, module, local, depth + 1))
            }
            Expr::Macro(e) if e.mac.path.is_ident("format") || e.mac.path.is_ident("concat") => {
                let parser = syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated;
                use syn::parse::Parser;
                let Ok(args) = parser.parse2(e.mac.tokens.clone()) else {
                    return Name::Input;
                };
                if e.mac.path.is_ident("concat") {
                    let mut s = String::new();
                    for arg in args {
                        match self.value(&arg, module, local, depth + 1) {
                            Name::Static(v) => s += &v,
                            _ => return Name::Input,
                        }
                    }
                    return Name::Static(s);
                }
                if let Some(Expr::Lit(first)) = args.first()
                    && let syn::Lit::Str(s) = &first.lit
                {
                    let text = s.value();
                    let mut out = String::new();
                    let mut chars = text.chars().peekable();
                    let mut position = 1;
                    while let Some(c) = chars.next() {
                        if c != '{' {
                            out.push(c);
                            continue;
                        }
                        if chars.peek() == Some(&'{') {
                            chars.next();
                            out.push('{');
                            continue;
                        }
                        let mut field = String::new();
                        for c in chars.by_ref() {
                            if c == '}' {
                                break;
                            }
                            field.push(c);
                        }
                        let field = field.split(':').next().unwrap_or_default();
                        let value = if field.is_empty() {
                            let value = args
                                .iter()
                                .nth(position)
                                .map(|arg| self.value(arg, module, local, depth + 1));
                            position += 1;
                            value
                        } else if let Ok(index) = field.parse::<usize>() {
                            args.iter()
                                .nth(index + 1)
                                .map(|arg| self.value(arg, module, local, depth + 1))
                        } else {
                            local.get(field).cloned()
                        };
                        out += match &value {
                            Some(Name::Static(value)) => value,
                            _ => "*",
                        };
                    }
                    return if out.contains('*') {
                        Name::Pattern(out)
                    } else {
                        Name::Static(out)
                    };
                }
                Name::Input
            }
            _ => Name::Input,
        }
    }
    /// What `name`, called in `module`, does to the process's variables
    /// other than read one: set or unset the one it names, or read them
    /// all.
    pub(super) fn operation(&self, module: &str, name: &str) -> Option<Operation> {
        use link_actions::Access;
        match self.canonical(self.qualify(module, name)).as_str() {
            "std::env::set_var" => Some(Operation::Named(Access::Set)),
            "std::env::remove_var" => Some(Operation::Named(Access::Unset)),
            "std::env::vars_os" | "std::env::vars" => Some(Operation::Every),
            _ => None,
        }
    }
    pub(super) fn reader(&self, module: &str, name: &str) -> BTreeSet<usize> {
        let full = self.canonical(self.qualify(module, name));
        if matches!(full.as_str(), "std::env::var" | "std::env::var_os")
            || full.ends_with("Secret::from_env")
        {
            return BTreeSet::from([0]);
        }
        self.helpers.get(&full).cloned().unwrap_or_default()
    }
}
