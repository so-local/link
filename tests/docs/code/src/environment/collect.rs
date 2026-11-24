//! Collect reader functions, aliases and callback boundaries from Rust syntax.

use super::resolve::Symbols;
use super::scan::path;
use std::collections::{BTreeMap, BTreeSet};
use syn::visit::{self, Visit};
use syn::{Expr, Item, Pat, UseTree};

pub(super) use crate::syntax::test_only as tested;

fn imports(tree: &UseTree, prefix: &str, out: &mut BTreeMap<String, String>) {
    match tree {
        UseTree::Path(p) => imports(&p.tree, &format!("{prefix}{}::", p.ident), out),
        UseTree::Name(n) if n.ident == "self" => {
            let full = prefix.trim_end_matches("::");
            out.insert(full.rsplit("::").next().unwrap_or(full).into(), full.into());
        }
        UseTree::Name(n) => {
            out.insert(n.ident.to_string(), format!("{prefix}{}", n.ident));
        }
        UseTree::Rename(n) => {
            let full = if n.ident == "self" {
                prefix.trim_end_matches("::").to_owned()
            } else {
                format!("{prefix}{}", n.ident)
            };
            out.insert(n.rename.to_string(), full);
        }
        UseTree::Group(g) => {
            for tree in &g.items {
                imports(tree, prefix, out);
            }
        }
        UseTree::Glob(_) => {}
    }
}

pub(super) struct Gather<'a> {
    pub(super) symbols: &'a mut Symbols,
    pub(super) module: String,
}
impl<'ast> Visit<'ast> for Gather<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        let attrs = match item {
            Item::Fn(i) => &i.attrs,
            Item::Mod(i) => &i.attrs,
            Item::Const(i) => &i.attrs,
            Item::Use(i) => &i.attrs,
            _ => return visit::visit_item(self, item),
        };
        if tested(attrs) {
            return;
        }
        visit::visit_item(self, item)
    }
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let old = self.module.clone();
        self.module += &format!("::{}", item.ident);
        self.symbols.modules.insert(self.module.clone());
        visit::visit_item_mod(self, item);
        self.module = old;
    }
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.symbols
            .functions
            .insert(format!("{}::{}", self.module, item.sig.ident));
        visit::visit_item_fn(self, item);
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if !tested(&item.attrs) {
            self.symbols
                .functions
                .insert(format!("{}::{}", self.module, item.sig.ident));
            visit::visit_impl_item_fn(self, item);
        }
    }
    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        self.symbols.constants.insert(
            format!("{}::{}", self.module, item.ident),
            (*item.expr).clone(),
        );
    }
    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        self.symbols.constants.insert(
            format!("{}::{}", self.module, item.ident),
            (*item.expr).clone(),
        );
    }
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        imports(
            &item.tree,
            "",
            self.symbols.aliases.entry(self.module.clone()).or_default(),
        );
    }
}

pub(super) fn parameters<'a>(patterns: impl Iterator<Item = &'a Pat>) -> BTreeMap<String, usize> {
    patterns
        .enumerate()
        .filter_map(|(i, p)| {
            let p = if let Pat::Type(p) = p {
                p.pat.as_ref()
            } else {
                p
            };
            if let Pat::Ident(p) = p {
                Some((p.ident.to_string(), i))
            } else {
                None
            }
        })
        .collect()
}
pub(super) struct Helper<'a> {
    pub(super) symbols: &'a Symbols,
    pub(super) module: &'a str,
    pub(super) parameters: BTreeMap<String, usize>,
    pub(super) found: BTreeSet<usize>,
}
impl<'ast> Visit<'ast> for Helper<'_> {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Expr::Path(p) = call.func.as_ref() {
            for index in self.symbols.reader(self.module, &path(&p.path)) {
                if let Some(arg) = call.args.iter().nth(index) {
                    let arg = if let Expr::Reference(r) = arg {
                        r.expr.as_ref()
                    } else {
                        arg
                    };
                    if let Expr::Path(p) = arg
                        && let Some(index) = self.parameters.get(&path(&p.path))
                    {
                        self.found.insert(*index);
                    }
                }
            }
        }
        visit::visit_expr_call(self, call)
    }
}
pub(super) struct Helpers<'a> {
    pub(super) symbols: &'a Symbols,
    pub(super) module: String,
    pub(super) found: BTreeMap<String, BTreeSet<usize>>,
}
impl<'ast> Visit<'ast> for Helpers<'_> {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if tested(&item.attrs) {
            return;
        }
        let mut helper = Helper {
            symbols: self.symbols,
            module: &self.module,
            parameters: parameters(item.sig.inputs.iter().filter_map(|p| {
                if let syn::FnArg::Typed(p) = p {
                    Some(p.pat.as_ref())
                } else {
                    None
                }
            })),
            found: BTreeSet::new(),
        };
        helper.visit_block(&item.block);
        if !helper.found.is_empty() {
            self.found
                .insert(format!("{}::{}", self.module, item.sig.ident), helper.found);
        }
    }
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if tested(&item.attrs) {
            return;
        }
        let old = self.module.clone();
        self.module += &format!("::{}", item.ident);
        visit::visit_item_mod(self, item);
        self.module = old;
    }
}

pub(super) struct Callbacks<'a> {
    pub(super) symbols: &'a Symbols,
    pub(super) module: String,
    pub(super) found: BTreeMap<String, BTreeSet<usize>>,
}
impl<'ast> Visit<'ast> for Callbacks<'_> {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if !tested(&item.attrs) {
            visit::visit_item_fn(self, item);
        }
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if !tested(&item.attrs) {
            visit::visit_impl_item_fn(self, item);
        }
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if tested(&item.attrs) {
            return;
        }
        let old = self.module.clone();
        self.module += &format!("::{}", item.ident);
        visit::visit_item_mod(self, item);
        self.module = old;
    }
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Expr::Path(function) = call.func.as_ref() {
            for (i, argument) in call.args.iter().enumerate() {
                let reads = match argument {
                    Expr::Closure(closure) => {
                        let mut helper = Helper {
                            symbols: self.symbols,
                            module: &self.module,
                            parameters: parameters(closure.inputs.iter()),
                            found: BTreeSet::new(),
                        };
                        helper.visit_expr(&closure.body);
                        !helper.found.is_empty()
                    }
                    Expr::Path(function) => !self
                        .symbols
                        .reader(&self.module, &path(&function.path))
                        .is_empty(),
                    _ => false,
                };
                if reads
                    && self.symbols.functions.contains(&format!(
                        "{}::{}",
                        self.module,
                        function.path.segments.last().unwrap().ident
                    ))
                {
                    self.found
                        .entry(format!(
                            "{}::{}",
                            self.module,
                            function.path.segments.last().unwrap().ident
                        ))
                        .or_default()
                        .insert(i);
                }
            }
        }
        visit::visit_expr_call(self, call)
    }
}
