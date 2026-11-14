use super::*;
use crate::{Direction, EditUnit, TextEditModel, message_channel};
use glyphon::Affinity;
use std::{
    num::NonZeroUsize,
    ops::Range,
    sync::mpsc,
    time::{Duration, Instant},
};
use unicode_segmentation::UnicodeSegmentation;
use winit::{
    event::{ElementState, Ime},
    keyboard::{Key, NamedKey},
};

pub(super) const BLINK_INTERVAL: Duration = Duration::from_millis(500);
const INSET: f32 = 4.0;

pub(super) fn single_line(text: &str) -> String {
    text.replace("\r\n", " ")
        .replace(['\r', '\n', '\u{85}', '\u{2028}', '\u{2029}'], " ")
}

pub(super) struct Editor {
    pub model: TextEditModel,
    signal: Signal<String>,
    pub readonly: bool,
    pub epoch: u64,
    pub dragging: bool,
    pub offset: f32,
    affinity: Affinity,
    caret_visible: bool,
    deadline: Option<Instant>,
    pub ime_session: u64,
    composition: Option<Composition>,
}

struct Composition {
    selection: crate::Selection,
    text: String,
    cursor: Option<(usize, usize)>,
}

impl Editor {
    pub(super) fn has_pending_ime(&self) -> bool {
        self.composition.is_some()
    }

    pub fn new(signal: Signal<String>, text: &str, readonly: bool) -> Self {
        Self {
            model: TextEditModel::new(text),
            signal,
            readonly,
            epoch: 0,
            dragging: false,
            offset: 0.0,
            affinity: Affinity::Before,
            caret_visible: false,
            deadline: None,
            ime_session: 0,
            composition: None,
        }
    }

    pub fn invalidate(&mut self) {
        self.epoch += 1;
        self.ime_session += 1;
        self.composition = None;
        self.dragging = false;
        self.deadline = None;
        self.caret_visible = false;
    }

    pub fn external(&mut self, text: &str) {
        if self.model.replace_programmatic(text) {
            self.invalidate();
        }
    }

    fn restart(&mut self) {
        self.epoch += 1;
        self.caret_visible = true;
        self.deadline = Some(Instant::now() + BLINK_INTERVAL);
    }

    fn display(&self) -> String {
        let Some(composition) = &self.composition else {
            return self.model.text().to_owned();
        };
        if composition.text.is_empty() {
            return self.model.text().to_owned();
        }
        let range = composition.selection.range();
        format!(
            "{}{}{}",
            &self.model.text()[..range.start],
            composition.text,
            &self.model.text()[range.end..]
        )
    }

    fn display_selection(&self) -> Range<usize> {
        if let Some(composition) = &self.composition
            && !composition.text.is_empty()
        {
            let start = composition.selection.range().start;
            return composition.cursor.map_or(start..start, |(anchor, focus)| {
                start + anchor.min(focus)..start + anchor.max(focus)
            });
        }
        self.model.selection().range()
    }

    fn display_caret(&self) -> Option<usize> {
        match &self.composition {
            Some(composition) if !composition.text.is_empty() => composition
                .cursor
                .map(|(_, focus)| composition.selection.range().start + focus),
            _ => Some(self.model.selection().focus()),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Stop {
    index: usize,
    x: f32,
    affinity: Affinity,
}

struct Span {
    range: Range<usize>,
    x: f32,
    width: f32,
}

struct Geometry {
    stops: Vec<Stop>,
    spans: Vec<Span>,
    height: f32,
}

impl Geometry {
    fn new(buffer: &Buffer, text: &str) -> Self {
        let boundaries: Vec<_> = text
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .chain(std::iter::once(text.len()))
            .collect();
        let mut geometry = Self {
            stops: Vec::new(),
            spans: Vec::new(),
            height: buffer.metrics().line_height,
        };
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let start = boundaries.partition_point(|index| *index < glyph.start);
                let end = boundaries.partition_point(|index| *index <= glyph.end);
                let cluster = &boundaries[start..end];
                let count = cluster.len().saturating_sub(1);
                if count == 0 {
                    continue;
                }
                for (ordinal, index) in cluster.iter().enumerate() {
                    let fraction = ordinal as f32 / count as f32;
                    let x = glyph.x
                        + glyph.w
                            * if glyph.level.is_rtl() {
                                1.0 - fraction
                            } else {
                                fraction
                            };
                    geometry.stops.push(Stop {
                        index: *index,
                        x,
                        affinity: if ordinal == count {
                            Affinity::Before
                        } else {
                            Affinity::After
                        },
                    });
                }
                for (ordinal, pair) in cluster.windows(2).enumerate() {
                    let fraction = if glyph.level.is_rtl() {
                        count - ordinal - 1
                    } else {
                        ordinal
                    } as f32
                        / count as f32;
                    geometry.spans.push(Span {
                        range: pair[0]..pair[1],
                        x: glyph.x + glyph.w * fraction,
                        width: glyph.w / count as f32,
                    });
                }
            }
        }
        if geometry.stops.is_empty() {
            geometry.stops.push(Stop {
                index: 0,
                x: 0.0,
                affinity: Affinity::After,
            });
        }
        geometry
    }

    fn caret(&self, index: usize, affinity: Affinity) -> f32 {
        self.stops
            .iter()
            .find(|stop| stop.index == index && stop.affinity == affinity)
            .or_else(|| self.stops.iter().find(|stop| stop.index == index))
            .or_else(|| {
                self.stops
                    .iter()
                    .min_by_key(|stop| stop.index.abs_diff(index))
            })
            .map_or(0.0, |stop| stop.x)
    }

    fn hit(&self, x: f32) -> Stop {
        *self
            .stops
            .iter()
            .min_by(|left, right| (left.x - x).abs().total_cmp(&(right.x - x).abs()))
            .unwrap()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClipboardAction {
    Copy,
    Cut,
    Paste,
}

#[derive(Clone, Copy)]
struct Ticket {
    id: NodeId,
    epoch: u64,
    action: ClipboardAction,
    expires: Instant,
}

struct ClipboardRequest {
    ticket: Ticket,
    text: String,
}
struct ClipboardReply {
    ticket: Ticket,
    result: std::result::Result<String, String>,
}

pub(super) struct ClipboardService {
    sender: mpsc::SyncSender<ClipboardRequest>,
    replies: Rc<RefCell<VecDeque<ClipboardReply>>>,
}

impl Scene {
    pub(super) fn semantic_text_value(&mut self, id: NodeId, value: &str) -> bool {
        self.refresh_controlled(id);
        let editor = self.nodes[id].editor.as_mut().unwrap();
        if editor.readonly || editor.has_pending_ime() {
            return false;
        }
        editor.model.select_all();
        editor.model.replace_selection(&single_line(value));
        self.finish_text_action(id);
        true
    }
    pub(crate) fn ime_target(&self) -> Option<(NodeId, u64)> {
        let id = self.interaction.focused?;
        let editor = self.nodes.get(id)?.editor.as_ref()?;
        (self.enabled(id) && !editor.readonly).then_some((id, editor.ime_session))
    }

    pub(crate) fn ime(&mut self, event: Ime) {
        self.sync();
        if event == Ime::Disabled {
            self.ime_ready = false;
            self.cancel_ime();
            return;
        }
        if self.ime_owner.is_none() || self.ime_owner != self.ime_target() {
            return;
        }
        if event == Ime::Enabled {
            self.ime_ready = true;
            return;
        }
        if !self.ime_ready {
            return;
        }
        let id = self.ime_owner.unwrap().0;
        self.refresh_controlled(id);
        if self.ime_owner != self.ime_target() {
            return;
        }
        let editor = self.nodes[id].editor.as_mut().unwrap();
        match event {
            Ime::Preedit(text, cursor) => {
                if text.chars().any(char::is_control)
                    || single_line(&text) != text
                    || cursor.is_some_and(|(anchor, focus)| {
                        !text.is_char_boundary(anchor) || !text.is_char_boundary(focus)
                    })
                {
                    self.cancel_ime();
                    return;
                }
                let composition = editor.composition.get_or_insert_with(|| Composition {
                    selection: editor.model.selection(),
                    text: String::new(),
                    cursor: None,
                });
                composition.text = text;
                composition.cursor = cursor;
                editor.restart();
                let composing = !composition_is_empty(editor);
                self.composing(composing);
                self.refresh_ime_display();
            }
            Ime::Commit(text) => {
                if let Some(composition) = editor.composition.take() {
                    editor
                        .model
                        .set_selection(
                            composition.selection.anchor(),
                            composition.selection.focus(),
                        )
                        .unwrap();
                }
                if !text.is_empty() {
                    editor.model.replace_selection(&single_line(&text));
                }
                self.composing(false);
                self.finish_text_action(id);
            }
            _ => {}
        }
    }

    pub(crate) fn cancel_ime(&mut self) {
        if let Some((id, _)) = self.ime_owner
            && let Some(editor) = self.nodes.get_mut(id).and_then(|node| node.editor.as_mut())
        {
            editor.invalidate();
        }
        self.composing(false);
        self.refresh_ime_display();
    }

    pub(super) fn refresh_ime_display(&mut self) {
        let ids: Vec<_> = self
            .nodes
            .iter()
            .filter_map(|node| {
                node.editor
                    .as_ref()
                    .map(|_| self.tree.get_node_context(node.layout_id).copied().unwrap())
            })
            .collect();
        for id in ids {
            let text = self.nodes[id].editor.as_ref().unwrap().display();
            let node = &mut self.nodes[id];
            if node.text.as_ref() == Some(&text) {
                continue;
            }
            node.text_version += 1;
            node.buffer.as_mut().unwrap().set_text(
                &mut self.fonts,
                &text,
                &Attrs::new().family(Family::SansSerif),
                Shaping::Advanced,
            );
            node.text = Some(text);
            node.revision += 1;
            self.tree.mark_dirty(node.layout_id).unwrap();
            self.dirty.text_measurement = true;
            self.present_needed = true;
        }
    }

    pub(crate) fn ime_cursor_area(&self) -> Option<Bounds> {
        let (id, _) = self.ime_target()?;
        let node = &self.nodes[id];
        let editor = node.editor.as_ref()?;
        let (origin, clip) = self.text_geometry(id);
        if clip.width <= 0.0 || clip.height <= 0.0 {
            return None;
        }
        let geometry = Geometry::new(node.buffer.as_ref()?, node.text.as_deref()?);
        let caret_x = origin.0
            + geometry.caret(
                editor
                    .display_caret()
                    .unwrap_or_else(|| editor.display_selection().start),
                editor.affinity,
            );
        let width = 1.5_f32.min(clip.width);
        let y = origin.1.max(clip.y);
        let height = (origin.1 + geometry.height).min(clip.y + clip.height) - y;
        (height > 0.0).then_some(Bounds {
            x: caret_x.clamp(clip.x, clip.x + clip.width - width),
            y,
            width,
            height,
        })
    }

    pub(super) fn text_focus_changed(&mut self, previous: Option<NodeId>, next: Option<NodeId>) {
        for id in previous.into_iter().chain(next) {
            if let Some(editor) = self.nodes.get_mut(id).and_then(|node| node.editor.as_mut()) {
                editor.invalidate();
            }
        }
        if let Some(id) = previous
            && self.interaction.captured == Some(id)
        {
            self.set_capture(None);
        }
    }

    pub(super) fn stop_text_drag(&mut self, id: NodeId) {
        if let Some(editor) = self.nodes.get_mut(id).and_then(|node| node.editor.as_mut()) {
            editor.dragging = false;
        }
    }

    pub(super) fn reconcile_editors(&mut self) {
        let mut ids: Vec<_> = self.root.into_iter().collect();
        while let Some(id) = ids.pop() {
            ids.extend(self.nodes[id].children.iter().copied());
            if !self.enabled(id)
                && let Some(editor) = self.nodes[id].editor.as_mut()
            {
                editor.invalidate();
            }
        }
        if let Some(id) = self.interaction.captured
            && self
                .nodes
                .get(id)
                .and_then(|node| node.editor.as_ref())
                .is_some_and(|editor| !editor.dragging)
        {
            self.set_capture(None);
        }
        if self.ime_owner.is_some() && self.ime_owner != self.ime_target() {
            self.composing(false);
        }
        self.refresh_ime_display();
    }

    pub(crate) fn update_blink(&mut self, now: Instant, drawable: bool) {
        let Some(id) = self.interaction.focused else {
            return;
        };
        let enabled = self.enabled(id);
        let (_, clip) = self.text_geometry(id);
        let node = &mut self.nodes[id];
        let Some(editor) = &mut node.editor else {
            return;
        };
        let active =
            drawable && enabled && !editor.readonly && clip.width > 0.0 && clip.height > 0.0;
        if !active {
            editor.deadline = None;
            if editor.caret_visible {
                self.present_needed = true;
            }
            editor.caret_visible = false;
        } else if editor.deadline.is_none() {
            editor.deadline = Some(now + BLINK_INTERVAL);
            editor.caret_visible = true;
            self.present_needed = true;
        } else if editor.deadline.is_some_and(|deadline| now >= deadline) {
            editor.caret_visible = !editor.caret_visible;
            editor.deadline = Some(now + BLINK_INTERVAL);
            self.present_needed = true;
        }
    }

    pub(crate) fn blink_deadline(&self) -> Option<Instant> {
        self.interaction
            .focused
            .and_then(|id| self.nodes.get(id))?
            .editor
            .as_ref()?
            .deadline
    }

    pub(crate) fn text_geometry(&self, id: NodeId) -> ((f32, f32), Bounds) {
        let node = &self.nodes[id];
        if let Some(editor) = &node.editor {
            let inner = Bounds {
                x: node.bounds.x + INSET,
                y: node.bounds.y + INSET,
                width: (node.bounds.width - 2.0 * INSET).max(0.0),
                height: (node.bounds.height - 2.0 * INSET).max(0.0),
            };
            (
                (inner.x - editor.offset, inner.y),
                inner.intersect(node.clip),
            )
        } else if node.presentation.control.is_some() {
            let padding = self.tree.layout(node.layout_id).unwrap().padding;
            let inner = Bounds {
                x: node.bounds.x + padding.left,
                y: node.bounds.y + padding.top,
                width: (node.bounds.width - padding.left - padding.right).max(0.0),
                height: (node.bounds.height - padding.top - padding.bottom).max(0.0),
            };
            ((inner.x, inner.y), inner.intersect(node.clip))
        } else {
            ((node.bounds.x, node.bounds.y), node.clip)
        }
    }

    pub(super) fn track_text_caret(&mut self) {
        let Some(id) = self.interaction.focused else {
            return;
        };
        let node = &mut self.nodes[id];
        let Some(editor) = &mut node.editor else {
            return;
        };
        let geometry = Geometry::new(node.buffer.as_ref().unwrap(), node.text.as_deref().unwrap());
        let caret = geometry.caret(
            editor
                .display_caret()
                .unwrap_or_else(|| editor.display_selection().start),
            editor.affinity,
        );
        let width = (node.bounds.width - 2.0 * INSET - 2.0).max(0.0);
        let offset = editor.offset;
        if caret < editor.offset {
            editor.offset = caret.max(0.0);
        }
        if caret > editor.offset + width {
            editor.offset = (caret - width).max(0.0);
        }
        let max_x = geometry.stops.iter().map(|stop| stop.x).fold(0.0, f32::max);
        editor.offset = editor.offset.min((max_x - width).max(0.0));
        self.present_needed |= offset != editor.offset;
    }

    fn finish_text_action(&mut self, id: NodeId) {
        let node = &mut self.nodes[id];
        let editor = node.editor.as_mut().unwrap();
        editor.restart();
        let text = editor.model.text().to_owned();
        let signal = editor.signal.clone();
        let publish = single_line(&signal.get()) != text;
        let changed = node.text.as_deref() != Some(&text);
        if changed {
            node.text_version += 1;
            node.buffer.as_mut().unwrap().set_text(
                &mut self.fonts,
                &text,
                &Attrs::new().family(Family::SansSerif),
                Shaping::Advanced,
            );
            node.text = Some(text.clone());
            node.revision += 1;
            self.tree.mark_dirty(node.layout_id).unwrap();
            self.dirty.text_measurement = true;
            self.commit_version += 1;
        }
        self.present_needed = true;
        if publish {
            signal.update(|value| *value = text);
        }
    }

    fn refresh_controlled(&mut self, id: NodeId) {
        let node = &mut self.nodes[id];
        let Some(editor) = &mut node.editor else {
            return;
        };
        let text = single_line(&editor.signal.get());
        if editor.model.text() == text {
            return;
        }
        editor.external(&text);
        node.text_version += 1;
        node.buffer.as_mut().unwrap().set_text(
            &mut self.fonts,
            &text,
            &Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
        );
        node.text = Some(text);
        node.revision += 1;
        self.tree.mark_dirty(node.layout_id).unwrap();
        self.dirty.text_measurement = true;
        self.commit_version += 1;
        self.present_needed = true;
    }

    pub(super) fn text_key(&mut self, id: NodeId, input: &KeyInput) -> bool {
        if input.state != ElementState::Pressed {
            return false;
        }
        self.refresh_controlled(id);
        let Some(editor) = self.nodes[id].editor.as_mut() else {
            return false;
        };
        let modifiers = self.interaction.modifiers;
        let command = modifiers.control_key() || modifiers.super_key();
        let shift = modifiers.shift_key();
        let unit = if command || modifiers.alt_key() {
            EditUnit::Word
        } else {
            EditUnit::Grapheme
        };
        let letter = match &input.logical_key {
            Key::Character(text) => text.to_lowercase(),
            _ => String::new(),
        };
        if command && !modifiers.alt_key() {
            let action = match letter.as_str() {
                "c" => Some(ClipboardAction::Copy),
                "x" if !editor.readonly => Some(ClipboardAction::Cut),
                "v" if !editor.readonly => Some(ClipboardAction::Paste),
                _ => None,
            };
            if let Some(action) = action {
                self.request_clipboard(id, action);
                return true;
            }
            match letter.as_str() {
                "a" => editor.model.select_all(),
                "z" if !editor.readonly => {
                    if shift {
                        editor.model.redo();
                    } else {
                        editor.model.undo();
                    }
                }
                "y" if !editor.readonly => {
                    editor.model.redo();
                }
                _ => {
                    if !matches!(
                        input.logical_key,
                        Key::Named(
                            NamedKey::ArrowLeft
                                | NamedKey::ArrowRight
                                | NamedKey::Home
                                | NamedKey::End
                                | NamedKey::Backspace
                                | NamedKey::Delete
                        )
                    ) {
                        return true;
                    }
                }
            }
        }
        match &input.logical_key {
            Key::Named(NamedKey::Tab) => return false,
            Key::Named(NamedKey::ArrowLeft) => {
                editor.model.move_cursor(Direction::Backward, unit, shift)
            }
            Key::Named(NamedKey::ArrowRight) => {
                editor.model.move_cursor(Direction::Forward, unit, shift)
            }
            Key::Named(NamedKey::Home | NamedKey::End) => {
                let focus = if input.logical_key == Key::Named(NamedKey::Home) {
                    0
                } else {
                    editor.model.text().len()
                };
                let anchor = if shift {
                    editor.model.selection().anchor()
                } else {
                    focus
                };
                editor.model.set_selection(anchor, focus).unwrap();
            }
            Key::Named(NamedKey::Backspace | NamedKey::Delete) if !editor.readonly => {
                editor.model.delete(
                    if input.logical_key == Key::Named(NamedKey::Backspace) {
                        Direction::Backward
                    } else {
                        Direction::Forward
                    },
                    unit,
                );
            }
            Key::Named(NamedKey::Enter) => return true,
            _ if !command && !modifiers.alt_key() && !editor.readonly => {
                if let Some(text) = &input.text {
                    let text: String = text
                        .chars()
                        .filter(|character| !character.is_control())
                        .collect();
                    if !text.is_empty() {
                        editor.model.replace_selection(&single_line(&text));
                    }
                }
            }
            _ => {}
        }
        editor.affinity = Affinity::Before;
        self.finish_text_action(id);
        true
    }

    pub(super) fn text_pointer_down(&mut self, id: NodeId, position: Option<(f32, f32)>) {
        if self.nodes[id].editor.is_none() {
            return;
        }
        self.refresh_controlled(id);
        if self.nodes[id].editor.as_ref().unwrap().has_pending_ime() {
            self.cancel_ime();
        }
        let Some((x, y)) = position else {
            return;
        };
        self.text_pointer_selection(id, x, y, self.interaction.modifiers.shift_key());
        self.nodes[id].editor.as_mut().unwrap().dragging = true;
        self.set_capture(Some(id));
    }

    pub(super) fn text_pointer_move(&mut self, id: NodeId, x: f32, y: f32) {
        if self.nodes[id]
            .editor
            .as_ref()
            .is_some_and(|editor| editor.dragging)
        {
            self.text_pointer_selection(id, x, y, true);
        }
    }

    fn text_pointer_selection(&mut self, id: NodeId, x: f32, _y: f32, extend: bool) {
        let (origin, _) = self.text_geometry(id);
        let node = &mut self.nodes[id];
        let editor = node.editor.as_mut().unwrap();
        let stop =
            Geometry::new(node.buffer.as_ref().unwrap(), editor.model.text()).hit(x - origin.0);
        let anchor = if extend {
            editor.model.selection().anchor()
        } else {
            stop.index
        };
        editor.model.set_selection(anchor, stop.index).unwrap();
        editor.affinity = stop.affinity;
        editor.restart();
        self.present_needed = true;
    }

    pub(super) fn paint_text_selection(&self, id: NodeId, paints: &mut Vec<Paint>) {
        let node = &self.nodes[id];
        let Some(editor) = &node.editor else {
            return;
        };
        if !self.enabled(id) || self.interaction.focused != Some(id) {
            return;
        }
        let (origin, clip) = self.text_geometry(id);
        let geometry = Geometry::new(node.buffer.as_ref().unwrap(), node.text.as_deref().unwrap());
        let selected = editor.display_selection();
        let underline = editor.composition.as_ref().map(|composition| {
            let start = composition.selection.range().start;
            start..start + composition.text.len()
        });
        for span in geometry.spans {
            if underline
                .as_ref()
                .is_some_and(|range| span.range.start < range.end && span.range.end > range.start)
            {
                paints.push(Paint::Rect {
                    bounds: Bounds {
                        x: origin.0 + span.x,
                        y: origin.1 + geometry.height - 2.0,
                        width: span.width,
                        height: 1.0,
                    },
                    clip,
                    color: self.edit_color(crate::ThemeColor::Focus, [1.0, 0.85, 0.4, 1.0]),
                });
            }
            if span.range.start < selected.end && span.range.end > selected.start {
                paints.push(Paint::Rect {
                    bounds: Bounds {
                        x: origin.0 + span.x,
                        y: origin.1,
                        width: span.width,
                        height: geometry.height,
                    },
                    clip,
                    color: self.edit_color(crate::ThemeColor::Selection, [0.18, 0.35, 0.56, 1.0]),
                });
            }
        }
    }

    pub(super) fn paint_text_caret(&self, id: NodeId, paints: &mut Vec<Paint>) {
        let node = &self.nodes[id];
        let Some(editor) = &node.editor else {
            return;
        };
        if self.interaction.focused != Some(id)
            || !self.enabled(id)
            || editor.readonly
            || !editor.caret_visible
        {
            return;
        }
        let (origin, clip) = self.text_geometry(id);
        let Some(caret) = editor.display_caret() else {
            return;
        };
        let geometry = Geometry::new(node.buffer.as_ref().unwrap(), node.text.as_deref().unwrap());
        paints.push(Paint::Rect {
            bounds: Bounds {
                x: origin.0 + geometry.caret(caret, editor.affinity),
                y: origin.1,
                width: 1.5,
                height: geometry.height,
            },
            clip,
            color: self.edit_color(crate::ThemeColor::Text, [1.0, 0.85, 0.4, 1.0]),
        });
    }

    fn request_clipboard(&mut self, id: NodeId, action: ClipboardAction) {
        if self.nodes[id].editor.as_ref().unwrap().has_pending_ime() {
            self.cancel_ime();
        }
        let editor = self.nodes[id].editor.as_ref().unwrap();
        if action != ClipboardAction::Paste && editor.model.selection().is_collapsed() {
            return;
        }
        let request = ClipboardRequest {
            ticket: Ticket {
                id,
                epoch: editor.epoch,
                action,
                expires: Instant::now() + Duration::from_secs(5),
            },
            text: editor.model.selected_text().to_owned(),
        };
        if self.clipboard.is_none() {
            let (sender, receiver) = mpsc::sync_channel::<ClipboardRequest>(8);
            let (results, ui_receiver) = message_channel(NonZeroUsize::new(8).unwrap());
            let replies = Rc::new(RefCell::new(VecDeque::new()));
            let local = replies.clone();
            let route = ui_receiver.bind(move |reply| local.borrow_mut().push_back(reply));
            match std::thread::Builder::new()
                .name("text-clipboard".into())
                .spawn(move || {
                    let mut clipboard = None;
                    while let Ok(request) = receiver.recv() {
                        if results.is_closed() {
                            break;
                        }
                        let result = (|| {
                            if clipboard.is_none() {
                                clipboard = Some(
                                    arboard::Clipboard::new().map_err(|error| error.to_string())?,
                                );
                            }
                            let clipboard = clipboard.as_mut().unwrap();
                            if request.ticket.action == ClipboardAction::Paste {
                                clipboard.get_text().map_err(|error| error.to_string())
                            } else {
                                clipboard
                                    .set_text(request.text)
                                    .map(|()| String::new())
                                    .map_err(|error| error.to_string())
                            }
                        })();
                        if results
                            .try_send(ClipboardReply {
                                ticket: request.ticket,
                                result,
                            })
                            .is_err()
                        {
                            eprintln!("Chronobreak clipboard reply dropped: full or closed");
                        }
                    }
                }) {
                Ok(_) => {
                    route.attach(&self.wake);
                    self.routes.push_back((self.root.unwrap(), route));
                    self.clipboard = Some(ClipboardService { sender, replies });
                }
                Err(error) => {
                    eprintln!("Chronobreak clipboard worker: {error}");
                    return;
                }
            }
        }
        if let Err(error) = self.clipboard.as_ref().unwrap().sender.try_send(request) {
            eprintln!("Chronobreak clipboard request rejected: {error}");
        }
    }

    pub(super) fn receive_clipboard(&mut self) {
        let replies = self
            .clipboard
            .as_ref()
            .map(|service| std::mem::take(&mut *service.replies.borrow_mut()))
            .unwrap_or_default();
        for reply in replies {
            self.apply_clipboard(reply);
        }
    }

    fn apply_clipboard(&mut self, reply: ClipboardReply) {
        let text = match reply.result {
            Ok(text) => text,
            Err(error) => {
                eprintln!("Chronobreak clipboard: {error}");
                return;
            }
        };
        let Ticket {
            id,
            epoch,
            action,
            expires,
        } = reply.ticket;
        if action == ClipboardAction::Copy
            || Instant::now() >= expires
            || self.interaction.focused != Some(id)
            || !self.enabled(id)
        {
            return;
        }
        self.refresh_controlled(id);
        let Some(editor) = self.nodes.get_mut(id).and_then(|node| node.editor.as_mut()) else {
            return;
        };
        if editor.readonly || editor.epoch != epoch {
            return;
        }
        let normalized = if action == ClipboardAction::Paste {
            single_line(&text)
        } else {
            String::new()
        };
        if action == ClipboardAction::Paste && normalized.is_empty() {
            return;
        }
        editor.model.replace_selection(&normalized);
        self.finish_text_action(id);
    }
}

fn composition_is_empty(editor: &Editor) -> bool {
    editor
        .composition
        .as_ref()
        .is_none_or(|composition| composition.text.is_empty())
}

#[cfg(test)]
#[path = "text_input_tests.rs"]
mod tests;
