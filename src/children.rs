use crate::{node::NodeId, ui::Element};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    fmt,
    rc::{Rc, Weak},
};

/// Errors are reported on a command receipt, and logged when a commit rejects one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeError {
    NotMounted,
    SceneDropped,
    ForeignScene,
    StaleNode,
    WrongContainer,
    DuplicateKey(String),
    InvalidOrder,
    InvalidMove,
}

impl fmt::Display for TreeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for TreeError {}

type Outcome = Result<Option<NodeId>, TreeError>;

/// `None` means queued; `Some(Ok(...))` means committed. Inserts return the new ID.
#[derive(Clone, Default)]
pub struct CommandReceipt(Rc<RefCell<Option<Outcome>>>);

impl CommandReceipt {
    pub fn result(&self) -> Option<Outcome> {
        self.0.borrow().clone()
    }
    pub(crate) fn finish(&self, outcome: Outcome) {
        if let Err(error) = &outcome {
            eprintln!("Chronobreak tree command rejected: {error}");
        }
        *self.0.borrow_mut() = Some(outcome);
    }
}

pub(crate) enum Action {
    Insert(String, Box<Element>),
    Remove(NodeId),
    MoveBefore(NodeId, Option<NodeId>),
    Reorder(Vec<NodeId>),
}

pub(crate) struct Command {
    pub target: NodeId,
    pub action: Action,
    pub receipt: CommandReceipt,
}

pub(crate) type Commands = RefCell<VecDeque<Command>>;

#[derive(Default)]
pub(crate) struct Container {
    pub id: Option<NodeId>,
    pub keys: BTreeMap<String, NodeId>,
    pub queue: Weak<Commands>,
}

/// UI-thread command handle for one keyed container, never a mutable scene reference.
#[derive(Clone, Default)]
pub struct Children(pub(crate) Rc<RefCell<Container>>);

impl Children {
    pub fn node(&self, key: &str) -> Option<NodeId> {
        self.0.borrow().keys.get(key).copied()
    }
    pub fn id(&self) -> Option<NodeId> {
        self.0.borrow().id
    }

    /// Append a fresh subtree. Keys are unique only inside this container.
    pub fn insert(&self, key: impl Into<String>, element: Element) -> CommandReceipt {
        self.submit(Action::Insert(key.into(), Box::new(element)))
    }

    pub fn remove(&self, node: NodeId) -> CommandReceipt {
        self.submit(Action::Remove(node))
    }

    /// Move a direct child before another; `None` appends. Reparenting is not supported.
    pub fn move_before(&self, node: NodeId, before: Option<NodeId>) -> CommandReceipt {
        self.submit(Action::MoveBefore(node, before))
    }

    /// Supply an exact permutation of all current direct children.
    pub fn reorder(&self, order: &[NodeId]) -> CommandReceipt {
        self.submit(Action::Reorder(order.to_vec()))
    }

    fn submit(&self, action: Action) -> CommandReceipt {
        let receipt = CommandReceipt::default();
        let container = self.0.borrow();
        match (container.id, container.queue.upgrade()) {
            (Some(target), Some(queue)) => queue.borrow_mut().push_back(Command {
                target,
                action,
                receipt: receipt.clone(),
            }),
            (None, _) => receipt.finish(Err(TreeError::NotMounted)),
            (_, None) => receipt.finish(Err(TreeError::SceneDropped)),
        }
        receipt
    }
}
