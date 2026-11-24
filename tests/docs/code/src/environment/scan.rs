//! A syntax pass follows environment readers and their string-valued inputs.
//! Rust's parser removes comments and string contents from the call graph.

use super::collect::{Callbacks, Gather, Helper, Helpers, parameters, tested};
use super::resolve::{Operation, Symbols};
use super::source::{Source, sources};
use link_actions::{Access, When};
use link_docs_check::{Finding, Tree};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use syn::visit::{self, Visit};
use syn::{Expr, Item, Pat};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Name {
    Static(String),
    Pattern(String),
    Input,
    Values(Vec<Name>),
    Unresolved(String),
}
/// One use of a variable: where, by which function, of which name, how
/// it's touched and when.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Read {
    pub path: PathBuf,
    pub owner: PathBuf,
    pub api: String,
    pub name: Name,
    pub access: Access,
    pub context: When,
}

/// A method of a child's command that touches its variables, and how:
/// `Command::env` sets one, `env_remove` unsets one, `envs` sets each it
/// is given. `env_clear` names none: it only narrows what the child gets.
const METHODS: [(&str, usize, Access); 3] = [
    ("env", 2, Access::Set),
    ("env_remove", 1, Access::Unset),
    ("envs", 1, Access::Set),
];

/// A macro reading a variable as the crate is compiled.
const COMPILED: [&str; 2] = ["env", "option_env"];

pub(super) fn path(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}
struct Walker<'a> {
    symbols: &'a Symbols,
    source: &'a Source,
    module: String,
    api: String,
    locals: BTreeMap<String, Name>,
    calls: BTreeMap<String, BTreeSet<usize>>,
    reads: BTreeSet<Read>,
}
impl<'a> Walker<'a> {
    fn bind_callbacks(&mut self, signature: &syn::Signature) {
        if let Some(indices) = self
            .symbols
            .callbacks
            .get(&format!("{}::{}", self.module, signature.ident))
        {
            for (i, argument) in signature.inputs.iter().enumerate() {
                if indices.contains(&i)
                    && let syn::FnArg::Typed(argument) = argument
                    && let Pat::Ident(name) = argument.pat.as_ref()
                {
                    self.calls
                        .insert(name.ident.to_string(), BTreeSet::from([0]));
                }
            }
        }
    }
    fn record(&mut self, expr: &Expr, access: Access, context: When) {
        let value = self.symbols.value(expr, &self.module, &self.locals, 0);
        self.record_name(value, access, context);
    }
    fn record_name(&mut self, name: Name, access: Access, context: When) {
        let mut values = vec![name];
        while let Some(name) = values.pop() {
            if let Name::Values(names) = name {
                values.extend(names);
            } else {
                self.reads.insert(Read {
                    path: self.source.path.clone(),
                    owner: self.source.owner.clone(),
                    api: self.api.clone(),
                    name,
                    access,
                    context,
                });
            }
        }
    }
    /// `envs`' argument: the names of a list of pairs written out, else
    /// whatever it is.
    fn record_pairs(&mut self, expr: &Expr, access: Access) {
        let pairs = match expr {
            Expr::Reference(e) => e.expr.as_ref(),
            other => other,
        };
        match pairs {
            Expr::Array(array) if array.elems.iter().all(|e| matches!(e, Expr::Tuple(_))) => {
                for pair in &array.elems {
                    if let Expr::Tuple(pair) = pair
                        && let Some(name) = pair.elems.first()
                    {
                        self.record(name, access, When::Runtime);
                    }
                }
            }
            _ => self.record(expr, access, When::Runtime),
        }
    }
    fn bind(&mut self, pattern: &Pat, value: Name) {
        match pattern {
            Pat::Ident(name) => {
                self.locals.insert(name.ident.to_string(), value);
            }
            Pat::Reference(p) => self.bind(&p.pat, value),
            Pat::Type(p) => self.bind(&p.pat, value),
            _ => {}
        }
    }
}
/// A statement's attributes, where it carries them: `#[cfg(test)]` on an
/// expression or a binding makes it test code, as on an item.
fn statement_attrs(statement: &syn::Stmt) -> &[syn::Attribute] {
    match statement {
        syn::Stmt::Local(local) => &local.attrs,
        syn::Stmt::Expr(expr, _) => match expr {
            Expr::If(e) => &e.attrs,
            Expr::Block(e) => &e.attrs,
            Expr::MethodCall(e) => &e.attrs,
            Expr::Call(e) => &e.attrs,
            Expr::Macro(e) => &e.attrs,
            Expr::Assign(e) => &e.attrs,
            Expr::ForLoop(e) => &e.attrs,
            Expr::Match(e) => &e.attrs,
            _ => &[],
        },
        syn::Stmt::Macro(mac) => &mac.attrs,
        syn::Stmt::Item(_) => &[],
    }
}

impl<'ast> Visit<'ast> for Walker<'_> {
    fn visit_stmt(&mut self, statement: &'ast syn::Stmt) {
        if !tested(statement_attrs(statement)) {
            visit::visit_stmt(self, statement);
        }
    }
    fn visit_item(&mut self, item: &'ast Item) {
        let attrs = match item {
            Item::Fn(i) => &i.attrs,
            Item::Mod(i) => &i.attrs,
            _ => return visit::visit_item(self, item),
        };
        if !tested(attrs) {
            visit::visit_item(self, item);
        }
    }
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let old = self.module.clone();
        self.module += &format!("::{}", item.ident);
        visit::visit_item_mod(self, item);
        self.module = old;
    }
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let old = self.api.clone();
        let locals = std::mem::take(&mut self.locals);
        let calls = std::mem::take(&mut self.calls);
        self.api = format!("{}::{}", self.module, item.sig.ident);
        self.bind_callbacks(&item.sig);
        visit::visit_item_fn(self, item);
        self.api = old;
        self.locals = locals;
        self.calls = calls;
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if tested(&item.attrs) {
            return;
        }
        let old = self.api.clone();
        let locals = std::mem::take(&mut self.locals);
        let calls = std::mem::take(&mut self.calls);
        self.api = format!("{}::{}", self.module, item.sig.ident);
        self.bind_callbacks(&item.sig);
        visit::visit_impl_item_fn(self, item);
        self.api = old;
        self.locals = locals;
        self.calls = calls;
    }
    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let (Pat::Ident(name), Some(init)) = (&local.pat, &local.init) {
            self.locals.insert(
                name.ident.to_string(),
                self.symbols
                    .value(&init.expr, &self.module, &self.locals, 0),
            );
            if let Expr::Path(function) = init.expr.as_ref() {
                let reader = self.symbols.reader(&self.module, &path(&function.path));
                if !reader.is_empty() {
                    self.calls.insert(name.ident.to_string(), reader);
                }
            }
            if let Expr::Closure(closure) = init.expr.as_ref() {
                let mut helper = Helper {
                    symbols: self.symbols,
                    module: &self.module,
                    parameters: parameters(closure.inputs.iter()),
                    found: BTreeSet::new(),
                };
                helper.visit_expr(&closure.body);
                if !helper.found.is_empty() {
                    self.calls.insert(name.ident.to_string(), helper.found);
                    return;
                }
            }
        }
        visit::visit_local(self, local)
    }
    fn visit_expr_for_loop(&mut self, expression: &'ast syn::ExprForLoop) {
        let old = self.locals.clone();
        let value = self
            .symbols
            .value(&expression.expr, &self.module, &self.locals, 0);
        self.bind(&expression.pat, value);
        visit::visit_expr_for_loop(self, expression);
        self.locals = old;
    }
    fn visit_expr_method_call(&mut self, expression: &'ast syn::ExprMethodCall) {
        let method = expression.method.to_string();
        if let Some((_, _, access)) = METHODS
            .iter()
            .find(|(name, count, _)| method == *name && expression.args.len() == *count)
            && let Some(first) = expression.args.first()
        {
            match method.as_str() {
                "envs" => self.record_pairs(first, *access),
                _ => self.record(first, *access, When::Runtime),
            }
        }
        let value = self
            .symbols
            .value(&expression.receiver, &self.module, &self.locals, 0);
        if matches!(value, Name::Values(_)) {
            self.visit_expr(&expression.receiver);
            for arg in &expression.args {
                if let Expr::Closure(closure) = arg {
                    let old = self.locals.clone();
                    if let Some(pattern) = closure.inputs.first() {
                        self.bind(pattern, value.clone());
                    }
                    self.visit_expr(&closure.body);
                    self.locals = old;
                } else {
                    self.visit_expr(arg);
                }
            }
        } else {
            visit::visit_expr_method_call(self, expression);
        }
    }
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        use syn::parse::Parser;
        let parser = syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated;
        if let Ok(args) = parser.parse2(mac.tokens.clone()) {
            if COMPILED.iter().any(|name| mac.path.is_ident(name))
                && let Some(first) = args.first()
            {
                self.record(first, Access::Read, When::Build);
            }
            for arg in &args {
                self.visit_expr(arg);
            }
        }
    }
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Expr::Path(p) = call.func.as_ref() {
            let name = path(&p.path);
            let indices = self
                .calls
                .get(&name)
                .cloned()
                .unwrap_or_else(|| self.symbols.reader(&self.module, &name));
            for index in indices {
                if let Some(arg) = call.args.iter().nth(index) {
                    self.record(arg, Access::Read, When::Runtime);
                }
            }
            match self.symbols.operation(&self.module, &name) {
                // The whole environment: no name of its own.
                Some(Operation::Every) => {
                    self.record_name(Name::Input, Access::Read, When::Runtime)
                }
                Some(Operation::Named(access)) => {
                    if let Some(arg) = call.args.first() {
                        self.record(arg, access, When::Runtime);
                    }
                }
                None => {}
            }
        }
        self.visit_expr(&call.func);
        let callbacks = if let Expr::Path(function) = call.func.as_ref() {
            self.symbols.callbacks.get(&format!(
                "{}::{}",
                self.module,
                function.path.segments.last().unwrap().ident
            ))
        } else {
            None
        };
        for (i, arg) in call.args.iter().enumerate() {
            if !matches!(arg, Expr::Closure(_))
                || !callbacks.is_some_and(|indices| indices.contains(&i))
            {
                self.visit_expr(arg);
            }
        }
    }
}

pub fn reads(tree: &Tree) -> (BTreeSet<Read>, Vec<Finding>) {
    let (sources, errors) = sources(tree);
    (analyze(&sources), errors)
}

fn analyze(sources: &[Source]) -> BTreeSet<Read> {
    let mut symbols = Symbols::default();
    for source in sources {
        symbols.modules.insert(source.module.clone());
        Gather {
            symbols: &mut symbols,
            module: source.module.clone(),
        }
        .visit_file(&source.file);
    }
    symbols.prepare();
    loop {
        let mut found = BTreeMap::new();
        for source in sources {
            let mut helpers = Helpers {
                symbols: &symbols,
                module: source.module.clone(),
                found: BTreeMap::new(),
            };
            helpers.visit_file(&source.file);
            found.extend(helpers.found);
        }
        if found == symbols.helpers {
            break;
        }
        symbols.helpers = found;
    }
    let mut callbacks = BTreeMap::new();
    for source in sources {
        let mut gather = Callbacks {
            symbols: &symbols,
            module: source.module.clone(),
            found: BTreeMap::new(),
        };
        gather.visit_file(&source.file);
        callbacks.extend(gather.found);
    }
    symbols.callbacks = callbacks;
    let mut reads = BTreeSet::new();
    for source in sources {
        let mut walker = Walker {
            symbols: &symbols,
            source,
            module: source.module.clone(),
            api: source.module.clone(),
            locals: BTreeMap::new(),
            calls: BTreeMap::new(),
            reads: BTreeSet::new(),
        };
        walker.visit_file(&source.file);
        reads.extend(walker.reads);
    }
    reads
}

#[cfg(test)]
pub(super) fn from_code(code: &str) -> BTreeSet<Read> {
    analyze(&[Source {
        path: "src/lib.rs".into(),
        owner: ".".into(),
        module: "example".into(),
        file: syn::parse_file(code).unwrap(),
    }])
}
