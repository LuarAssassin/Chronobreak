use super::*;
use crate::semantics::SemanticNode;
use crate::theme::ControlStyle;
use crate::{
    ActionKind, ActionResult, Role, SemanticAction, SemanticTheme, ThemeColor, ThemeController,
};

#[derive(Default)]
pub(super) struct Presentation {
    pub explicit_color: bool,
    pub foreground: Option<ThemeColor>,
    pub background: Option<ThemeColor>,
    pub connector: Option<ThemeColor>,
    pub control: Option<ControlStyle>,
    pub role: Option<Role>,
    pub name: Option<String>,
    pub selected: Option<Signal<bool>>,
    pub expanded: Option<Signal<bool>>,
    pub invalid: Option<Signal<bool>>,
    pub toggle: Option<Signal<bool>>,
}

impl Element {
    pub fn label(text: impl Into<String>) -> Self {
        let mut element = Self::text(text);
        element.presentation.role = Some(Role::Label);
        element
    }

    pub fn button(text: impl Into<String>) -> Self {
        let mut element = Self::label(text).padding(8.0).on_click(|| {});
        element.style.padding.top = length(4.0_f32);
        element.style.padding.bottom = length(4.0_f32);
        element.presentation.role = Some(Role::Button);
        element.presentation.control = Some(ControlStyle::Primary);
        element
    }

    pub fn secondary_button(text: impl Into<String>) -> Self {
        let mut element = Self::button(text);
        element.presentation.control = Some(ControlStyle::Secondary);
        element
    }

    /// 核心值切换先于 on_click，且不会被 on_click 替换；prevent_default 明确取消本次默认动作。
    pub fn checkbox(text: impl Into<String>, value: &Signal<bool>) -> Self {
        let mut element = Self::button(text);
        element.presentation.role = Some(Role::Checkbox);
        element.presentation.control = Some(ControlStyle::Checkbox);
        element.presentation.selected = Some(value.clone());
        element.presentation.toggle = Some(value.clone());
        element.style.padding.left = length(30.0_f32);
        element
    }

    /// 展开值控制子内容 visible；标题核心激活与复选框使用相同路径。
    pub fn tree_node(text: impl Into<String>, expanded: &Signal<bool>, content: Self) -> Self {
        Self::column()
            .gap(4.0)
            .child(Self::tree_header(text, expanded))
            .child(
                Self::column()
                    .padding(12.0)
                    .visible(expanded)
                    .child(content),
            )
    }

    /// 可组合的树标题；需要选择状态时对标题调用 selected_when，再组合 visible 内容。
    pub fn tree_header(text: impl Into<String>, expanded: &Signal<bool>) -> Self {
        let mut header = Self::button(text);
        header.presentation.role = Some(Role::TreeItem);
        header.presentation.control = Some(ControlStyle::Tree);
        header.presentation.expanded = Some(expanded.clone());
        header.presentation.toggle = Some(expanded.clone());
        header.style.padding.left = length(30.0_f32);
        header
    }

    pub fn semantic_role(mut self, role: Role) -> Self {
        self.presentation.role = Some(role);
        self
    }

    pub fn accessible_name(mut self, name: impl Into<String>) -> Self {
        self.presentation.name = Some(name.into());
        self
    }

    pub fn selected_when(mut self, selected: &Signal<bool>) -> Self {
        assert!(
            self.presentation.control != Some(ControlStyle::Checkbox),
            "checkbox selection comes from its value Signal"
        );
        self.presentation.selected = Some(selected.clone());
        self
    }

    pub fn invalid_when(mut self, invalid: &Signal<bool>) -> Self {
        self.presentation.invalid = Some(invalid.clone());
        self
    }

    pub fn theme_color(mut self, color: ThemeColor) -> Self {
        self.presentation.foreground = Some(color);
        self.presentation.explicit_color = false;
        self
    }

    pub fn theme_background(mut self, color: ThemeColor) -> Self {
        self.presentation.background = Some(color);
        self.background = None;
        self
    }

    pub fn connector(from: [f32; 2], to: [f32; 2], width: f32) -> Self {
        let mut element = Self::segment(from, to, width, [0.0; 4]);
        element.presentation.connector = Some(ThemeColor::Muted);
        element
    }
}

impl Scene {
    pub(super) fn edit_color(&self, role: ThemeColor, legacy: Color) -> Color {
        if self.theme.is_some() {
            self.palette().color(role)
        } else {
            legacy
        }
    }
    pub(crate) fn set_theme(&mut self, theme: ThemeController) {
        let wake = Arc::downgrade(&self.wake);
        self.theme_subscription = Some(theme.0.subscribe(move |_| {
            if let Some(wake) = wake.upgrade() {
                wake.notify();
            }
        }));
        self.theme = Some(theme);
    }

    pub(crate) fn set_semantics(&mut self, semantics: crate::Semantics) {
        semantics.attach(&self.wake);
        self.semantics = Some(semantics);
    }

    pub(super) fn palette(&self) -> SemanticTheme {
        SemanticTheme::new(self.theme_mode.unwrap_or_default())
    }

    pub(super) fn sync_theme(&mut self) {
        let mode = self.theme.as_ref().map(ThemeController::mode);
        let states: BTreeMap<_, _> = self
            .preorder()
            .into_iter()
            .filter(|id| {
                let presentation = &self.nodes[*id].presentation;
                self.is_visible(*id)
                    && (presentation.selected.is_some()
                        || presentation.expanded.is_some()
                        || presentation.invalid.is_some())
            })
            .map(|id| {
                let presentation = &self.nodes[id].presentation;
                (
                    id,
                    (
                        presentation.selected.as_ref().is_some_and(Signal::get),
                        presentation.expanded.as_ref().is_some_and(Signal::get),
                        presentation.invalid.as_ref().is_some_and(Signal::get),
                    ),
                )
            })
            .collect();
        if self.theme_mode != mode || self.style_states != states {
            self.present_needed = true;
            self.commit_version += 1;
        }
        if self.theme_mode != mode {
            self.theme_generation += 1;
            self.theme_mode = mode;
        }
        self.style_states = states;
    }

    pub(super) fn resolved_color(&self, id: NodeId) -> Color {
        let node = &self.nodes[id];
        let presentation = &node.presentation;
        if presentation.explicit_color {
            return node.color;
        }
        let role = presentation.foreground.unwrap_or_else(|| {
            if !self.enabled(id) {
                ThemeColor::Muted
            } else if presentation.control == Some(ControlStyle::Primary) {
                ThemeColor::OnPrimary
            } else {
                ThemeColor::Text
            }
        });
        if self.theme.is_some()
            || presentation.control.is_some()
            || presentation.foreground.is_some()
        {
            self.palette().color(role)
        } else {
            node.color
        }
    }

    pub(super) fn resolved_style(&self, id: NodeId) -> (Option<Color>, crate::visual::Decoration) {
        let node = &self.nodes[id];
        let presentation = &node.presentation;
        let palette = self.palette();
        let (selected, _, invalid) = self.style_states.get(&id).copied().unwrap_or_default();
        let mut background = presentation.background.map(|role| palette.color(role));
        let mut decoration = node.decoration;
        if let Some(control) = presentation.control {
            if control == ControlStyle::Input && self.theme.is_none() {
                return (node.background, decoration);
            }
            let enabled = self.enabled(id);
            let hovered = self.interaction.hovered.contains(&id) && enabled;
            let pressed = (self.pressed == Some(id)
                || self
                    .interaction
                    .space
                    .as_ref()
                    .is_some_and(|(owner, _)| *owner == id))
                && enabled;
            let role = if !enabled {
                ThemeColor::SurfaceRaised
            } else if pressed || selected {
                ThemeColor::Selection
            } else if control == ControlStyle::Primary {
                ThemeColor::Primary
            } else if hovered {
                ThemeColor::SurfaceRaised
            } else {
                ThemeColor::Surface
            };
            background = background.or(Some(palette.color(role)));
            decoration.radius = decoration.radius.max(4.0);
            if decoration.border.is_none() {
                let role = if invalid {
                    ThemeColor::Error
                } else {
                    ThemeColor::Muted
                };
                decoration.border = Some(crate::Border::new(
                    if hovered || pressed || invalid {
                        2.0
                    } else {
                        1.0
                    },
                    palette.color(role),
                ));
            }
        }
        (node.background.or(background), decoration)
    }

    pub(super) fn paint_control_indicator(&self, id: NodeId, paints: &mut Vec<Paint>) {
        let node = &self.nodes[id];
        let presentation = &node.presentation;
        let (selected, expanded, _) = self.style_states.get(&id).copied().unwrap_or_default();
        if presentation.control != Some(ControlStyle::Checkbox) && selected {
            paints.push(Paint::Rect {
                bounds: Bounds {
                    x: node.bounds.x + 3.0,
                    y: node.bounds.y + 3.0,
                    width: 3.0,
                    height: (node.bounds.height - 6.0).max(0.0),
                },
                clip: node.clip,
                color: self.palette().color(ThemeColor::Muted),
            });
        }
        let Some(_) = &presentation.toggle else {
            return;
        };
        let color = self.palette().color(ThemeColor::Text);
        let square = Bounds {
            x: node.bounds.x + 8.0,
            y: node.bounds.y + (node.bounds.height - 12.0) * 0.5,
            width: 12.0,
            height: 12.0,
        };
        if presentation.expanded.is_some() {
            paints.push(Paint::Rect {
                bounds: Bounds {
                    y: square.y + 5.0,
                    height: 2.0,
                    ..square
                },
                clip: node.clip,
                color,
            });
            if !expanded {
                paints.push(Paint::Rect {
                    bounds: Bounds {
                        x: square.x + 5.0,
                        width: 2.0,
                        ..square
                    },
                    clip: node.clip,
                    color,
                });
            }
        } else {
            paints.push(Paint::Shape {
                bounds: square,
                clip: node.clip,
                color: [0.0; 4],
                radius: 1.0,
                border: Some(crate::Border::new(1.5, color)),
                blur: 0.0,
            });
            if selected {
                paints.push(Paint::Rect {
                    bounds: Bounds {
                        x: square.x + 3.0,
                        y: square.y + 3.0,
                        width: 6.0,
                        height: 6.0,
                    },
                    clip: node.clip,
                    color,
                });
            }
        }
    }

    pub(super) fn preorder(&self) -> Vec<NodeId> {
        fn collect(scene: &Scene, id: NodeId, ids: &mut Vec<NodeId>) {
            ids.push(id);
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

    pub(super) fn publish_semantics(&self) {
        let Some(semantics) = &self.semantics else {
            return;
        };
        let mut nodes = Vec::new();
        for id in self
            .preorder()
            .into_iter()
            .filter(|id| self.is_visible(*id))
        {
            let node = &self.nodes[id];
            let presentation = &node.presentation;
            let (selected, expanded, invalid) =
                self.style_states.get(&id).copied().unwrap_or_default();
            let disabled = !self.enabled(id);
            let mut actions = Vec::new();
            if !disabled {
                actions.push(ActionKind::ScrollIntoView);
                if node.focusable {
                    actions.push(ActionKind::Focus);
                }
                if !node.readonly {
                    if node.click.is_some() {
                        actions.push(ActionKind::Activate);
                    }
                    if node.editor.is_some() {
                        actions.push(ActionKind::SetValue);
                    }
                    if presentation.expanded.is_some() {
                        actions.push(if expanded {
                            ActionKind::Collapse
                        } else {
                            ActionKind::Expand
                        });
                    }
                }
            }
            let role = presentation.role.unwrap_or_else(|| {
                if node.scroll.is_some() {
                    Role::ScrollView
                } else if node.click.is_some() {
                    Role::Button
                } else if node.text.is_some() {
                    Role::Label
                } else {
                    Role::Group
                }
            });
            nodes.push(SemanticNode {
                id,
                parent: node.parent,
                children: node
                    .children
                    .iter()
                    .copied()
                    .filter(|child| self.is_visible(*child))
                    .collect(),
                role,
                name: presentation.name.clone().unwrap_or_else(|| {
                    if node.editor.is_some() {
                        String::new()
                    } else {
                        node.text.clone().unwrap_or_default()
                    }
                }),
                value: node
                    .editor
                    .as_ref()
                    .map(|editor| editor.model.text().to_owned())
                    .or_else(|| {
                        if role == Role::Checkbox {
                            presentation.toggle.as_ref().map(|_| selected.to_string())
                        } else {
                            None
                        }
                    }),
                disabled,
                focusable: node.focusable,
                readonly: node.readonly,
                focused: self.interaction.focused == Some(id),
                selected: presentation.selected.as_ref().map(|_| selected),
                expanded: presentation.expanded.as_ref().map(|_| expanded),
                invalid,
                bounds: node.bounds,
                actions,
            });
        }
        semantics.publish(nodes);
    }

    pub(super) fn consume_semantic_actions(&mut self) {
        let Some(semantics) = &self.semantics else {
            return;
        };
        let requests = semantics.requests();
        for (id, action, receipt) in requests {
            let allowed = self.enabled(id);
            let mut applied = false;
            if allowed {
                crate::batch(|| {
                    let node = &self.nodes[id];
                    match action {
                        SemanticAction::Activate if !node.readonly && node.click.is_some() => {
                            self.activate(id);
                            applied = true;
                        }
                        SemanticAction::Focus if node.focusable => {
                            self.set_focus(Some(id));
                            applied = true;
                        }
                        SemanticAction::ScrollIntoView => {
                            self.focus_reveal = Some(id);
                            applied = true;
                        }
                        SemanticAction::SetValue(value)
                            if !node.readonly && node.editor.is_some() =>
                        {
                            applied = self.semantic_text_value(id, &value);
                        }
                        SemanticAction::Expand | SemanticAction::Collapse if !node.readonly => {
                            if let Some(expanded) = &node.presentation.expanded {
                                let requested = matches!(action, SemanticAction::Expand);
                                if expanded.get() != requested {
                                    self.activate(id);
                                }
                                applied = true;
                            }
                        }
                        _ => {}
                    }
                });
                self.sync();
            }
            receipt.finish(if applied {
                ActionResult::Applied
            } else {
                ActionResult::Unavailable
            });
        }
    }
}
