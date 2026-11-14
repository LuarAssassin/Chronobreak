//! 活的 UI 提交边界，不是 OS 无障碍适配器或资格声明。
use crate::{NodeId, ui::Bounds};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Group,
    Label,
    Button,
    TextInput,
    Checkbox,
    ScrollView,
    TreeItem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Activate,
    Focus,
    SetValue,
    ScrollIntoView,
    Expand,
    Collapse,
}

#[derive(Clone, PartialEq)]
pub struct SemanticNode {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub role: Role,
    pub name: String,
    pub value: Option<String>,
    pub disabled: bool,
    pub focusable: bool,
    pub readonly: bool,
    pub focused: bool,
    pub selected: Option<bool>,
    pub expanded: Option<bool>,
    pub invalid: bool,
    pub bounds: Bounds,
    pub actions: Vec<ActionKind>,
}

#[derive(Clone, Default, PartialEq)]
pub struct SemanticSnapshot {
    pub version: u64,
    pub nodes: Vec<SemanticNode>,
}

#[derive(Clone, Default)]
pub struct SemanticDelta {
    pub version: u64,
    pub changed: Vec<SemanticNode>,
    pub removed: Vec<NodeId>,
}

#[derive(Clone)]
pub enum SemanticAction {
    Activate,
    Focus,
    SetValue(String),
    ScrollIntoView,
    Expand,
    Collapse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionResult {
    Applied,
    Unavailable,
}

#[derive(Clone)]
pub struct ActionReceipt(Rc<RefCell<Option<ActionResult>>>);

impl ActionReceipt {
    pub fn result(&self) -> Option<ActionResult> {
        *self.0.borrow()
    }

    pub(crate) fn finish(&self, result: ActionResult) {
        *self.0.borrow_mut() = Some(result);
    }
}

pub(crate) type Request = (NodeId, SemanticAction, ActionReceipt);

pub(crate) const MAX_PENDING_ACTIONS: usize = 1024;

#[derive(Default)]
struct Boundary {
    wake: Option<std::sync::Weak<crate::messages::Wake>>,
    snapshot: SemanticSnapshot,
    consumed: SemanticSnapshot,
    requests: VecDeque<Request>,
}

/// UI 线程的生产/消费边界。读取包含真实提交文字；框架日志不输出这些数据。
/// delta 是相对上次 take_delta 的合并变化，不要求消费者逐帧轮询。
#[derive(Clone, Default)]
pub struct Semantics(Rc<RefCell<Boundary>>);

impl Semantics {
    pub fn snapshot(&self) -> SemanticSnapshot {
        self.0.borrow().snapshot.clone()
    }

    pub fn take_delta(&self) -> SemanticDelta {
        let mut boundary = self.0.borrow_mut();
        let delta = SemanticDelta {
            version: boundary.snapshot.version,
            changed: boundary
                .snapshot
                .nodes
                .iter()
                .filter(|node| {
                    boundary.consumed.nodes.iter().find(|old| old.id == node.id) != Some(*node)
                })
                .cloned()
                .collect(),
            removed: boundary
                .consumed
                .nodes
                .iter()
                .filter(|old| !boundary.snapshot.nodes.iter().any(|node| node.id == old.id))
                .map(|old| old.id)
                .collect(),
        };
        boundary.consumed = boundary.snapshot.clone();
        delta
    }

    pub fn perform(&self, id: NodeId, action: SemanticAction) -> ActionReceipt {
        let receipt = ActionReceipt(Rc::default());
        let closed = self
            .0
            .borrow()
            .wake
            .as_ref()
            .is_some_and(|wake| wake.upgrade().is_none());
        if closed {
            receipt.finish(ActionResult::Unavailable);
            return receipt;
        }
        {
            let mut boundary = self.0.borrow_mut();
            if boundary.requests.len() >= MAX_PENDING_ACTIONS {
                receipt.finish(ActionResult::Unavailable);
                return receipt;
            }
            boundary.requests.push_back((id, action, receipt.clone()));
        }
        if let Some(wake) = self
            .0
            .borrow()
            .wake
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
        {
            wake.notify();
        }
        receipt
    }

    pub(crate) fn requests(&self) -> VecDeque<Request> {
        std::mem::take(&mut self.0.borrow_mut().requests)
    }

    pub(crate) fn attach(&self, wake: &std::sync::Arc<crate::messages::Wake>) {
        self.0.borrow_mut().wake = Some(std::sync::Arc::downgrade(wake));
    }

    pub(crate) fn publish(&self, nodes: Vec<SemanticNode>) {
        let mut boundary = self.0.borrow_mut();
        if boundary.snapshot.nodes == nodes {
            return;
        }
        let version = boundary.snapshot.version + 1;
        boundary.snapshot = SemanticSnapshot { version, nodes };
    }
}
