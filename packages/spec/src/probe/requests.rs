//! Observed requests are traversal facts, never proof of input refusal.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequestKind {
    Boolean,
    Integer,
    Number,
    String,
    Array,
    Object,
    Enum,
    Opaque,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Request {
    pub kind: RequestKind,
    pub method: String,
    pub enum_route: Vec<String>,
    pub opaque_ancestor: bool,
    pub containers: BTreeMap<String, RequestKind>,
    pub some: BTreeSet<String>,
}
#[derive(Clone, Default)]
pub(super) struct Trace {
    request: Rc<RefCell<Option<(RequestKind, String)>>>,
    pub(super) enums: Rc<RefCell<Vec<String>>>,
    pub(super) opaque: Rc<Cell<bool>>,
    path: Vec<String>,
    containers: Rc<RefCell<BTreeMap<String, RequestKind>>>,
    some: Rc<RefCell<BTreeSet<String>>>,
}
impl Trace {
    pub(super) fn at(path: Vec<String>) -> Self {
        Self {
            path,
            ..Self::default()
        }
    }
    fn prefix(&self, remaining: usize) -> String {
        self.path[..self.path.len().saturating_sub(remaining)].join(".")
    }
    pub(super) fn some(&self, remaining: usize) {
        self.some.borrow_mut().insert(self.prefix(remaining));
    }
    fn context(&self, remaining: usize, kind: RequestKind) {
        if matches!(kind, RequestKind::Object | RequestKind::Array) {
            self.containers
                .borrow_mut()
                .entry(self.prefix(remaining))
                .or_insert(kind);
        }
    }
    pub(super) fn first(&self, kind: RequestKind, method: &str) {
        let mut request = self.request.borrow_mut();
        if request.is_none() {
            *request = Some((kind, method.into()));
        }
    }
    pub(super) fn get(&self) -> Option<Request> {
        self.request
            .borrow()
            .as_ref()
            .map(|(kind, method)| Request {
                kind: *kind,
                method: method.clone(),
                enum_route: self.enums.borrow().clone(),
                opaque_ancestor: self.opaque.get(),
                containers: self.containers.borrow().clone(),
                some: self.some.borrow().clone(),
            })
    }
}
impl Probe<'_> {
    pub(super) fn requested(&self, kind: RequestKind, method: &str) {
        self.trace.context(self.remaining.len(), kind);
        if self.remaining.is_empty() {
            self.trace.first(kind, method);
        }
    }
}
