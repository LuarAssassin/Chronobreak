use super::*;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScrollError {
    NotMounted,
    SceneDropped,
    StaleViewport,
    ForeignScene,
    StaleTarget,
    OutsideScope,
    Hidden,
    NonFiniteOffset,
}

impl fmt::Display for ScrollError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for ScrollError {}

#[derive(Clone, Default)]
pub struct ScrollReceipt(Rc<RefCell<Option<Result<(), ScrollError>>>>);

impl ScrollReceipt {
    pub fn result(&self) -> Option<Result<(), ScrollError>> {
        self.0.borrow().clone()
    }

    fn finish(&self, result: Result<(), ScrollError>) {
        *self.0.borrow_mut() = Some(result);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollSnapshot {
    pub offset: f32,
    pub max_offset: f32,
    pub viewport_height: f32,
}

#[derive(Default)]
struct HandleState {
    id: Option<NodeId>,
    mounted: bool,
    closed: bool,
    queue: Weak<RefCell<VecDeque<Command>>>,
    snapshot: ScrollSnapshot,
}

/// UI 线程句柄；命令在最终内容测量后提交，结果由 receipt 返回。
#[derive(Clone, Default)]
pub struct ScrollHandle(Rc<RefCell<HandleState>>);

impl ScrollHandle {
    pub fn id(&self) -> Option<NodeId> {
        self.0.borrow().id
    }

    pub fn snapshot(&self) -> ScrollSnapshot {
        self.0.borrow().snapshot
    }

    pub fn scroll_to(&self, offset: f32) -> ScrollReceipt {
        self.submit(Action::To(offset))
    }

    pub fn scroll_by(&self, delta: f32) -> ScrollReceipt {
        self.submit(Action::By(delta))
    }

    /// 最近边缘对齐；仅允许本视口的有效可见后代，连同其嵌套视口一起揭示。
    pub fn scroll_into_view(&self, target: NodeId) -> ScrollReceipt {
        self.submit(Action::Reveal(target))
    }

    fn submit(&self, action: Action) -> ScrollReceipt {
        let receipt = ScrollReceipt::default();
        let state = self.0.borrow();
        let error = if state.closed {
            Some(ScrollError::SceneDropped)
        } else if state.id.is_none() {
            Some(ScrollError::NotMounted)
        } else if !state.mounted {
            Some(ScrollError::StaleViewport)
        } else {
            None
        };
        if let Some(error) = error {
            receipt.finish(Err(error));
        } else if let Some(queue) = state.queue.upgrade() {
            queue.borrow_mut().push_back(Command {
                viewport: state.id.unwrap(),
                action,
                receipt: receipt.clone(),
            });
        } else {
            receipt.finish(Err(ScrollError::SceneDropped));
        }
        receipt
    }
}

#[derive(Clone, Copy)]
enum Action {
    To(f32),
    By(f32),
    Reveal(NodeId),
}

pub(super) struct Command {
    viewport: NodeId,
    action: Action,
    receipt: ScrollReceipt,
}

pub(super) struct Viewport {
    handle: ScrollHandle,
    pub offset: f32,
    max: f32,
    height: f32,
}

impl Viewport {
    pub fn new(handle: ScrollHandle, queue: &Rc<RefCell<VecDeque<Command>>>) -> Self {
        handle.0.borrow_mut().queue = Rc::downgrade(queue);
        Self {
            handle,
            offset: 0.0,
            max: 0.0,
            height: 0.0,
        }
    }

    pub fn attach(&self, id: NodeId) {
        let mut state = self.handle.0.borrow_mut();
        state.id = Some(id);
        state.mounted = true;
    }

    pub fn detach(&self) {
        let mut state = self.handle.0.borrow_mut();
        state.mounted = false;
        state.snapshot = ScrollSnapshot::default();
    }

    fn publish(&self) {
        self.handle.0.borrow_mut().snapshot = ScrollSnapshot {
            offset: self.offset,
            max_offset: self.max,
            viewport_height: self.height,
        };
        if std::env::var_os("CHRONOBREAK_TRACE").is_some() {
            eprintln!(
                "scroll viewport={:?} offset={} max={} height={}",
                self.handle.id(),
                self.offset,
                self.max,
                self.height
            );
        }
    }
}

impl Scene {
    pub(super) fn close_scrolls(&mut self) {
        for node in self.nodes.iter() {
            if let Some(scroll) = &node.scroll {
                scroll.handle.0.borrow_mut().closed = true;
                scroll.detach();
            }
        }
        for command in self.scroll_commands.borrow_mut().drain(..) {
            command.receipt.finish(Err(ScrollError::SceneDropped));
        }
    }

    pub(super) fn clamp_scrolls(&mut self) {
        for id in self.scroll_ids() {
            let node = &self.nodes[id];
            let height = self.tree.layout(node.layout_id).unwrap().size.height;
            let content = self
                .tree
                .layout(self.nodes[node.children[0]].layout_id)
                .unwrap();
            let max = if self.is_visible(id) {
                (content.location.y + content.size.height - height).max(0.0)
            } else {
                0.0
            };
            let scroll = self.nodes[id].scroll.as_mut().unwrap();
            scroll.height = height;
            scroll.max = max;
            scroll.offset = scroll.offset.clamp(0.0, max);
            scroll.publish();
        }
    }

    fn scroll_ids(&self) -> Vec<NodeId> {
        fn collect(scene: &Scene, id: NodeId, ids: &mut Vec<NodeId>) {
            if scene.nodes[id].scroll.is_some() {
                ids.push(id);
            }
            for child in &scene.nodes[id].children {
                collect(scene, *child, ids);
            }
        }
        let mut ids = Vec::new();
        if let Some(root) = self.root {
            collect(self, root, &mut ids);
        }
        ids
    }

    fn set_offset(&mut self, id: NodeId, offset: f32) -> bool {
        let scroll = self.nodes[id].scroll.as_mut().unwrap();
        let next = offset.clamp(0.0, scroll.max);
        if next == scroll.offset {
            return false;
        }
        scroll.offset = next;
        scroll.publish();
        true
    }

    fn reveal_target(&mut self, target: NodeId, scope: Option<NodeId>) -> bool {
        let mut changed = false;
        let layout = self.tree.layout(self.nodes[target].layout_id).unwrap();
        let mut top = layout.location.y;
        let mut height = layout.size.height;
        let mut parent = self.nodes[target].parent;
        while let Some(id) = parent {
            if height <= 0.0 {
                break;
            }
            if let Some(scroll) = &self.nodes[id].scroll {
                let bottom = top + height;
                let offset = scroll.offset;
                let extent = scroll.height;
                let next = if top < offset && bottom > offset + extent {
                    offset
                } else if top < offset {
                    if height > extent {
                        bottom - extent
                    } else {
                        top
                    }
                } else if bottom > offset + extent {
                    if height > extent {
                        top
                    } else {
                        bottom - extent
                    }
                } else {
                    offset
                };
                changed |= self.set_offset(id, next);
                top -= self.nodes[id].scroll.as_ref().unwrap().offset;
            }
            let layout = self.tree.layout(self.nodes[id].layout_id).unwrap();
            let bottom = (top + height).min(layout.size.height);
            top = top.max(0.0);
            height = (bottom - top).max(0.0);
            if scope == Some(id) {
                break;
            }
            top += layout.location.y;
            parent = self.nodes[id].parent;
        }
        changed
    }

    pub(super) fn commit_scrolls(&mut self) {
        let commands = std::mem::take(&mut *self.scroll_commands.borrow_mut());
        if commands.is_empty() && self.focus_reveal.is_none() {
            return;
        }
        let previous: Vec<_> = self
            .scroll_ids()
            .into_iter()
            .map(|id| (id, self.nodes[id].scroll.as_ref().unwrap().offset))
            .collect();
        let mut changed = false;
        for command in commands {
            let result = self.apply_scroll(command.viewport, command.action);
            match result {
                Ok(moved) => {
                    changed |= moved;
                    command.receipt.finish(Ok(()));
                }
                Err(error) => command.receipt.finish(Err(error)),
            }
        }
        if let Some(target) = self.focus_reveal.take()
            && self.nodes.get(target).is_some()
            && self.is_visible(target)
        {
            changed |= self.reveal_target(target, None);
        }
        if changed
            && previous
                .iter()
                .any(|(id, offset)| self.nodes[*id].scroll.as_ref().unwrap().offset != *offset)
        {
            self.refresh_placement();
        }
    }

    fn apply_scroll(&mut self, viewport: NodeId, action: Action) -> Result<bool, ScrollError> {
        if self
            .nodes
            .get(viewport)
            .is_none_or(|node| node.scroll.is_none())
        {
            return Err(ScrollError::StaleViewport);
        }
        if !self.is_visible(viewport) {
            return Err(ScrollError::Hidden);
        }
        match action {
            Action::To(offset) | Action::By(offset) => {
                if !offset.is_finite() {
                    return Err(ScrollError::NonFiniteOffset);
                }
                let current = self.nodes[viewport].scroll.as_ref().unwrap().offset;
                let next = if matches!(action, Action::By(_)) {
                    current + offset
                } else {
                    offset
                };
                Ok(self.set_offset(viewport, next))
            }
            Action::Reveal(target) => {
                if !self.nodes.owns(target) {
                    return Err(ScrollError::ForeignScene);
                }
                if self.nodes.get(target).is_none() {
                    return Err(ScrollError::StaleTarget);
                }
                let mut ancestor = self.nodes[target].parent;
                while ancestor.is_some() && ancestor != Some(viewport) {
                    ancestor = self.nodes[ancestor.unwrap()].parent;
                }
                if ancestor.is_none() {
                    return Err(ScrollError::OutsideScope);
                }
                if !self.is_visible(target) {
                    return Err(ScrollError::Hidden);
                }
                Ok(self.reveal_target(target, Some(viewport)))
            }
        }
    }

    pub(super) fn scroll_chain(&mut self, target: NodeId, delta: f32) {
        if !delta.is_finite() {
            return;
        }
        let mut remainder = delta;
        let mut parent = Some(target);
        let mut changed = false;
        while let Some(id) = parent {
            if self.scroll_enabled(id)
                && let Some(scroll) = &self.nodes[id].scroll
            {
                let old = scroll.offset;
                let moved = self.set_offset(id, old + remainder);
                let consumed = self.nodes[id].scroll.as_ref().unwrap().offset - old;
                remainder -= consumed;
                changed |= moved;
            }
            if remainder == 0.0 {
                break;
            }
            parent = self.nodes[id].parent;
        }
        if changed {
            self.refresh_placement();
            self.reconcile_hover();
        }
    }

    fn refresh_placement(&mut self) {
        let Some((width, height)) = self.viewport else {
            return;
        };
        self.commit_version += 1;
        self.place(
            self.root.unwrap(),
            0.0,
            0.0,
            Bounds {
                x: 0.0,
                y: 0.0,
                width,
                height,
            },
            false,
        );
        self.placements += 1;
        self.geometry_version = self.commit_version;
        self.present_needed = true;
    }

    pub(super) fn paint_scroll_indicator(&self, id: NodeId, paints: &mut Vec<Paint>) {
        let node = &self.nodes[id];
        let Some(scroll) = &node.scroll else {
            return;
        };
        if scroll.max <= 0.0 || scroll.height <= 0.0 {
            return;
        }
        let thumb = scroll.height * scroll.height / (scroll.height + scroll.max);
        paints.push(Paint::Rect {
            bounds: Bounds {
                x: node.bounds.x + (node.bounds.width - 4.0).max(0.0),
                y: node.bounds.y + (scroll.height - thumb) * scroll.offset / scroll.max,
                width: node.bounds.width.min(4.0),
                height: thumb,
            },
            clip: node.clip,
            color: [0.55, 0.68, 0.80, 0.85],
        });
    }
}
