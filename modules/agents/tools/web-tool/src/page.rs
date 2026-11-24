//! A page's HTML as a small tree of the tool's own, parsed by Servo's
//! `html5ever` as a browser parses it.

use std::borrow::Cow;
use std::cell::RefCell;

use html5ever::interface::{ElemName, ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{Attribute, LocalName, Namespace, QualName};

/// A parsed page: its nodes, the document first. Held in one list, so
/// however deep a page nests, dropping it never recurses.
pub(super) struct Page {
    pub nodes: Vec<Node>,
}

/// The document's node in [`Page::nodes`].
pub(super) const DOCUMENT: usize = 0;

pub(super) struct Node {
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub kind: Kind,
}

pub(super) enum Kind {
    /// The document, or a template's contents.
    Document,
    Element {
        name: QualName,
        attrs: Vec<Attribute>,
        template: Option<usize>,
    },
    Text(String),
    /// A comment or a processing instruction: never written.
    Other,
}

impl Page {
    /// `html` parsed as a browser parses it, its errors recovered from.
    pub fn parse(html: &str) -> Page {
        html5ever::parse_document(Sink::default(), Default::default()).one(html)
    }

    /// Node `id`'s local name, if it's an element.
    pub fn name(&self, id: usize) -> Option<&str> {
        match &self.nodes[id].kind {
            Kind::Element { name, .. } => Some(&name.local),
            _ => None,
        }
    }

    /// Element `id`'s attribute `name`.
    pub fn attr(&self, id: usize, name: &str) -> Option<&str> {
        match &self.nodes[id].kind {
            Kind::Element { attrs, .. } => attrs
                .iter()
                .find(|a| &*a.name.local == name)
                .map(|a| &*a.value),
            _ => None,
        }
    }

    /// Whether element `id` is hidden from a reader, as a browser hides it:
    /// `hidden`, `aria-hidden="true"`, or an inline style of `display:
    /// none` or `visibility: hidden`. A page's hidden text is never read
    /// to the model: it's where instructions aimed at an agent hide.
    pub fn hidden(&self, id: usize) -> bool {
        if self.attr(id, "hidden").is_some()
            || self
                .attr(id, "aria-hidden")
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("true"))
        {
            return true;
        }
        let Some(style) = self.attr(id, "style") else {
            return false;
        };
        let style: String = style
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        style.split(';').any(|rule| {
            let rule = rule.trim_end_matches("!important");
            rule == "display:none" || rule == "visibility:hidden"
        })
    }

    /// What the page's links are made whole against: its first
    /// `<base href>`, itself made whole against `url`, as a browser takes
    /// it, where that's the web's; else `url`.
    pub fn base(&self, url: &reqwest::Url) -> reqwest::Url {
        (0..self.nodes.len())
            .filter(|&id| self.name(id) == Some("base"))
            .find_map(|id| self.attr(id, "href"))
            .and_then(|href| url.join(href.trim()).ok())
            .filter(|base| matches!(base.scheme(), "http" | "https"))
            .unwrap_or_else(|| url.clone())
    }

    /// How deep the page nests, walked without recursion, stopping once
    /// it's past `most`.
    pub fn depth(&self, most: usize) -> usize {
        let mut deepest = 0;
        let mut stack = vec![(DOCUMENT, 0)];
        while let Some((id, at)) = stack.pop() {
            deepest = deepest.max(at);
            if deepest > most {
                break;
            }
            stack.extend(self.nodes[id].children.iter().map(|c| (*c, at + 1)));
        }
        deepest
    }
}

/// What `html5ever` builds a [`Page`] through.
struct Sink {
    nodes: RefCell<Vec<Node>>,
}

impl Default for Sink {
    fn default() -> Self {
        let document = Node {
            parent: None,
            children: Vec::new(),
            kind: Kind::Document,
        };
        Sink {
            nodes: RefCell::new(vec![document]),
        }
    }
}

impl Sink {
    fn add(&self, kind: Kind) -> usize {
        let mut nodes = self.nodes.borrow_mut();
        nodes.push(Node {
            parent: None,
            children: Vec::new(),
            kind,
        });
        nodes.len() - 1
    }

    /// Puts `child`, which has no parent, in `parent` at `at`, text
    /// joining the text before it.
    fn insert(&self, parent: usize, at: usize, child: NodeOrText<usize>) {
        let child = match child {
            NodeOrText::AppendNode(child) => child,
            NodeOrText::AppendText(text) => {
                let mut nodes = self.nodes.borrow_mut();
                if let Some(before) = at.checked_sub(1).map(|i| nodes[parent].children[i])
                    && let Kind::Text(joined) = &mut nodes[before].kind
                {
                    joined.push_str(&text);
                    return;
                }
                drop(nodes);
                self.add(Kind::Text(text.to_string()))
            }
        };
        let mut nodes = self.nodes.borrow_mut();
        nodes[child].parent = Some(parent);
        nodes[parent].children.insert(at, child);
    }
}

/// An element's name, as `html5ever` asks for it.
#[derive(Debug)]
struct Name(QualName);

impl ElemName for Name {
    fn ns(&self) -> &Namespace {
        &self.0.ns
    }

    fn local_name(&self) -> &LocalName {
        &self.0.local
    }
}

impl TreeSink for Sink {
    type Handle = usize;
    type Output = Page;
    type ElemName<'a> = Name;

    fn finish(self) -> Page {
        Page {
            nodes: self.nodes.into_inner(),
        }
    }

    fn parse_error(&self, _: Cow<'static, str>) {}

    fn get_document(&self) -> usize {
        DOCUMENT
    }

    fn elem_name(&self, target: &usize) -> Name {
        match &self.nodes.borrow()[*target].kind {
            Kind::Element { name, .. } => Name(name.clone()),
            _ => unreachable!("html5ever asks only an element's name"),
        }
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> usize {
        let template = flags.template.then(|| self.add(Kind::Document));
        self.add(Kind::Element {
            name,
            attrs,
            template,
        })
    }

    fn create_comment(&self, _: StrTendril) -> usize {
        self.add(Kind::Other)
    }

    fn create_pi(&self, _: StrTendril, _: StrTendril) -> usize {
        self.add(Kind::Other)
    }

    fn append(&self, parent: &usize, child: NodeOrText<usize>) {
        let at = self.nodes.borrow()[*parent].children.len();
        self.insert(*parent, at, child);
    }

    fn append_based_on_parent_node(&self, element: &usize, prev: &usize, child: NodeOrText<usize>) {
        if self.nodes.borrow()[*element].parent.is_some() {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev, child);
        }
    }

    fn append_doctype_to_document(&self, _: StrTendril, _: StrTendril, _: StrTendril) {}

    fn get_template_contents(&self, target: &usize) -> usize {
        match &self.nodes.borrow()[*target].kind {
            Kind::Element {
                template: Some(contents),
                ..
            } => *contents,
            _ => unreachable!("html5ever asks only a template's contents"),
        }
    }

    fn same_node(&self, x: &usize, y: &usize) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, _: QuirksMode) {}

    fn append_before_sibling(&self, sibling: &usize, child: NodeOrText<usize>) {
        if let NodeOrText::AppendNode(child) = &child {
            self.remove_from_parent(child);
        }
        let nodes = self.nodes.borrow();
        let Some(parent) = nodes[*sibling].parent else {
            return;
        };
        let at = nodes[parent].children.iter().position(|c| c == sibling);
        drop(nodes);
        if let Some(at) = at {
            self.insert(parent, at, child);
        }
    }

    fn add_attrs_if_missing(&self, target: &usize, added: Vec<Attribute>) {
        if let Kind::Element { attrs, .. } = &mut self.nodes.borrow_mut()[*target].kind {
            for attr in added {
                if !attrs.iter().any(|a| a.name == attr.name) {
                    attrs.push(attr);
                }
            }
        }
    }

    fn remove_from_parent(&self, target: &usize) {
        let mut nodes = self.nodes.borrow_mut();
        if let Some(parent) = nodes[*target].parent.take() {
            nodes[parent].children.retain(|c| c != target);
        }
    }

    fn reparent_children(&self, node: &usize, new_parent: &usize) {
        let mut nodes = self.nodes.borrow_mut();
        let moved = std::mem::take(&mut nodes[*node].children);
        for child in &moved {
            nodes[*child].parent = Some(*new_parent);
        }
        nodes[*new_parent].children.extend(moved);
    }
}
