use crate::{ActionKind, NodeId, Role, SemanticAction, semantics::SemanticSnapshot};
use accesskit::{Action, ActionData, ActionRequest, Node, Rect, TreeId, TreeInfo, TreeUpdate};
use std::collections::{HashMap, HashSet};

const ROOT: accesskit::NodeId = accesskit::NodeId(0);

#[derive(Default)]
pub(crate) struct Bridge {
    ids: HashMap<NodeId, accesskit::NodeId>,
    next: u64,
}

fn action(kind: ActionKind) -> Action {
    match kind {
        ActionKind::Activate => Action::Click,
        ActionKind::Focus => Action::Focus,
        ActionKind::SetValue => Action::SetValue,
        ActionKind::ScrollIntoView => Action::ScrollIntoView,
        ActionKind::Expand => Action::Expand,
        ActionKind::Collapse => Action::Collapse,
    }
}

impl Bridge {
    pub fn update(&mut self, snapshot: &SemanticSnapshot, title: &str, scale: f64) -> TreeUpdate {
        let live: HashSet<_> = snapshot.nodes.iter().map(|node| node.id).collect();
        self.ids.retain(|id, _| live.contains(id));
        for node in &snapshot.nodes {
            self.ids.entry(node.id).or_insert_with(|| {
                self.next = self
                    .next
                    .checked_add(1)
                    .filter(|next| *next < (1_u64 << 63))
                    .expect("accessibility identities exhausted");
                accesskit::NodeId(self.next)
            });
        }
        let mut root = Node::new(accesskit::Role::Window);
        root.set_label(title);
        root.set_children(
            snapshot
                .nodes
                .iter()
                .filter(|node| node.parent.is_none())
                .map(|node| self.ids[&node.id])
                .collect::<Vec<_>>(),
        );
        let mut nodes = vec![(ROOT, root)];
        let mut focus = ROOT;
        for source in &snapshot.nodes {
            let role = match source.role {
                Role::Group => accesskit::Role::GenericContainer,
                Role::Label => accesskit::Role::Label,
                Role::Button => accesskit::Role::Button,
                Role::TextInput => accesskit::Role::TextInput,
                Role::Checkbox => accesskit::Role::CheckBox,
                Role::ScrollView => accesskit::Role::ScrollView,
                Role::TreeItem => accesskit::Role::TreeItem,
            };
            let mut node = Node::new(role);
            node.set_label(source.name.clone());
            if let Some(value) = &source.value {
                node.set_value(value.clone());
            }
            node.set_children(
                source
                    .children
                    .iter()
                    .filter_map(|id| self.ids.get(id).copied())
                    .collect::<Vec<_>>(),
            );
            if source.disabled {
                node.set_disabled();
            }
            if source.readonly {
                node.set_read_only();
            }
            if source.invalid {
                node.set_invalid(accesskit::Invalid::True);
            }
            if let Some(expanded) = source.expanded {
                node.set_expanded(expanded);
            }
            if let Some(selected) = source.selected {
                if source.role == Role::Checkbox {
                    node.set_toggled(if selected {
                        accesskit::Toggled::True
                    } else {
                        accesskit::Toggled::False
                    });
                } else {
                    node.set_selected(selected);
                }
            }
            let bounds = source.bounds;
            node.set_bounds(Rect::new(
                bounds.x as f64 * scale,
                bounds.y as f64 * scale,
                (bounds.x + bounds.width) as f64 * scale,
                (bounds.y + bounds.height) as f64 * scale,
            ));
            if source.role == Role::TextInput {
                let text_id = accesskit::NodeId(self.ids[&source.id].0 | (1_u64 << 63));
                let value = source.value.as_deref().unwrap_or_default();
                let mut text = Node::new(accesskit::Role::TextRun);
                text.set_value(value);
                text.set_character_lengths(
                    value
                        .chars()
                        .map(|character| character.len_utf8() as u8)
                        .collect::<Vec<_>>(),
                );
                node.push_child(text_id);
                nodes.push((text_id, text));
            }
            for kind in &source.actions {
                node.add_action(action(*kind));
            }
            let id = self.ids[&source.id];
            if source.focused {
                focus = id;
            }
            nodes.push((id, node));
        }
        let mut tree = TreeInfo::new(ROOT);
        tree.toolkit_name = Some("Chronobreak".into());
        tree.toolkit_version = Some(env!("CARGO_PKG_VERSION").into());
        TreeUpdate {
            nodes,
            tree: Some(tree),
            tree_id: TreeId::ROOT,
            focus,
        }
    }

    pub fn request(&self, request: ActionRequest) -> Option<(NodeId, SemanticAction)> {
        if request.target_tree != TreeId::ROOT {
            return None;
        }
        let id = self
            .ids
            .iter()
            .find(|(_, target)| **target == request.target_node)
            .map(|(id, _)| *id)?;
        let action = match request.action {
            Action::Click => SemanticAction::Activate,
            Action::Focus => SemanticAction::Focus,
            Action::SetValue => match request.data {
                Some(ActionData::Value(value)) => SemanticAction::SetValue(value.into()),
                _ => return None,
            },
            Action::ScrollIntoView => SemanticAction::ScrollIntoView,
            Action::Expand => SemanticAction::Expand,
            Action::Collapse => SemanticAction::Collapse,
            _ => return None,
        };
        Some((id, action))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{semantics::SemanticNode, ui::Bounds};

    fn snapshot(id: NodeId) -> SemanticSnapshot {
        SemanticSnapshot {
            version: 1,
            nodes: vec![SemanticNode {
                id,
                parent: None,
                children: vec![],
                role: Role::TextInput,
                name: "source".into(),
                value: Some("中文😀".into()),
                disabled: false,
                focusable: true,
                readonly: false,
                focused: true,
                selected: None,
                expanded: None,
                invalid: false,
                bounds: Bounds {
                    x: 2.0,
                    y: 3.0,
                    width: 10.0,
                    height: 20.0,
                },
                actions: vec![ActionKind::Focus, ActionKind::SetValue],
            }],
        }
    }

    #[test]
    fn exposes_physical_bounds_focus_unicode_and_actions() {
        let mut arena = crate::node::Arena::new();
        let id = arena.insert(());
        let mut bridge = Bridge::default();
        let tree = bridge.update(&snapshot(id), "window", 1.5);
        let node = tree
            .nodes
            .iter()
            .find(|(_, node)| node.role() == accesskit::Role::TextInput)
            .unwrap();
        assert_eq!(tree.focus, node.0);
        assert_eq!(node.1.label(), Some("source"));
        assert_eq!(node.1.bounds(), Some(Rect::new(3.0, 4.5, 18.0, 34.5)));
        assert!(node.1.supports_action(Action::SetValue));
        let text = tree
            .nodes
            .iter()
            .find(|(_, node)| node.role() == accesskit::Role::TextRun)
            .unwrap();
        assert_eq!(text.1.value(), Some("中文😀"));
        assert_eq!(text.1.character_lengths(), &[3, 3, 4]);
        assert!(
            matches!(bridge.request(ActionRequest { action: Action::SetValue, target_tree: TreeId::ROOT, target_node: node.0, data: Some(ActionData::Value("new".into())) }), Some((target, SemanticAction::SetValue(value))) if target == id && value == "new")
        );
    }

    #[test]
    fn removed_and_reused_generations_never_receive_stale_actions() {
        let mut arena = crate::node::Arena::new();
        let old = arena.insert(());
        let mut bridge = Bridge::default();
        bridge.update(&snapshot(old), "window", 1.0);
        let old_target = bridge.ids[&old];
        arena.remove(old);
        let current = arena.insert(());
        bridge.update(&snapshot(current), "window", 1.0);
        assert_ne!(old_target, bridge.ids[&current]);
        assert!(
            bridge
                .request(ActionRequest {
                    action: Action::Focus,
                    target_tree: TreeId::ROOT,
                    target_node: old_target,
                    data: None
                })
                .is_none()
        );
        let empty = bridge.update(&SemanticSnapshot::default(), "window", 1.0);
        assert_eq!(empty.nodes.len(), 1);
        assert_eq!(empty.focus, ROOT);
        assert!(bridge.ids.is_empty());
    }
}
