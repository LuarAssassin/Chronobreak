use super::*;
use std::collections::HashSet;
use winit::{
    event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, TouchPhase},
    keyboard::{Key, KeyLocation, ModifiersState, NamedKey, PhysicalKey},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventPhase {
    Capture,
    Target,
    Bubble,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyInput {
    pub logical_key: Key,
    pub physical_key: PhysicalKey,
    pub location: KeyLocation,
    pub state: ElementState,
    pub repeat: bool,
    pub synthetic: bool,
}

impl KeyInput {
    pub(crate) fn from_winit(event: KeyEvent, synthetic: bool) -> Self {
        Self {
            logical_key: event.logical_key,
            physical_key: event.physical_key,
            location: event.location,
            state: event.state,
            repeat: event.repeat,
            synthetic,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelInput {
    pub raw: MouseScrollDelta,
    pub phase: TouchPhase,
    pub vertical: f32,
}

impl WheelInput {
    pub fn from_winit(raw: MouseScrollDelta, phase: TouchPhase, scale: f64) -> Self {
        let vertical = match raw {
            MouseScrollDelta::LineDelta(_, vertical) => -vertical * 40.0,
            MouseScrollDelta::PixelDelta(position) => -(position.y / scale) as f32,
        };
        Self {
            raw,
            phase,
            vertical,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EventKind {
    PointerDown(MouseButton),
    PointerMove,
    PointerUp(MouseButton),
    PointerEnter,
    PointerLeave,
    PointerCancel,
    CaptureLost,
    Key(KeyInput),
    Wheel(WheelInput),
}

#[derive(Clone, Debug)]
pub struct UiEvent {
    pub kind: EventKind,
    pub position: Option<(f32, f32)>,
    pub modifiers: ModifiersState,
}

pub struct EventContext {
    pub target: NodeId,
    pub current_target: NodeId,
    pub phase: EventPhase,
    propagation_stopped: bool,
    default_prevented: bool,
    capture: Option<Option<NodeId>>,
    focus: Option<NodeId>,
}

impl EventContext {
    pub fn stop_propagation(&mut self) {
        self.propagation_stopped = true;
    }

    pub fn prevent_default(&mut self) {
        self.default_prevented = true;
    }

    pub fn propagation_stopped(&self) -> bool {
        self.propagation_stopped
    }

    pub fn default_prevented(&self) -> bool {
        self.default_prevented
    }

    pub fn capture_pointer(&mut self) {
        self.capture = Some(Some(self.current_target));
    }

    pub fn release_pointer(&mut self) {
        self.capture = Some(None);
    }

    pub fn focus(&mut self) {
        self.focus = Some(self.current_target);
    }
}

pub(super) type Handler = (EventPhase, Rc<dyn Fn(&UiEvent, &mut EventContext)>);

#[derive(Default)]
struct DispatchOutcome {
    default_prevented: bool,
    focus_requested: bool,
}

#[derive(Default)]
pub(super) struct Interaction {
    pointer: Option<(f32, f32)>,
    hovered: Vec<NodeId>,
    captured: Option<NodeId>,
    focused: Option<NodeId>,
    space: Option<(NodeId, PhysicalKey)>,
    held: HashSet<PhysicalKey>,
    buttons: HashSet<MouseButton>,
    modifiers: ModifiersState,
    composing: bool,
}

impl Scene {
    pub(super) fn scroll_enabled(&self, id: NodeId) -> bool {
        self.enabled(id)
    }

    fn enabled(&self, id: NodeId) -> bool {
        self.nodes.get(id).is_some_and(|node| {
            node.visible && !node.disabled && node.parent.is_none_or(|parent| self.enabled(parent))
        })
    }

    fn path(&self, target: NodeId) -> Vec<NodeId> {
        let mut path = Vec::new();
        let mut next = Some(target);
        while let Some(id) = next {
            let Some(node) = self.nodes.get(id) else {
                return Vec::new();
            };
            path.push(id);
            next = node.parent;
        }
        path.reverse();
        path
    }

    pub(super) fn deepest_hit(&self, x: f32, y: f32) -> Option<NodeId> {
        self.root
            .and_then(|root| self.hit(root, x, y))
            .filter(|id| self.enabled(*id))
    }

    pub(super) fn click_target(&self, target: NodeId) -> Option<NodeId> {
        self.path(target)
            .into_iter()
            .rev()
            .find(|id| self.enabled(*id) && self.nodes[*id].click.is_some())
    }

    fn dispatch(&mut self, target: NodeId, kind: EventKind) -> DispatchOutcome {
        if !self.enabled(target) {
            return DispatchOutcome::default();
        }
        let path = self.path(target);
        let event = UiEvent {
            kind,
            position: self.interaction.pointer,
            modifiers: self.interaction.modifiers,
        };
        let mut context = EventContext {
            target,
            current_target: target,
            phase: EventPhase::Target,
            propagation_stopped: false,
            default_prevented: false,
            capture: None,
            focus: None,
        };
        let ancestors = &path[..path.len() - 1];
        let visits = ancestors
            .iter()
            .map(|id| (*id, EventPhase::Capture))
            .chain(std::iter::once((target, EventPhase::Target)))
            .chain(ancestors.iter().rev().map(|id| (*id, EventPhase::Bubble)));
        crate::batch(|| {
            for (id, phase) in visits {
                context.current_target = id;
                context.phase = phase;
                for (handler_phase, handler) in &self.nodes[id].handlers {
                    if *handler_phase == phase {
                        handler(&event, &mut context);
                    }
                }
                if context.propagation_stopped {
                    break;
                }
            }
        });
        if let Some(capture) = context.capture
            && (capture.is_none()
                || (!self.interaction.buttons.is_empty()
                    && matches!(
                        event.kind,
                        EventKind::PointerDown(_)
                            | EventKind::PointerMove
                            | EventKind::PointerUp(_)
                    )))
        {
            self.set_capture(capture.filter(|id| self.enabled(*id)));
        }
        let focus = context
            .focus
            .filter(|id| self.enabled(*id) && self.nodes[*id].focusable);
        if let Some(focus) = focus {
            self.set_focus(Some(focus));
        }
        DispatchOutcome {
            default_prevented: context.default_prevented,
            focus_requested: focus.is_some(),
        }
    }

    fn set_capture(&mut self, target: Option<NodeId>) {
        let previous = std::mem::replace(&mut self.interaction.captured, target);
        if previous != target
            && let Some(previous) = previous.filter(|id| self.enabled(*id))
        {
            self.dispatch(previous, EventKind::CaptureLost);
        }
    }

    fn set_focus(&mut self, target: Option<NodeId>) {
        if self.interaction.focused != target {
            self.interaction.focused = target;
            self.focus_reveal = target;
            self.interaction.space = None;
            self.present_needed = true;
        }
    }

    fn focus_order(&self) -> Vec<NodeId> {
        fn collect(scene: &Scene, id: NodeId, order: &mut Vec<NodeId>) {
            if !scene.enabled(id) {
                return;
            }
            let node = &scene.nodes[id];
            if node.focusable {
                order.push(id);
            }
            for child in &node.children {
                collect(scene, *child, order);
            }
        }
        let mut order = Vec::new();
        if let Some(root) = self.root {
            collect(self, root, &mut order);
        }
        order
    }

    pub(super) fn reconcile_interaction(&mut self) {
        if self.pressed.is_some_and(|id| !self.enabled(id)) {
            self.pressed = None;
        }
        if self
            .interaction
            .captured
            .is_some_and(|id| !self.enabled(id))
        {
            self.interaction.captured = None;
            self.pressed = None;
        }
        if self.interaction.focused.is_some_and(|id| !self.enabled(id)) {
            self.set_focus(self.focus_order().first().copied());
        }
        if self
            .interaction
            .space
            .is_some_and(|(id, _)| !self.enabled(id))
        {
            self.interaction.space = None;
        }
    }

    pub(super) fn reconcile_hover(&mut self) {
        let next = self
            .interaction
            .pointer
            .and_then(|(x, y)| self.deepest_hit(x, y))
            .map(|id| self.path(id))
            .unwrap_or_default();
        if self.interaction.hovered == next {
            return;
        }
        let old = std::mem::replace(&mut self.interaction.hovered, next.clone());
        self.present_needed = true;
        crate::batch(|| {
            for id in old.iter().rev().filter(|id| !next.contains(id)) {
                self.dispatch(*id, EventKind::PointerLeave);
            }
            for id in next.iter().filter(|id| !old.contains(id)) {
                self.dispatch(*id, EventKind::PointerEnter);
            }
        });
    }

    fn prepare_input(&mut self) {
        if let Some((width, height)) = self.viewport {
            self.flush(width, height);
        }
    }

    pub(crate) fn pointer_move(&mut self, x: f32, y: f32) {
        self.prepare_input();
        crate::batch(|| {
            self.interaction.pointer = Some((x, y));
            self.reconcile_hover();
            if let Some(target) = self.interaction.captured.or_else(|| self.deepest_hit(x, y)) {
                self.dispatch(target, EventKind::PointerMove);
            }
        });
    }

    pub(crate) fn cursor_left(&mut self) {
        self.interaction.pointer = None;
        self.reconcile_hover();
        if self.interaction.captured.is_none() {
            self.cancel_pointer();
        }
    }

    #[cfg(test)]
    pub(crate) fn mouse_button(&mut self, down: bool, button: MouseButton, x: f32, y: f32) -> bool {
        self.mouse_input(down, button, Some((x, y)))
    }

    pub(crate) fn mouse_input(
        &mut self,
        down: bool,
        button: MouseButton,
        position: Option<(f32, f32)>,
    ) -> bool {
        self.prepare_input();
        crate::batch(|| {
            self.interaction.pointer = position;
            self.reconcile_hover();
            if down {
                self.interaction.buttons.insert(button);
            } else {
                self.interaction.buttons.remove(&button);
            }
            let hit = position.and_then(|(x, y)| self.deepest_hit(x, y));
            let target = self.interaction.captured.or(hit);
            let outcome = target.map_or_else(DispatchOutcome::default, |id| {
                self.dispatch(
                    id,
                    if down {
                        EventKind::PointerDown(button)
                    } else {
                        EventKind::PointerUp(button)
                    },
                )
            });
            let prevented = outcome.default_prevented;
            if !down && self.interaction.buttons.is_empty() {
                self.set_capture(None);
            }
            if button != MouseButton::Left {
                return false;
            }
            let click = position.and_then(|(x, y)| self.hit_test(x, y));
            if down {
                self.pressed = if prevented { None } else { click };
                if !prevented && !outcome.focus_requested {
                    let focus = hit.and_then(|id| {
                        self.path(id)
                            .into_iter()
                            .rev()
                            .find(|id| self.nodes[*id].focusable)
                    });
                    self.set_focus(focus);
                    self.focus_reveal = None;
                }
                false
            } else {
                let pressed = self.pressed.take();
                if let Some(pressed) =
                    pressed.filter(|pressed| !prevented && Some(*pressed) == click)
                {
                    self.activate(pressed);
                    true
                } else {
                    false
                }
            }
        })
    }

    fn activate(&self, id: NodeId) {
        if let Some(click) = self
            .nodes
            .get(id)
            .filter(|_| self.enabled(id))
            .and_then(|node| node.click.as_ref())
        {
            click();
        }
    }

    pub(super) fn cancel_pointer_interaction(&mut self) {
        if let Some(target) = self.interaction.captured.or(self.pressed) {
            self.dispatch(target, EventKind::PointerCancel);
        }
        self.pressed = None;
        self.interaction.buttons.clear();
        self.set_capture(None);
    }

    pub(crate) fn focus_lost(&mut self) {
        crate::batch(|| {
            self.cancel_pointer();
            self.cursor_left();
            self.set_focus(None);
            self.interaction.held.clear();
            self.interaction.modifiers = ModifiersState::empty();
            self.interaction.composing = false;
        });
    }

    pub(crate) fn modifiers(&mut self, modifiers: ModifiersState) {
        self.interaction.modifiers = modifiers;
    }

    pub(crate) fn pointer_position(&mut self, position: Option<(f32, f32)>) {
        self.interaction.pointer = position;
    }

    pub(crate) fn composing(&mut self, composing: bool) {
        self.interaction.composing = composing;
        if composing {
            self.interaction.space = None;
        }
    }

    pub(crate) fn keyboard(&mut self, input: KeyInput) {
        self.prepare_input();
        let down = input.state == ElementState::Pressed;
        let repeated = if down {
            !self.interaction.held.insert(input.physical_key)
        } else {
            self.interaction.held.remove(&input.physical_key);
            false
        };
        if self.interaction.composing {
            return;
        }
        crate::batch(|| {
            let target = self.interaction.focused.or(self.root);
            let prevented = target.is_some_and(|id| {
                self.dispatch(id, EventKind::Key(input.clone()))
                    .default_prevented
            });
            let space_owner = if !down && input.logical_key == Key::Named(NamedKey::Space) {
                self.interaction.space.take()
            } else {
                None
            };
            if prevented || input.synthetic || (down && (repeated || input.repeat)) {
                return;
            }
            let modifiers = self.interaction.modifiers;
            if modifiers.control_key() || modifiers.alt_key() || modifiers.super_key() {
                return;
            }
            match input.logical_key {
                Key::Named(NamedKey::Tab) if down => {
                    let order = self.focus_order();
                    let current = order
                        .iter()
                        .position(|id| Some(*id) == self.interaction.focused);
                    let next = if modifiers.shift_key() {
                        current
                            .unwrap_or(0)
                            .checked_sub(1)
                            .or_else(|| order.len().checked_sub(1))
                    } else {
                        Some(current.map_or(0, |index| (index + 1) % order.len()))
                    };
                    self.set_focus(next.and_then(|index| order.get(index).copied()));
                }
                Key::Named(NamedKey::Enter) if down => {
                    if let Some(id) = self.interaction.focused {
                        self.activate(id);
                    }
                }
                Key::Named(NamedKey::Space) if down => {
                    self.interaction.space =
                        self.interaction.focused.map(|id| (id, input.physical_key));
                }
                Key::Named(NamedKey::Space) => {
                    if let Some((id, _)) = space_owner.filter(|(id, key)| {
                        Some(*id) == self.interaction.focused && *key == input.physical_key
                    }) {
                        self.activate(id);
                    }
                }
                _ => {}
            }
        });
    }

    pub(crate) fn wheel(&mut self, wheel: WheelInput) {
        self.prepare_input();
        let Some(target) = self
            .interaction
            .pointer
            .and_then(|(x, y)| self.deepest_hit(x, y))
        else {
            return;
        };
        crate::batch(|| {
            if !self
                .dispatch(target, EventKind::Wheel(wheel))
                .default_prevented
            {
                self.scroll_chain(target, wheel.vertical);
            }
        });
    }

    pub(crate) fn pointer_cursor(&self) -> bool {
        self.interaction
            .hovered
            .last()
            .and_then(|id| self.click_target(*id))
            .is_some()
            || self.interaction.captured.is_some()
    }

    pub(super) fn paint_interaction(&self, id: NodeId, paints: &mut Vec<Paint>) {
        let focused = self.interaction.focused == Some(id);
        let hovered = self
            .interaction
            .hovered
            .last()
            .and_then(|id| self.click_target(*id))
            == Some(id);
        if !self.enabled(id) || (!focused && !hovered) {
            return;
        }
        let node = &self.nodes[id];
        let bounds = node.bounds;
        let thickness = 2.0_f32.min(bounds.width / 2.0).min(bounds.height / 2.0);
        let color = if focused {
            [1.0, 0.80, 0.30, 1.0]
        } else {
            [0.65, 0.70, 0.76, 1.0]
        };
        for edge in [
            Bounds {
                height: thickness,
                ..bounds
            },
            Bounds {
                y: bounds.y + bounds.height - thickness,
                height: thickness,
                ..bounds
            },
            Bounds {
                width: thickness,
                ..bounds
            },
            Bounds {
                x: bounds.x + bounds.width - thickness,
                width: thickness,
                ..bounds
            },
        ] {
            paints.push(Paint::Rect {
                bounds: edge,
                clip: node.clip,
                color,
            });
        }
    }
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
