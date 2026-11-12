use crate::signal::{Signal, Subscription};
use glyphon::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Wrap};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use taffy::{prelude::*, style::AvailableSpace};

pub type Color = [f32; 4];
type Pending = Rc<RefCell<BTreeMap<usize, String>>>;
type Binding = (Signal<u64>, fn(u64) -> String);

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
            click: None,
        }
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Self::column()
        }
    }

    pub fn bound_text(signal: &Signal<u64>, format: fn(u64) -> String) -> Self {
        Self {
            binding: Some((signal.clone(), format)),
            ..Self::text(format(signal.get()))
        }
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

pub struct Node {
    layout_id: NodeId,
    children: Vec<usize>,
    pub buffer: Option<Buffer>,
    pub color: Color,
    pub bounds: Bounds,
    pub clip: Bounds,
    background: Option<Color>,
    click: Option<Rc<dyn Fn()>>,
    _subscription: Option<Subscription>,
    pub revision: usize,
}

#[derive(Clone, Copy)]
pub enum Paint {
    Rect {
        bounds: Bounds,
        clip: Bounds,
        color: Color,
    },
    Text(usize),
}

pub struct Scene {
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
        let buffer = element.text.map(|text| {
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
        let layout_id = self
            .tree
            .new_with_children(element.style, &child_ids)
            .unwrap();
        if buffer.is_some() {
            self.tree.set_node_context(layout_id, Some(index)).unwrap();
        }
        let subscription = element.binding.map(|(signal, format)| {
            let pending = Rc::downgrade(&self.pending);
            signal.subscribe(move |value| {
                if let Some(pending) = pending.upgrade() {
                    pending.borrow_mut().insert(index, format(*value));
                }
            })
        });
        self.nodes.push(Node {
            layout_id,
            children,
            buffer,
            color: element.color,
            bounds: Bounds::default(),
            clip: Bounds::default(),
            background: element.background,
            click: element.click,
            _subscription: subscription,
            revision: 0,
        });
        index
    }

    pub fn sync(&mut self) -> usize {
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        let count = pending.len();
        for (index, text) in pending {
            let node = &mut self.nodes[index];
            node.buffer.as_mut().unwrap().set_text(
                &mut self.fonts,
                &text,
                &Attrs::new().family(Family::SansSerif),
                Shaping::Advanced,
            );
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
        if !node.clip.contains(x, y) {
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

pub fn demo(counter: &Signal<u64>) -> Element {
    let increment = counter.clone();
    Element::column().padding(28.0).child(
        Element::column().max_width(640.0).padding(24.0).gap(18.0)
            .background([0.075, 0.10, 0.16, 1.0])
            .child(Element::text("Chronobreak / 自绘 GUI").font_size(28.0))
            .child(Element::text("这是一个由 Rust 构建的保留模式信息卡片。文字使用真实字体排版，调整窗口宽度即可观察中文自动换行与 Flexbox 布局变化。"))
            .child(Element::text("原生 Rust builder · 响应式状态 · GPU 绘制").font_size(15.0))
            .child(Element::column().padding(16.0).background([0.12, 0.28, 0.52, 1.0])
                .on_click(move || increment.update(|value| *value += 1))
                .child(Element::bound_text(counter, |value| format!("点击计数：{value}  /  点击这里 +1"))))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut scene = Scene::new(demo(&counter));
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
        assert_eq!(
            updated.buffer.as_ref().unwrap().lines[0].text(),
            "点击计数：2  /  点击这里 +1"
        );
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
        let mut scene = Scene::new(demo(&counter));
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
}
