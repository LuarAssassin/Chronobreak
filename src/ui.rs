use crate::signal::{Signal, Subscription};
use glyphon::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Wrap};
use std::{
    any::Any,
    cell::RefCell,
    collections::BTreeMap,
    rc::{Rc, Weak},
};
use taffy::{prelude::*, style::AvailableSpace};

pub type Color = [f32; 4];
#[derive(Default)]
struct Update {
    text: Option<Box<dyn Any>>,
    visible: Option<bool>,
}

type Updates = RefCell<BTreeMap<usize, Update>>;
type Pending = Rc<Updates>;
type TextFormatter = Box<dyn Fn(Box<dyn Any>) -> String>;
type Binding = Box<dyn FnOnce(usize, Weak<Updates>) -> (String, Subscription, TextFormatter)>;
type VisibilityBinding = Box<dyn FnOnce(usize, Weak<Updates>) -> (bool, Subscription)>;

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
    click: Option<Rc<dyn Fn()>>,
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
            click: None,
        }
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Self::column()
        }
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
        self.children.push(child);
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
    layout_id: NodeId,
    children: Vec<usize>,
    pub buffer: Option<Buffer>,
    pub color: Color,
    pub bounds: Bounds,
    pub clip: Bounds,
    background: Option<Color>,
    click: Option<Rc<dyn Fn()>>,
    _subscriptions: Vec<Subscription>,
    formatter: Option<TextFormatter>,
    visible: bool,
    pub revision: usize,
}

#[derive(Clone, Copy)]
pub(crate) enum Paint {
    Rect {
        bounds: Bounds,
        clip: Bounds,
        color: Color,
    },
    Text(usize),
}

pub(crate) struct Scene {
    pub fonts: FontSystem,
    pub nodes: Vec<Node>,
    tree: TaffyTree<usize>,
    root: usize,
    pending: Pending,
    pressed: Option<usize>,
}

impl Scene {
    pub fn new(element: Element) -> Self {
        let mut scene = Self {
            fonts: FontSystem::new(),
            nodes: Vec::new(),
            tree: TaffyTree::new(),
            root: 0,
            pending: Rc::default(),
            pressed: None,
        };
        scene.root = scene.mount(element);
        scene
    }

    fn mount(&mut self, element: Element) -> usize {
        let children: Vec<_> = element
            .children
            .into_iter()
            .map(|child| self.mount(child))
            .collect();
        let child_ids: Vec<_> = children
            .iter()
            .map(|index| self.nodes[*index].layout_id)
            .collect();
        let index = self.nodes.len();
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
        let mut style = element.style;
        if !visible {
            style.display = Display::None;
        }
        let buffer = text.map(|text| {
            let mut buffer = Buffer::new(
                &mut self.fonts,
                Metrics::new(element.font_size, element.font_size * 1.5),
            );
            buffer.set_wrap(&mut self.fonts, Wrap::WordOrGlyph);
            buffer.set_text(
                &mut self.fonts,
                &text,
                &Attrs::new().family(Family::SansSerif),
                Shaping::Advanced,
            );
            buffer
        });
        let layout_id = self.tree.new_with_children(style, &child_ids).unwrap();
        if buffer.is_some() {
            self.tree.set_node_context(layout_id, Some(index)).unwrap();
        }
        self.nodes.push(Node {
            layout_id,
            children,
            buffer,
            color: element.color,
            bounds: Bounds::default(),
            clip: Bounds::default(),
            background: element.background,
            click: element.click,
            _subscriptions: subscriptions,
            formatter,
            visible,
            revision: 0,
        });
        index
    }

    pub fn sync(&mut self) -> usize {
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        let count = pending.len();
        for (index, update) in pending {
            let node = &mut self.nodes[index];
            if let Some(value) = update.text {
                let text = node.formatter.as_ref().unwrap()(value);
                node.buffer.as_mut().unwrap().set_text(
                    &mut self.fonts,
                    &text,
                    &Attrs::new().family(Family::SansSerif),
                    Shaping::Advanced,
                );
            }
            if let Some(visible) = update.visible {
                node.visible = visible;
                let mut style = self.tree.style(node.layout_id).unwrap().clone();
                style.display = if visible {
                    Display::Flex
                } else {
                    Display::None
                };
                self.tree.set_style(node.layout_id, style).unwrap();
                if !visible {
                    self.pressed = None;
                }
            }
            node.revision += 1;
            self.tree.mark_dirty(node.layout_id).unwrap();
        }
        count
    }

    pub fn layout(&mut self, width: f32, height: f32) {
        let root_id = self.nodes[self.root].layout_id;
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
        self.place(
            self.root,
            0.0,
            0.0,
            Bounds {
                x: 0.0,
                y: 0.0,
                width,
                height,
            },
        );
    }

    fn place(&mut self, index: usize, parent_x: f32, parent_y: f32, clip: Bounds) {
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
        if let Some(buffer) = &mut node.buffer {
            buffer.set_size(&mut self.fonts, Some(bounds.width), None);
            buffer.shape_until_scroll(&mut self.fonts, false);
        }
        let clip = node.clip;
        for child in self.nodes[index].children.clone() {
            self.place(child, bounds.x, bounds.y, clip);
        }
    }

    pub fn display_list(&self) -> Vec<Paint> {
        let mut paints = Vec::new();
        self.paint(self.root, &mut paints);
        paints
    }

    fn paint(&self, index: usize, paints: &mut Vec<Paint>) {
        let node = &self.nodes[index];
        if !node.visible {
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
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<usize> {
        self.hit(self.root, x, y)
    }

    fn hit(&self, index: usize, x: f32, y: f32) -> Option<usize> {
        let node = &self.nodes[index];
        if !node.visible || !node.clip.contains(x, y) {
            return None;
        }
        for child in node.children.iter().rev() {
            if let Some(hit) = self.hit(*child, x, y) {
                return Some(hit);
            }
        }
        node.click.as_ref().map(|_| index)
    }

    pub fn pointer_button(&mut self, down: bool, x: f32, y: f32) -> bool {
        self.sync();
        let hit = self.hit_test(x, y);
        if down {
            self.pressed = hit;
            return false;
        }
        if let Some(index) = self.pressed.take().filter(|pressed| Some(*pressed) == hit) {
            self.nodes[index].click.as_ref().unwrap()();
            return true;
        }
        false
    }

    pub fn cancel_pointer(&mut self) {
        self.pressed = None;
    }
}

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
        let wide = scene.nodes[0].bounds.height;
        scene.layout(120.0, 800.0);
        assert!(scene.nodes[0].bounds.height > wide);
        assert!(scene.nodes[0].bounds.width <= 120.0);
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
        let scene = Scene::new(
            Element::column()
                .background([0.0; 4])
                .child(Element::text("first"))
                .child(
                    Element::column()
                        .background([1.0; 4])
                        .child(Element::text("last")),
                ),
        );
        assert!(matches!(
            scene.display_list().as_slice(),
            [
                Paint::Rect { .. },
                Paint::Text(0),
                Paint::Rect { .. },
                Paint::Text(1)
            ]
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
        assert_eq!(scene.sync(), 1);
        assert_eq!(scene.nodes[0].revision, 1);
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
        assert_eq!(scene.nodes[scene.root].bounds.height, 0.0);
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
