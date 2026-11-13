use crate::signal::{Signal, Subscription};
use crate::{
    children::{Action, Children, Commands, TreeError},
    messages::{MessageReceiver, Route, Wake},
    node::{Arena, NodeId},
};
use glyphon::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Wrap};
use std::{
    any::Any,
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    rc::{Rc, Weak},
    sync::Arc,
};
use taffy::{prelude::*, style::AvailableSpace};

#[path = "input.rs"]
mod input;
#[path = "scroll.rs"]
mod scroll;
pub use input::{EventContext, EventKind, EventPhase, KeyInput, UiEvent, WheelInput};
use input::{Handler, Interaction};
pub use scroll::{ScrollError, ScrollHandle, ScrollReceipt, ScrollSnapshot};

pub type Color = [f32; 4];
#[derive(Default)]
struct Update {
    text: Option<Box<dyn Any>>,
    visible: Option<bool>,
    disabled: Option<bool>,
}

type Updates = RefCell<BTreeMap<NodeId, Update>>;
type Pending = Rc<Updates>;
type TextFormatter = Box<dyn Fn(Box<dyn Any>) -> String>;
type Binding = Box<dyn FnOnce(NodeId, Weak<Updates>) -> (String, Subscription, TextFormatter)>;
type VisibilityBinding = Box<dyn FnOnce(NodeId, Weak<Updates>) -> (bool, Subscription)>;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Bounds {
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }

    pub fn intersect(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        Self {
            x,
            y,
            width: ((self.x + self.width).min(other.x + other.width) - x).max(0.0),
            height: ((self.y + self.height).min(other.y + other.height) - y).max(0.0),
        }
    }
}

pub struct Element {
    style: Style,
    text: Option<String>,
    font_size: f32,
    color: Color,
    background: Option<Color>,
    children: Vec<Element>,
    binding: Option<Binding>,
    visibility: Option<VisibilityBinding>,
    disability: Option<VisibilityBinding>,
    disabled: bool,
    focusable: Option<bool>,
    handlers: Vec<Handler>,
    click: Option<Rc<dyn Fn()>>,
    container: Option<Children>,
    messages: Vec<Box<dyn Route>>,
    scroll: Option<ScrollHandle>,
}

impl Element {
    pub fn column() -> Self {
        Self {
            style: Style {
                flex_direction: FlexDirection::Column,
                min_size: Size {
                    width: length(0.0_f32),
                    height: length(0.0_f32),
                },
                flex_shrink: 0.0,
                ..Style::default()
            },
            text: None,
            font_size: 18.0,
            color: [0.86, 0.90, 0.96, 1.0],
            background: None,
            children: Vec::new(),
            binding: None,
            visibility: None,
            disability: None,
            disabled: false,
            focusable: None,
            handlers: Vec::new(),
            click: None,
            container: None,
            messages: Vec::new(),
            scroll: None,
        }
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Self::column()
        }
    }

    /// Create an initially empty keyed container and its deferred command handle.
    pub fn keyed_column() -> (Self, Children) {
        let children = Children::default();
        (
            Self {
                container: Some(children.clone()),
                ..Self::column()
            },
            children,
        )
    }

    pub fn bound_text<T: Clone + PartialEq + 'static>(
        signal: &Signal<T>,
        format: impl Fn(T) -> String + 'static,
    ) -> Self {
        let signal = signal.clone();
        Self {
            binding: Some(Box::new(move |index, pending| {
                let text = format(signal.get());
                let subscription = signal.subscribe(move |value| {
                    if let Some(pending) = pending.upgrade() {
                        pending.borrow_mut().entry(index).or_default().text =
                            Some(Box::new(value.clone()));
                    }
                });
                let formatter: TextFormatter =
                    Box::new(move |value| format(*value.downcast::<T>().unwrap()));
                (text, subscription, formatter)
            })),
            ..Self::column()
        }
    }

    pub fn visible(mut self, signal: &Signal<bool>) -> Self {
        let signal = signal.clone();
        self.visibility = Some(Box::new(move |index, pending| {
            let visible = signal.get();
            let subscription = signal.subscribe(move |value| {
                if let Some(pending) = pending.upgrade() {
                    pending.borrow_mut().entry(index).or_default().visible = Some(*value);
                }
            });
            (visible, subscription)
        }));
        self
    }

    pub fn child(mut self, child: Self) -> Self {
        assert!(
            self.scroll.is_none(),
            "scroll_view owns exactly one content child"
        );
        self.children.push(child);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn disabled_when(mut self, signal: &Signal<bool>) -> Self {
        let signal = signal.clone();
        self.disability = Some(Box::new(move |index, pending| {
            let disabled = signal.get();
            let subscription = signal.subscribe(move |value| {
                if let Some(pending) = pending.upgrade() {
                    pending.borrow_mut().entry(index).or_default().disabled = Some(*value);
                }
            });
            (disabled, subscription)
        }));
        self
    }

    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = Some(focusable);
        self
    }

    pub fn on_event(
        mut self,
        phase: EventPhase,
        handler: impl Fn(&UiEvent, &mut EventContext) + 'static,
    ) -> Self {
        self.handlers.push((phase, Rc::new(handler)));
        self
    }

    /// 在 UI 线程的框架批次内处理有界后台数据。
    /// 已提交卸载关闭接收器；排队删除不是立即取消。
    pub fn on_message<T: Send + 'static>(
        mut self,
        receiver: MessageReceiver<T>,
        handler: impl FnMut(T) + 'static,
    ) -> Self {
        self.messages.push(receiver.bind(handler));
        self
    }

    pub fn padding(mut self, padding: f32) -> Self {
        self.style.padding = Rect::length(padding);
        self
    }

    pub fn gap(mut self, gap: f32) -> Self {
        self.style.gap = Size {
            width: length(gap),
            height: length(gap),
        };
        self
    }

    pub fn max_width(mut self, width: f32) -> Self {
        self.style.max_size.width = length(width);
        self
    }

    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        assert!(height.is_finite() && height >= 0.0);
        self.style.size.height = length(height);
        self
    }

    /// 固定逻辑高度的纵向视口；内容独立测量，不卸载被裁剪的节点。
    pub fn scroll_view(height: f32, content: Self) -> (Self, ScrollHandle) {
        let handle = ScrollHandle::default();
        let mut viewport = Self::column().height(height).child(content);
        viewport.scroll = Some(handle.clone());
        (viewport, handle)
    }

    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    pub fn on_click(mut self, click: impl Fn() + 'static) -> Self {
        self.click = Some(Rc::new(click));
        self
    }
}

pub(crate) struct Node {
    layout_id: taffy::NodeId,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    pub buffer: Option<Buffer>,
    text: Option<String>,
    pub color: Color,
    pub bounds: Bounds,
    pub clip: Bounds,
    background: Option<Color>,
    click: Option<Rc<dyn Fn()>>,
    _subscriptions: Vec<Subscription>,
    formatter: Option<TextFormatter>,
    visible: bool,
    disabled: bool,
    focusable: bool,
    handlers: Vec<Handler>,
    pub revision: usize,
    container: Option<Children>,
    scroll: Option<scroll::Viewport>,
}

#[derive(Clone, Copy)]
pub(crate) enum Paint {
    Rect {
        bounds: Bounds,
        clip: Bounds,
        color: Color,
    },
    Text(NodeId),
}

pub(crate) struct Scene {
    pub(crate) wake: Arc<Wake>,
    routes: VecDeque<(NodeId, Box<dyn Route>)>,
    pub fonts: FontSystem,
    pub nodes: Arena<Node>,
    tree: TaffyTree<NodeId>,
    root: Option<NodeId>,
    pending: Pending,
    pressed: Option<NodeId>,
    interaction: Interaction,
    commands: Rc<Commands>,
    viewport: Option<(f32, f32)>,
    dirty: Dirty,
    pub commit_version: u64,
    pub geometry_version: u64,
    pub layouts: u64,
    pub formats: u64,
    present_needed: bool,
    scroll_commands: Rc<RefCell<VecDeque<scroll::Command>>>,
    focus_reveal: Option<NodeId>,
    pub placements: u64,
}

#[derive(Debug, Default)]
struct Dirty {
    structure: bool,
    text_measurement: bool,
    visibility: bool,
    viewport: bool,
}

impl Dirty {
    fn any(&self) -> bool {
        self.structure || self.text_measurement || self.visibility || self.viewport
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        self.close_messages();
        for node in self.nodes.iter() {
            if let Some(container) = &node.container {
                container.0.borrow_mut().keys.clear();
            }
        }
    }
}

impl Scene {
    pub fn new(element: Element) -> Self {
        let mut scene = Self {
            wake: Arc::default(),
            routes: VecDeque::new(),
            fonts: FontSystem::new(),
            nodes: Arena::new(),
            tree: TaffyTree::new(),
            root: None,
            pending: Rc::default(),
            pressed: None,
            interaction: Interaction::default(),
            commands: Rc::default(),
            viewport: None,
            dirty: Dirty {
                structure: true,
                ..Dirty::default()
            },
            commit_version: 0,
            geometry_version: 0,
            layouts: 0,
            formats: 0,
            present_needed: true,
            scroll_commands: Rc::default(),
            focus_reveal: None,
            placements: 0,
        };
        scene.root = Some(scene.mount(element));
        scene
    }

    fn mount(&mut self, element: Element) -> NodeId {
        let children: Vec<_> = element
            .children
            .into_iter()
            .map(|child| self.mount(child))
            .collect();
        let child_ids: Vec<_> = children
            .iter()
            .map(|index| self.nodes[*index].layout_id)
            .collect();
        let layout_id = self
            .tree
            .new_with_children(element.style.clone(), &child_ids)
            .unwrap();
        let index = self.nodes.insert(Node {
            layout_id,
            parent: None,
            children,
            buffer: None,
            text: None,
            color: element.color,
            bounds: Bounds::default(),
            clip: Bounds::default(),
            background: element.background,
            click: element.click,
            disabled: element.disabled,
            focusable: element.focusable.unwrap_or(false),
            handlers: element.handlers,
            _subscriptions: Vec::new(),
            formatter: None,
            visible: true,
            revision: 0,
            container: element.container,
            scroll: element
                .scroll
                .map(|handle| scroll::Viewport::new(handle, &self.scroll_commands)),
        });
        if let Some(scroll) = &self.nodes[index].scroll {
            scroll.attach(index);
        }
        for child in self.nodes[index].children.clone() {
            self.nodes[child].parent = Some(index);
        }
        if let Some(handle) = &self.nodes[index].container {
            let mut container = handle.0.borrow_mut();
            container.id = Some(index);
            container.queue = Rc::downgrade(&self.commands);
        }
        let mut subscriptions = Vec::new();
        let mut text = element.text;
        let formatter = element.binding.map(|bind| {
            let (initial, subscription, formatter) = bind(index, Rc::downgrade(&self.pending));
            text = Some(initial);
            subscriptions.push(subscription);
            formatter
        });
        let visible = element.visibility.is_none_or(|bind| {
            let (visible, subscription) = bind(index, Rc::downgrade(&self.pending));
            subscriptions.push(subscription);
            visible
        });
        let disabled = element.disability.map(|bind| {
            let (disabled, subscription) = bind(index, Rc::downgrade(&self.pending));
            subscriptions.push(subscription);
            disabled
        });
        let mut style = element.style;
        if !visible {
            style.display = Display::None;
        }
        let buffer = text.as_ref().map(|text| {
            let mut buffer = Buffer::new(
                &mut self.fonts,
                Metrics::new(element.font_size, element.font_size * 1.5),
            );
            buffer.set_wrap(&mut self.fonts, Wrap::WordOrGlyph);
            buffer.set_text(
                &mut self.fonts,
                text,
                &Attrs::new().family(Family::SansSerif),
                Shaping::Advanced,
            );
            buffer
        });
        self.tree.set_style(layout_id, style).unwrap();
        if buffer.is_some() {
            self.tree.set_node_context(layout_id, Some(index)).unwrap();
        }
        let node = &mut self.nodes[index];
        node.buffer = buffer;
        node.text = text;
        node._subscriptions = subscriptions;
        node.formatter = formatter;
        node.visible = visible;
        node.disabled = disabled.unwrap_or(node.disabled);
        node.focusable = element.focusable.unwrap_or(node.click.is_some());
        for route in element.messages {
            route.attach(&self.wake);
            self.routes.push_back((index, route));
        }
        index
    }

    pub(crate) fn close_messages(&mut self) {
        self.close_scrolls();
        self.wake.close();
        self.routes.clear();
        let commands = std::mem::take(&mut *self.commands.borrow_mut());
        for command in commands {
            command.receipt.finish(Err(TreeError::SceneDropped));
        }
    }

    pub(crate) fn drain_messages(&mut self) -> usize {
        self.wake.acknowledge();
        self.sync();
        let mut count = 0;
        let mut empty = 0;
        crate::batch(|| {
            while count < 64 && empty < self.routes.len() {
                let Some((id, route)) = self.routes.pop_front() else {
                    break;
                };
                if self.nodes.get(id).is_some() && route.dispatch_one() {
                    count += 1;
                    empty = 0;
                } else {
                    empty += 1;
                }
                self.routes.push_back((id, route));
            }
        });
        self.sync();
        if self.routes.iter().any(|(_, route)| route.pending()) {
            self.wake.notify();
        }
        count
    }

    pub fn sync(&mut self) -> usize {
        let commands = std::mem::take(&mut *self.commands.borrow_mut());
        let mut count = 0;
        for command in commands {
            let before = self
                .nodes
                .get(command.target)
                .map(|node| node.children.clone());
            let result = self.apply(command.target, command.action);
            let after = self
                .nodes
                .get(command.target)
                .map(|node| node.children.clone());
            if result.is_ok() && before != after {
                count += 1;
                self.dirty.structure |= self.is_visible(command.target);
            }
            command.receipt.finish(result);
        }
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        for (index, update) in pending {
            let parent_visible = self
                .nodes
                .get(index)
                .is_some_and(|node| node.parent.is_none_or(|parent| self.is_visible(parent)));
            let Some(node) = self.nodes.get_mut(index) else {
                continue;
            };
            let mut changed = false;
            if let Some(value) = update.text {
                self.formats += 1;
                let text = node.formatter.as_ref().unwrap()(value);
                if node.text.as_ref() != Some(&text) {
                    node.buffer.as_mut().unwrap().set_text(
                        &mut self.fonts,
                        &text,
                        &Attrs::new().family(Family::SansSerif),
                        Shaping::Advanced,
                    );
                    node.text = Some(text);
                    self.dirty.text_measurement |= parent_visible && node.visible;
                    changed = true;
                }
            }
            if let Some(visible) = update.visible.filter(|visible| *visible != node.visible) {
                node.visible = visible;
                self.dirty.visibility |= parent_visible;
                changed = true;
                let mut style = self.tree.style(node.layout_id).unwrap().clone();
                style.display = if visible {
                    Display::Flex
                } else {
                    Display::None
                };
                self.tree.set_style(node.layout_id, style).unwrap();
            }
            if let Some(disabled) = update
                .disabled
                .filter(|disabled| *disabled != node.disabled)
            {
                node.disabled = disabled;
                self.present_needed |= parent_visible && node.visible;
                count += 1;
            }
            if changed {
                count += 1;
                node.revision += 1;
                self.tree.mark_dirty(node.layout_id).unwrap();
            }
        }
        self.reconcile_interaction();
        if count != 0 {
            self.commit_version += 1;
            self.present_needed |= self.dirty.any();
        }
        count
    }

    pub fn flush(&mut self, width: f32, height: f32) -> usize {
        let changed = self.sync();
        self.layout(width, height);
        self.commit_scrolls();
        self.reconcile_hover();
        changed
    }

    pub fn needs_present(&self) -> bool {
        self.present_needed
    }

    pub fn presented(&mut self) {
        self.present_needed = false;
    }

    fn is_visible(&self, index: NodeId) -> bool {
        let node = &self.nodes[index];
        node.visible && node.parent.is_none_or(|parent| self.is_visible(parent))
    }

    fn validate(&self, id: NodeId) -> Result<(), TreeError> {
        if !self.nodes.owns(id) {
            return Err(TreeError::ForeignScene);
        }
        self.nodes.get(id).ok_or(TreeError::StaleNode).map(|_| ())
    }

    fn direct_child(&self, parent: NodeId, child: NodeId) -> Result<(), TreeError> {
        self.validate(child)?;
        if self.nodes[child].parent != Some(parent) {
            return Err(TreeError::WrongContainer);
        }
        Ok(())
    }

    fn apply(&mut self, target: NodeId, action: Action) -> Result<Option<NodeId>, TreeError> {
        self.validate(target)?;
        let handle = self.nodes[target]
            .container
            .clone()
            .ok_or(TreeError::WrongContainer)?;
        match action {
            Action::Insert(key, element) => {
                if handle.0.borrow().keys.contains_key(&key) {
                    return Err(TreeError::DuplicateKey(key));
                }
                let child = self.mount(*element);
                self.nodes[child].parent = Some(target);
                self.nodes[target].children.push(child);
                handle.0.borrow_mut().keys.insert(key, child);
                self.update_children(target);
                Ok(Some(child))
            }
            Action::Remove(child) => {
                self.direct_child(target, child)?;
                self.nodes[target].children.retain(|id| *id != child);
                handle.0.borrow_mut().keys.retain(|_, id| *id != child);
                self.update_children(target);
                self.unmount(child);
                Ok(None)
            }
            Action::MoveBefore(child, before) => {
                self.direct_child(target, child)?;
                if let Some(before) = before {
                    self.direct_child(target, before)?;
                    if child == before {
                        return Err(TreeError::InvalidMove);
                    }
                }
                let children = &mut self.nodes[target].children;
                children.retain(|id| *id != child);
                let position = before
                    .and_then(|before| children.iter().position(|id| *id == before))
                    .unwrap_or(children.len());
                children.insert(position, child);
                self.update_children(target);
                Ok(None)
            }
            Action::Reorder(order) => {
                for child in &order {
                    self.direct_child(target, *child)?;
                }
                let unique: BTreeSet<_> = order.iter().copied().collect();
                if unique.len() != order.len() || order.len() != self.nodes[target].children.len() {
                    return Err(TreeError::InvalidOrder);
                }
                self.nodes[target].children = order;
                self.update_children(target);
                Ok(None)
            }
        }
    }

    fn update_children(&mut self, parent: NodeId) {
        let layouts: Vec<_> = self.nodes[parent]
            .children
            .iter()
            .map(|id| self.nodes[*id].layout_id)
            .collect();
        self.tree
            .set_children(self.nodes[parent].layout_id, &layouts)
            .unwrap();
    }

    fn unmount(&mut self, id: NodeId) {
        self.routes.retain(|(target, _)| *target != id);
        let node = self.nodes.remove(id).expect("validated subtree");
        if let Some(scroll) = &node.scroll {
            scroll.detach();
        }
        if self.pressed == Some(id) {
            self.pressed = None;
        }
        self.pending.borrow_mut().remove(&id);
        if let Some(container) = &node.container {
            container.0.borrow_mut().keys.clear();
        }
        let queued = std::mem::take(&mut *self.commands.borrow_mut());
        for command in queued {
            if command.target == id {
                command.receipt.finish(Err(TreeError::StaleNode));
            } else {
                self.commands.borrow_mut().push_back(command);
            }
        }
        for child in &node.children {
            self.unmount(*child);
        }
        self.tree.remove(node.layout_id).unwrap();
    }

    pub(crate) fn stats(&self) -> String {
        format!(
            "live={} taffy={} slots={} subscriptions={} pending={} queued={} text_buffers={}",
            self.nodes.len(),
            self.tree.total_node_count(),
            self.nodes.slot_capacity(),
            self.nodes
                .iter()
                .map(|node| node._subscriptions.len())
                .sum::<usize>(),
            self.pending.borrow().len(),
            self.commands.borrow().len(),
            self.nodes
                .iter()
                .filter(|node| node.buffer.is_some())
                .count()
        )
    }

    pub fn layout(&mut self, width: f32, height: f32) {
        if self.viewport != Some((width, height)) {
            self.dirty.viewport = true;
            self.present_needed = true;
            self.commit_version += 1;
        }
        self.viewport = Some((width, height));
        if !self.dirty.any() {
            self.geometry_version = self.commit_version;
            return;
        }
        let dirty = std::mem::take(&mut self.dirty);
        let root_id = self.nodes[self.root.unwrap()].layout_id;
        if self.tree.style(root_id).unwrap().size.width != length(width) {
            let mut style = self.tree.style(root_id).unwrap().clone();
            style.size.width = length(width);
            self.tree.set_style(root_id, style).unwrap();
        }
        let nodes = &mut self.nodes;
        let fonts = &mut self.fonts;
        self.tree
            .compute_layout_with_measure(
                root_id,
                Size {
                    width: AvailableSpace::Definite(width),
                    height: AvailableSpace::MaxContent,
                },
                |known, available, _, context, _| {
                    let Some(index) = context else {
                        return Size::ZERO;
                    };
                    let buffer = nodes[*index].buffer.as_mut().unwrap();
                    let width = known.width.or(match available.width {
                        AvailableSpace::Definite(width) => Some(width),
                        AvailableSpace::MinContent => Some(0.0),
                        AvailableSpace::MaxContent => None,
                    });
                    buffer.set_size(fonts, width, None);
                    buffer.shape_until_scroll(fonts, false);
                    let measured_width = buffer
                        .layout_runs()
                        .map(|run| run.line_w)
                        .fold(0.0, f32::max);
                    let measured_height =
                        buffer.layout_runs().count() as f32 * buffer.metrics().line_height;
                    Size {
                        width: known.width.unwrap_or(measured_width),
                        height: known.height.unwrap_or(measured_height),
                    }
                },
            )
            .unwrap();
        self.clamp_scrolls();
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
            true,
        );
        self.placements += 1;
        self.geometry_version = self.commit_version;
        self.layouts += 1;
        if std::env::var_os("CHRONOBREAK_TRACE").is_some() {
            eprintln!(
                "layout version={} layouts={} formats={} dirty={dirty:?}",
                self.geometry_version, self.layouts, self.formats
            );
        }
    }

    fn place(&mut self, index: NodeId, parent_x: f32, parent_y: f32, clip: Bounds, shape: bool) {
        let layout = self.tree.layout(self.nodes[index].layout_id).unwrap();
        let bounds = Bounds {
            x: parent_x + layout.location.x,
            y: parent_y + layout.location.y,
            width: layout.size.width,
            height: layout.size.height,
        };
        let node = &mut self.nodes[index];
        node.bounds = bounds;
        node.clip = bounds.intersect(clip);
        if shape && let Some(buffer) = &mut node.buffer {
            buffer.set_size(&mut self.fonts, Some(bounds.width), None);
            buffer.shape_until_scroll(&mut self.fonts, false);
        }
        let clip = node.clip;
        let offset = node.scroll.as_ref().map_or(0.0, |scroll| scroll.offset);
        for child in self.nodes[index].children.clone() {
            self.place(child, bounds.x, bounds.y - offset, clip, shape);
        }
    }

    pub fn display_list(&self) -> Vec<Paint> {
        let mut paints = Vec::new();
        self.paint(self.root.unwrap(), &mut paints);
        paints
    }

    fn paint(&self, index: NodeId, paints: &mut Vec<Paint>) {
        let node = &self.nodes[index];
        if !node.visible || node.clip.width <= 0.0 || node.clip.height <= 0.0 {
            return;
        }
        if let Some(color) = node.background {
            paints.push(Paint::Rect {
                bounds: node.bounds,
                clip: node.clip,
                color,
            });
        }
        if node.buffer.is_some() {
            paints.push(Paint::Text(index));
        }
        for child in &node.children {
            self.paint(*child, paints);
        }
        self.paint_interaction(index, paints);
        self.paint_scroll_indicator(index, paints);
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<NodeId> {
        self.deepest_hit(x, y).and_then(|id| self.click_target(id))
    }

    fn hit(&self, index: NodeId, x: f32, y: f32) -> Option<NodeId> {
        let node = &self.nodes[index];
        if !node.visible || !node.clip.contains(x, y) {
            return None;
        }
        for child in node.children.iter().rev() {
            if let Some(hit) = self.hit(*child, x, y) {
                return Some(hit);
            }
        }
        Some(index)
    }

    #[cfg(test)]
    pub fn pointer_button(&mut self, down: bool, x: f32, y: f32) -> bool {
        self.mouse_button(down, winit::event::MouseButton::Left, x, y)
    }

    pub fn cancel_pointer(&mut self) {
        self.cancel_pointer_interaction();
    }
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;

#[cfg(test)]
#[path = "batch_tests.rs"]
mod batch_tests;

#[cfg(test)]
#[path = "background_tests.rs"]
mod background_tests;

#[cfg(test)]
#[path = "scroll_tests.rs"]
mod scroll_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn test_button(value: &Signal<i32>) -> Element {
        let increment = value.clone();
        Element::column()
            .padding(8.0)
            .on_click(move || increment.update(|value| *value += 1))
            .child(Element::bound_text(value, |value| format!("value={value}")))
    }

    #[test]
    fn real_text_wraps_when_width_changes() {
        let mut scene = Scene::new(Element::column().child(Element::text(
            "中文排版与自动换行测试，Rust retained layout and real font measurement.",
        )));
        scene.layout(600.0, 800.0);
        let wide = scene.nodes.iter().next().unwrap().bounds.height;
        scene.layout(120.0, 800.0);
        assert!(scene.nodes.iter().next().unwrap().bounds.height > wide);
        assert!(scene.nodes.iter().next().unwrap().bounds.width <= 120.0);
    }

    #[test]
    fn click_updates_only_bound_node_and_drop_unsubscribes() {
        let counter = Signal::new(0);
        let mut scene = Scene::new(test_button(&counter));
        scene.layout(720.0, 600.0);
        let button = scene
            .nodes
            .iter()
            .find(|node| node.click.is_some())
            .unwrap()
            .bounds;
        let x = button.x + 8.0;
        let y = button.y + 8.0;
        assert!(scene.hit_test(x, y).is_some());
        assert!(!scene.pointer_button(false, x, y));
        scene.pointer_button(true, x, y);
        assert!(scene.pointer_button(false, x, y));
        assert_eq!(counter.get(), 1);
        counter.update(|value| *value += 1);
        assert_eq!(scene.sync(), 1);
        let updated = scene.nodes.iter().find(|node| node.revision == 1).unwrap();
        assert_eq!(updated.buffer.as_ref().unwrap().lines[0].text(), "value=2");
        assert_eq!(
            scene.nodes.iter().filter(|node| node.revision == 1).count(),
            1
        );
        assert_eq!(scene.sync(), 0);
        scene.pointer_button(true, x, y);
        assert!(!scene.pointer_button(false, -1.0, -1.0));
        assert_eq!(counter.get(), 2);
        assert_eq!(counter.subscriber_count(), 1);
        drop(scene);
        assert_eq!(counter.subscriber_count(), 0);
    }

    #[test]
    fn display_list_preserves_interleaved_tree_order() {
        let mut scene = Scene::new(
            Element::column()
                .background([0.0; 4])
                .child(Element::text("first"))
                .child(
                    Element::column()
                        .background([1.0; 4])
                        .child(Element::text("last")),
                ),
        );
        scene.layout(400.0, 600.0);
        assert!(matches!(
            scene.display_list().as_slice(),
            [
                Paint::Rect { .. },
                Paint::Text(first),
                Paint::Rect { .. },
                Paint::Text(last)
            ] if first != last
                && scene.nodes[*first].buffer.as_ref().unwrap().lines[0].text() == "first"
                && scene.nodes[*last].buffer.as_ref().unwrap().lines[0].text() == "last"
        ));
    }

    #[test]
    fn clipping_excludes_invisible_button_and_edges() {
        let counter = Signal::new(0);
        let mut scene = Scene::new(test_button(&counter));
        scene.layout(400.0, 1.0);
        assert!(scene.hit_test(50.0, 300.0).is_none());
        let bounds = Bounds {
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 20.0,
        };
        assert!(!bounds.contains(30.0, 20.0));
        assert_eq!(bounds.intersect(Bounds::default()).width, 0.0);
    }

    fn texts(scene: &Scene) -> Vec<String> {
        scene
            .nodes
            .iter()
            .filter_map(|node| node.buffer.as_ref())
            .map(|buffer| buffer.lines[0].text().to_owned())
            .collect()
    }

    #[test]
    fn generic_bindings_capture_and_coalesce_independently() {
        let number = Signal::new(1.5_f64);
        let text = Signal::new(String::from("初始"));
        let flag = Signal::new(false);
        let prefix = String::from("数字");
        let calls = Rc::new(RefCell::new(0));
        let formatted = calls.clone();
        let mut scene = Scene::new(
            Element::column()
                .child(Element::bound_text(&number, move |value| {
                    *formatted.borrow_mut() += 1;
                    format!("{prefix}={value}")
                }))
                .child(Element::bound_text(&text, |value| value))
                .child(Element::bound_text(&flag, |value| value.to_string())),
        );
        number.update(|value| *value = 2.5);
        number.update(|value| *value = 3.5);
        assert_eq!(*calls.borrow(), 1);
        assert_eq!(scene.sync(), 1);
        assert_eq!(*calls.borrow(), 2);
        assert_eq!(texts(&scene), ["数字=3.5", "初始", "false"]);
        text.update(|value| *value = "更新".into());
        flag.update(|value| *value = true);
        assert_eq!(scene.sync(), 2);
        assert_eq!(texts(&scene), ["数字=3.5", "更新", "true"]);
        assert_eq!(scene.sync(), 0);
    }

    #[test]
    fn two_buttons_are_independent() {
        let left = Signal::new(0);
        let right = Signal::new(0);
        let mut scene = Scene::new(
            Element::column()
                .child(test_button(&left))
                .child(test_button(&right)),
        );
        scene.layout(400.0, 600.0);
        let buttons: Vec<_> = scene
            .nodes
            .iter()
            .filter(|node| node.click.is_some())
            .map(|node| node.bounds)
            .collect();
        for (index, expected) in [(0, (1, 0)), (0, (2, 0)), (1, (2, 1))] {
            let bounds = buttons[index];
            scene.pointer_button(true, bounds.x + 1.0, bounds.y + 1.0);
            assert!(scene.pointer_button(false, bounds.x + 1.0, bounds.y + 1.0));
            assert_eq!((left.get(), right.get()), expected);
            scene.sync();
        }
    }

    #[test]
    fn hidden_parent_excludes_layout_paint_and_hits_and_retains_state() {
        let shown = Signal::new(true);
        let value = Signal::new(7);
        let mut scene = Scene::new(
            Element::column()
                .gap(10.0)
                .child(
                    Element::column()
                        .padding(5.0)
                        .background([1.0; 4])
                        .visible(&shown)
                        .child(test_button(&value)),
                )
                .child(Element::text("after")),
        );
        scene.layout(400.0, 600.0);
        let button = scene
            .nodes
            .iter()
            .find(|node| node.click.is_some())
            .unwrap()
            .bounds;
        let after = scene
            .nodes
            .iter()
            .find(|node| {
                node.buffer
                    .as_ref()
                    .is_some_and(|buffer| buffer.lines[0].text() == "after")
            })
            .unwrap()
            .bounds;
        let paints = scene.display_list().len();
        scene.pointer_button(true, button.x + 1.0, button.y + 1.0);
        shown.update(|value| *value = false);
        assert!(!scene.pointer_button(false, button.x + 1.0, button.y + 1.0));
        assert_eq!(value.get(), 7);
        scene.layout(400.0, 600.0);
        assert_eq!(scene.display_list().len(), 1);
        assert!(scene.hit_test(button.x + 1.0, button.y + 1.0).is_none());
        assert!(scene.nodes.iter().any(|node| {
            node.buffer
                .as_ref()
                .is_some_and(|buffer| buffer.lines[0].text() == "after")
                && node.bounds.y == 0.0
        }));
        assert!(after.y > 0.0);
        value.update(|value| *value += 1);
        shown.update(|value| *value = true);
        assert_eq!(scene.sync(), 2);
        scene.layout(400.0, 600.0);
        assert_eq!(scene.display_list().len(), paints);
        assert_eq!(texts(&scene), ["value=8", "after"]);
        assert!(scene.hit_test(button.x + 1.0, button.y + 1.0).is_some());
        assert!(!scene.pointer_button(false, button.x + 1.0, button.y + 1.0));
    }

    #[test]
    fn text_and_visibility_subscriptions_drop_without_a_capture_cycle() {
        let signal = Signal::new(true);
        let captured_signal = signal.clone();
        let lifetime = Rc::new(());
        let weak = Rc::downgrade(&lifetime);
        let element = Element::bound_text(&signal, move |value| {
            let _owner = &lifetime;
            format!("{value}/{}", captured_signal.get())
        })
        .visible(&signal);
        let mut scene = Scene::new(element);
        assert_eq!(signal.subscriber_count(), 2);
        signal.update(|value| *value = false);
        signal.update(|value| *value = true);
        assert_eq!(scene.sync(), 0);
        assert_eq!(scene.nodes.iter().next().unwrap().revision, 0);
        signal.update(|value| *value = false);
        drop(scene);
        assert_eq!(signal.subscriber_count(), 0);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn initially_hidden_root_has_no_paints_or_hits() {
        let shown = Signal::new(false);
        let value = Signal::new(0);
        let element = test_button(&value).visible(&shown);
        let mut scene = Scene::new(element);
        scene.layout(400.0, 600.0);
        assert!(scene.display_list().is_empty());
        assert!(scene.hit_test(1.0, 1.0).is_none());
        assert_eq!(scene.nodes[scene.root.unwrap()].bounds.height, 0.0);
        shown.update(|value| *value = true);
        scene.sync();
        scene.layout(400.0, 600.0);
        assert!(!scene.display_list().is_empty());
    }

    #[test]
    fn mounting_reads_latest_values_and_drops_owners_before_scene() {
        let lifetime = Rc::new(());
        let weak = Rc::downgrade(&lifetime);
        let text = Signal::new((lifetime, String::from("before")));
        let shown = Signal::new(true);
        let readable = text.clone();
        let root = Element::bound_text(&text, move |(_, value)| {
            assert_eq!(value, readable.get().1);
            value
        })
        .visible(&shown);
        text.update(|(_, value)| *value = "after".into());
        shown.update(|value| *value = false);
        assert_eq!(text.subscriber_count(), 0);
        let mut scene = Scene::new(root);
        assert_eq!(texts(&scene), ["after"]);
        assert!(scene.display_list().is_empty());
        text.update(|(_, value)| *value = "pending".into());
        drop(text);
        drop(shown);
        assert_eq!(scene.sync(), 1);
        assert_eq!(texts(&scene), ["pending"]);
        drop(scene);
        assert!(weak.upgrade().is_none());
    }
}
