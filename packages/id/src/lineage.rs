//! An execution's parent: the execution that started it, a field beside
//! it, never part of its ID, so nesting one changes no ID.

use serde::{Deserialize, Serialize};

use crate::limits::limits;
use crate::{Execution, NotOne};

/// An execution with the execution that started it, if one did, and how
/// many parents are above it, at most `limits.toml`'s `parent_depth`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Written", into = "Written")]
pub struct Lineage {
    execution: Execution,
    parent: Option<Execution>,
    depth: usize,
}

impl Lineage {
    /// An execution nothing started: a start of yours.
    pub fn root(execution: Execution) -> Self {
        Self {
            execution,
            parent: None,
            depth: 0,
        }
    }

    /// `execution`, started by this one: refused when it is this one, or
    /// when it would be deeper than `parent_depth`.
    pub fn child(&self, execution: Execution) -> Result<Self, NotOne> {
        if execution == self.execution || Some(&execution) == self.parent.as_ref() {
            return Err(NotOne("an execution never starts itself".into()));
        }
        let depth = self.depth + 1;
        if depth > limits().parent_depth {
            return Err(NotOne(format!(
                "an execution has at most {} parents above it",
                limits().parent_depth
            )));
        }
        Ok(Self {
            execution,
            parent: Some(self.execution.clone()),
            depth,
        })
    }

    pub fn execution(&self) -> &Execution {
        &self.execution
    }

    /// The execution that started it, if one did.
    pub fn parent(&self) -> Option<&Execution> {
        self.parent.as_ref()
    }

    /// How many parents are above it: 0 for a root.
    pub fn depth(&self) -> usize {
        self.depth
    }
}

/// A lineage as it's written, checked as it's read.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Written {
    execution: Execution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent: Option<Execution>,
    #[serde(default)]
    depth: usize,
}

impl TryFrom<Written> for Lineage {
    type Error = NotOne;
    fn try_from(written: Written) -> Result<Self, NotOne> {
        let rooted = written.parent.is_none() == (written.depth == 0);
        if !rooted || written.depth > limits().parent_depth {
            return Err(NotOne(format!(
                "an execution with a parent is 1 to {} deep, one without is 0",
                limits().parent_depth
            )));
        }
        if written.parent.as_ref() == Some(&written.execution) {
            return Err(NotOne("an execution never starts itself".into()));
        }
        Ok(Self {
            execution: written.execution,
            parent: written.parent,
            depth: written.depth,
        })
    }
}

impl From<Lineage> for Written {
    fn from(lineage: Lineage) -> Self {
        Self {
            execution: lineage.execution,
            parent: lineage.parent,
            depth: lineage.depth,
        }
    }
}
