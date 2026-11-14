use super::*;
use crate::{
    ActionResult, Role, SemanticAction, Semantics, ThemeColor, ThemeController, ThemeMode,
};
use winit::{
    event::{ElementState, Ime},
    keyboard::{Key, KeyCode, KeyLocation, NamedKey, PhysicalKey},
};

fn setup(element: Element) -> (Scene, ThemeController, Semantics) {
    let theme = ThemeController::default();
    let semantics = Semantics::default();
    let mut scene = Scene::new(element);
    scene.set_theme(theme.clone());
    scene.set_semantics(semantics.clone());
    scene.flush(400.0, 600.0);
    (scene, theme, semantics)
}

fn tap(scene: &mut Scene, logical: NamedKey) {
    for state in [ElementState::Pressed, ElementState::Released] {
        scene.keyboard(KeyInput {
            logical_key: Key::Named(logical),
            text: None,
            physical_key: PhysicalKey::Code(match logical {
                NamedKey::Tab => KeyCode::Tab,
                NamedKey::Space => KeyCode::Space,
                _ => KeyCode::Enter,
            }),
            location: KeyLocation::Standard,
            state,
            repeat: false,
            synthetic: false,
        });
    }
    scene.flush(400.0, 600.0);
}

#[test]
fn checkbox_mouse_enter_space_and_semantic_share_one_core_then_callback() {
    let value = Signal::new(false);
    let callbacks = Signal::new(0);
    let observed = value.clone();
    let calls = callbacks.clone();
    let (mut scene, _, semantics) = setup(Element::checkbox("check", &value).on_click(move || {
        assert_eq!(observed.get(), calls.get() % 2 == 0);
        calls.update(|count| *count += 1);
    }));
    let id = scene.root.unwrap();
    scene.pointer_button(true, 8.0, 8.0);
    scene.pointer_button(false, 8.0, 8.0);
    tap(&mut scene, NamedKey::Enter);
    tap(&mut scene, NamedKey::Space);
    let receipt = semantics.perform(id, SemanticAction::Activate);
    scene.flush(400.0, 600.0);
    assert_eq!(receipt.result(), Some(ActionResult::Applied));
    assert_eq!(callbacks.get(), 4);
    assert!(!value.get());
    assert_eq!(semantics.snapshot().nodes[0].selected, Some(false));
    assert_eq!(
        semantics.snapshot().nodes[0].value.as_deref(),
        Some("false")
    );
}

#[test]
fn prevent_default_is_explicit_override_but_stop_propagation_is_not() {
    for prevent in [false, true] {
        let value = Signal::new(false);
        let (mut scene, _, _) = setup(Element::checkbox("check", &value).on_event(
            EventPhase::Target,
            move |_, context| {
                context.stop_propagation();
                if prevent {
                    context.prevent_default();
                }
            },
        ));
        scene.pointer_button(true, 8.0, 8.0);
        scene.pointer_button(false, 8.0, 8.0);
        assert_eq!(value.get(), !prevent);
    }
}

#[test]
fn disable_readonly_parent_disable_reject_all_value_actions() {
    for mode in 0..3 {
        let value = Signal::new(false);
        let control = Element::checkbox("check", &value)
            .disabled(mode == 0)
            .readonly(mode == 1);
        let (mut scene, _, semantics) = setup(Element::column().disabled(mode == 2).child(control));
        let id = semantics
            .snapshot()
            .nodes
            .iter()
            .find(|node| node.role == Role::Checkbox)
            .unwrap()
            .id;
        let receipt = semantics.perform(id, SemanticAction::Activate);
        scene.flush(400.0, 600.0);
        assert_eq!(receipt.result(), Some(ActionResult::Unavailable));
        scene.pointer_button(true, 8.0, 8.0);
        scene.pointer_button(false, 8.0, 8.0);
        tap(&mut scene, NamedKey::Tab);
        tap(&mut scene, NamedKey::Enter);
        tap(&mut scene, NamedKey::Space);
        assert!(!value.get());
    }
}

#[test]
fn button_secondary_and_label_have_real_roles_and_actions() {
    let count = Signal::new(0);
    let increment = count.clone();
    let (mut scene, _, semantics) = setup(
        Element::column()
            .child(Element::label("label"))
            .child(
                Element::button("primary").on_click(move || increment.update(|value| *value += 1)),
            )
            .child(Element::secondary_button("secondary")),
    );
    for node in semantics.snapshot().nodes {
        if node.role == Role::Button {
            let receipt = semantics.perform(node.id, SemanticAction::Activate);
            scene.flush(400.0, 600.0);
            assert_eq!(receipt.result(), Some(ActionResult::Applied));
        } else {
            assert!(!node.actions.contains(&crate::ActionKind::Activate));
        }
    }
    assert_eq!(count.get(), 1);
}

#[test]
fn semantic_text_value_is_committed_normalized_edit_with_undo() {
    let value = Signal::new("before".to_owned());
    let (mut scene, _, semantics) = setup(Element::text_input(&value).accessible_name("field"));
    let id = scene.root.unwrap();
    let receipt = semantics.perform(id, SemanticAction::SetValue("after\nline".into()));
    scene.flush(400.0, 600.0);
    assert_eq!(receipt.result(), Some(ActionResult::Applied));
    assert_eq!(
        semantics.snapshot().nodes[0].value.as_deref(),
        Some("after line")
    );
    let model = &mut scene.nodes[id].editor.as_mut().unwrap().model;
    assert!(model.undo());
    assert_eq!(model.text(), "before");
}

#[test]
fn snapshot_ids_hide_reorder_unmount_and_foreign_actions() {
    let shown = Signal::new(true);
    let (root, children) = Element::keyed_column();
    let (mut scene, _, semantics) = setup(root);
    children.insert("a", Element::button("a").visible(&shown));
    children.insert("b", Element::label("b"));
    scene.flush(400.0, 600.0);
    let first = children.node("a").unwrap();
    let second = children.node("b").unwrap();
    let parent = scene.root.unwrap();
    children.reorder(&[second, first]);
    scene.flush(400.0, 600.0);
    assert_eq!(semantics.snapshot().nodes[0].children, [second, first]);
    semantics.take_delta();
    shown.update(|value| *value = false);
    scene.flush(400.0, 600.0);
    assert!(semantics.take_delta().removed.contains(&first));
    assert!(
        !semantics
            .snapshot()
            .nodes
            .iter()
            .any(|node| node.id == first)
    );
    shown.update(|value| *value = true);
    scene.flush(400.0, 600.0);
    assert_eq!(children.node("a"), Some(first));
    assert_eq!(
        semantics
            .snapshot()
            .nodes
            .iter()
            .find(|node| node.id == first)
            .unwrap()
            .parent,
        Some(parent)
    );
    children.remove(first);
    scene.flush(400.0, 600.0);
    children.insert("a", Element::button("new"));
    scene.flush(400.0, 600.0);
    assert_ne!(children.node("a"), Some(first));
    let receipt = semantics.perform(first, SemanticAction::Activate);
    let foreign = Scene::new(Element::button("foreign")).root.unwrap();
    let other = semantics.perform(foreign, SemanticAction::Focus);
    scene.flush(400.0, 600.0);
    assert_eq!(receipt.result(), Some(ActionResult::Unavailable));
    assert_eq!(other.result(), Some(ActionResult::Unavailable));
}

#[test]
fn tree_expand_collapse_and_offscreen_semantic_focus_reveal() {
    let expanded = Signal::new(true);
    let (scroll, handle) = Element::scroll_view(
        80.0,
        Element::tree_node(
            "tree",
            &expanded,
            Element::column()
                .child(Element::label("spacer").height(240.0))
                .child(Element::button("tail")),
        ),
    );
    let (mut scene, _, semantics) = setup(scroll);
    let tree = semantics
        .snapshot()
        .nodes
        .iter()
        .find(|node| node.role == Role::TreeItem)
        .unwrap()
        .id;
    let tail = semantics
        .snapshot()
        .nodes
        .iter()
        .find(|node| node.name == "tail")
        .unwrap()
        .id;
    assert!(scene.nodes[tail].clip.height <= 0.0);
    semantics.perform(tail, SemanticAction::Focus);
    scene.flush(400.0, 600.0);
    assert!(handle.snapshot().offset > 0.0);
    assert!(
        semantics
            .snapshot()
            .nodes
            .iter()
            .find(|node| node.id == tail)
            .unwrap()
            .focused
    );
    semantics.perform(tree, SemanticAction::Collapse);
    scene.flush(400.0, 600.0);
    assert!(!expanded.get());
    assert!(
        !semantics
            .snapshot()
            .nodes
            .iter()
            .any(|node| node.id == tail)
    );
    semantics.perform(tree, SemanticAction::Expand);
    scene.flush(400.0, 600.0);
    assert!(expanded.get());
    assert!(
        semantics
            .snapshot()
            .nodes
            .iter()
            .any(|node| node.id == tail)
    );
}

#[test]
fn theme_only_change_preserves_identity_selection_history_expansion_scroll_and_tasks() {
    let value = Signal::new("abc".to_owned());
    let expanded = Signal::new(true);
    let progress = Signal::new(0);
    let updates = progress.clone();
    let (sender, receiver) = crate::message_channel(std::num::NonZeroUsize::new(4).unwrap());
    let (viewport, handle) = Element::scroll_view(
        120.0,
        Element::column()
            .child(Element::text_input(&value).accessible_name("field"))
            .child(Element::tree_node(
                "tree",
                &expanded,
                Element::label("long").height(400.0),
            )),
    );
    let (mut scene, theme, semantics) =
        setup(viewport.on_message(receiver, move |step| updates.update(|value| *value = step)));
    let ids = scene.preorder();
    let id = semantics
        .snapshot()
        .nodes
        .iter()
        .find(|node| node.name == "field")
        .unwrap()
        .id;
    semantics.perform(id, SemanticAction::Focus);
    semantics.perform(id, SemanticAction::SetValue("abcdef".into()));
    scene.flush(400.0, 600.0);
    scene.nodes[id]
        .editor
        .as_mut()
        .unwrap()
        .model
        .set_selection(1, 4)
        .unwrap();
    let selection = scene.nodes[id].editor.as_ref().unwrap().model.selection();
    handle.scroll_to(60.0);
    scene.flush(400.0, 600.0);
    let offset = handle.snapshot().offset;
    let history = scene.nodes[id]
        .editor
        .as_ref()
        .unwrap()
        .model
        .history_stats();
    let counts = (
        scene.layouts,
        scene.shape_calls,
        scene.formats,
        scene.placements,
    );
    let text_version = scene.nodes[id].text_version;
    let before = scene.resolved_color(id);
    theme.set(ThemeMode::Dark);
    scene.flush(400.0, 600.0);
    assert_eq!(scene.preorder(), ids);
    assert_eq!(
        (
            scene.layouts,
            scene.shape_calls,
            scene.formats,
            scene.placements
        ),
        counts
    );
    assert_eq!(scene.nodes[id].text_version, text_version);
    assert_ne!(scene.resolved_color(id), before);
    assert_eq!(scene.interaction.focused, Some(id));
    assert_eq!(
        scene.nodes[id].editor.as_ref().unwrap().model.selection(),
        selection
    );
    assert_eq!(
        scene.nodes[id]
            .editor
            .as_ref()
            .unwrap()
            .model
            .history_stats(),
        history
    );
    assert_eq!(handle.snapshot().offset, offset);
    assert!(expanded.get());
    assert_eq!(value.get(), "abcdef");
    sender.try_send(7).unwrap();
    scene.drain_messages();
    assert_eq!(progress.get(), 7);
    assert!(!sender.is_closed());
    assert!(scene.nodes[id].editor.as_mut().unwrap().model.undo());
    assert_eq!(scene.nodes[id].editor.as_ref().unwrap().model.text(), "abc");
}

#[test]
fn theme_preserves_native_composition_owner_epoch_and_committed_semantics() {
    let value = Signal::new("abc".to_owned());
    let (mut scene, theme, semantics) = setup(Element::text_input(&value));
    let id = scene.root.unwrap();
    semantics.perform(id, SemanticAction::Focus);
    scene.flush(400.0, 600.0);
    scene.ime_owner = scene.ime_target();
    scene.ime(Ime::Enabled);
    scene.ime(Ime::Preedit("你好".into(), Some((0, 6))));
    scene.flush(400.0, 600.0);
    let display = scene.nodes[id].text.clone();
    let owner = scene.ime_owner;
    let editor = scene.nodes[id].editor.as_ref().unwrap();
    let epoch = (editor.epoch, editor.ime_session, editor.model.selection());
    let counts = (scene.layouts, scene.shape_calls, scene.formats);
    theme.set(ThemeMode::Dark);
    scene.flush(400.0, 600.0);
    let editor = scene.nodes[id].editor.as_ref().unwrap();
    assert!(editor.has_pending_ime());
    assert_eq!(
        (editor.epoch, editor.ime_session, editor.model.selection()),
        epoch
    );
    assert_eq!(scene.ime_owner, owner);
    assert!(scene.ime_ready);
    assert_eq!(scene.nodes[id].text, display);
    assert_eq!((scene.layouts, scene.shape_calls, scene.formats), counts);
    assert_eq!(semantics.snapshot().nodes[0].value.as_deref(), Some("abc"));
    let blocked = semantics.perform(id, SemanticAction::SetValue("overwrite".into()));
    scene.flush(400.0, 600.0);
    assert_eq!(blocked.result(), Some(ActionResult::Unavailable));
    scene.ime(Ime::Commit("你好".into()));
    scene.flush(400.0, 600.0);
    assert_eq!(
        semantics.snapshot().nodes[0].value.as_deref(),
        Some("abc你好")
    );
}

#[test]
fn semantic_state_and_theme_generations_invalidate_paint_not_metrics() {
    let checked = Signal::new(false);
    let invalid = Signal::new(false);
    let (mut scene, theme, semantics) =
        setup(Element::checkbox("check", &checked).invalid_when(&invalid));
    scene.presented();
    let layouts = scene.layouts;
    let generation = scene.theme_generation;
    checked.update(|value| *value = true);
    invalid.update(|value| *value = true);
    scene.flush(400.0, 600.0);
    assert!(scene.needs_present());
    assert_eq!(scene.theme_generation, generation);
    assert!(semantics.snapshot().nodes[0].invalid);
    assert_eq!(semantics.snapshot().nodes[0].selected, Some(true));
    theme.set(ThemeMode::Dark);
    scene.flush(400.0, 600.0);
    assert_eq!(scene.theme_generation, generation + 1);
    assert_eq!(scene.layouts, layouts);
}

#[test]
fn explicit_app_colors_override_tokens_and_legacy_is_unchanged() {
    let color = crate::srgb8(20, 40, 60);
    let (mut scene, theme, _) = setup(Element::button("override").color(color).background(color));
    let id = scene.root.unwrap();
    theme.set(ThemeMode::Dark);
    scene.flush(400.0, 600.0);
    assert_eq!(scene.frame().texts[&id].color, color);
    assert!(
        matches!(scene.display_list()[0], Paint::Shape { color: actual, .. } if actual == color)
    );
    let mut legacy = Scene::new(Element::text("legacy"));
    legacy.flush(400.0, 600.0);
    let legacy_id = legacy.root.unwrap();
    assert_eq!(
        legacy.frame().texts[&legacy_id].color,
        [0.86, 0.90, 0.96, 1.0]
    );
}

#[test]
fn clipped_full_viewport_focus_ring_is_inside_effective_clip() {
    let (mut scene, _, semantics) = setup(Element::button("full").height(900.0));
    let id = scene.root.unwrap();
    semantics.perform(id, SemanticAction::Focus);
    scene.flush(400.0, 600.0);
    let mut rectangles = Vec::new();
    scene.paint_interaction(id, &mut rectangles);
    assert_eq!(rectangles.len(), 4);
    for paint in rectangles {
        let Paint::Rect {
            bounds,
            clip,
            color,
        } = paint
        else {
            panic!("focus rectangle")
        };
        assert_eq!(bounds.intersect(clip), bounds);
        assert_eq!(color, scene.palette().color(ThemeColor::Focus));
    }
}

#[test]
fn dropped_scene_publishes_removals_and_rejects_queued_actions() {
    let (scene, _, semantics) = setup(Element::button("button"));
    let id = scene.root.unwrap();
    semantics.take_delta();
    let receipt = semantics.perform(id, SemanticAction::Activate);
    drop(scene);
    assert!(semantics.snapshot().nodes.is_empty());
    assert_eq!(semantics.take_delta().removed, [id]);
    assert_eq!(receipt.result(), Some(ActionResult::Unavailable));
}

#[test]
fn control_text_padding_reserves_indicator_and_matches_shaping_clip() {
    let expanded = Signal::new(true);
    let (scene, _, _) = setup(Element::tree_header("long 中文 label", &expanded).width(180.0));
    let id = scene.root.unwrap();
    let (origin, clip) = scene.text_geometry(id);
    assert_eq!(origin.0, scene.nodes[id].bounds.x + 30.0);
    assert_eq!(origin.1, scene.nodes[id].bounds.y + 4.0);
    assert_eq!(clip.width, scene.nodes[id].bounds.width - 38.0);
    assert_eq!(clip.height, scene.nodes[id].bounds.height - 8.0);
}

#[test]
fn live_deltas_coalesce_hide_restore_and_do_not_republish_on_theme_only() {
    let visible = Signal::new(true);
    let (mut scene, theme, semantics) = setup(Element::button("control").visible(&visible));
    let id = scene.root.unwrap();
    semantics.take_delta();
    visible.update(|value| *value = false);
    scene.flush(400.0, 600.0);
    visible.update(|value| *value = true);
    scene.flush(400.0, 600.0);
    let delta = semantics.take_delta();
    assert!(delta.removed.is_empty());
    assert!(delta.changed.is_empty());
    assert_eq!(semantics.snapshot().nodes[0].id, id);
    let version = semantics.snapshot().version;
    theme.set(ThemeMode::Dark);
    scene.flush(400.0, 600.0);
    assert_eq!(semantics.snapshot().version, version);
    let delta = semantics.take_delta();
    assert_eq!(delta.version, version);
    assert!(delta.changed.is_empty() && delta.removed.is_empty());
}

#[test]
fn queued_semantic_actions_revalidate_dynamic_disabled_and_readonly() {
    let value = Signal::new("initial".to_owned());
    let disabled = Signal::new(false);
    let readonly = Signal::new(false);
    let (mut scene, _, semantics) = setup(
        Element::text_input(&value)
            .disabled_when(&disabled)
            .readonly_when(&readonly),
    );
    let id = scene.root.unwrap();
    for disable in [false, true] {
        let receipt = semantics.perform(id, SemanticAction::SetValue("blocked".into()));
        if disable {
            disabled.update(|value| *value = true);
        } else {
            readonly.update(|value| *value = true);
        }
        scene.flush(400.0, 600.0);
        assert_eq!(receipt.result(), Some(ActionResult::Unavailable));
        assert_eq!(value.get(), "initial");
        assert!(
            !semantics.snapshot().nodes[0]
                .actions
                .contains(&crate::ActionKind::SetValue)
        );
    }
}

#[test]
fn semantic_action_queue_is_bounded_and_recovers_after_consumption() {
    let (scene, _, semantics) = setup(Element::button("button"));
    let id = scene.root.unwrap();
    for _ in 0..crate::semantics::MAX_PENDING_ACTIONS {
        let receipt = semantics.perform(id, SemanticAction::Activate);
        assert_eq!(receipt.result(), None);
    }
    let rejected = semantics.perform(id, SemanticAction::Activate);
    assert_eq!(rejected.result(), Some(ActionResult::Unavailable));
    let consumed = semantics.requests();
    assert_eq!(consumed.len(), crate::semantics::MAX_PENDING_ACTIONS);
    drop(consumed);
    let accepted = semantics.perform(id, SemanticAction::Activate);
    assert_eq!(accepted.result(), None);
}

#[test]
fn theme_and_semantic_requests_wake_the_installed_application_boundary() {
    let (mut scene, theme, semantics) = setup(Element::button("button"));
    let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = wakes.clone();
    scene.wake.install(Arc::new(move || {
        observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        true
    }));
    theme.set(ThemeMode::Dark);
    assert_eq!(wakes.load(std::sync::atomic::Ordering::SeqCst), 1);
    scene.drain_messages();
    semantics.perform(scene.root.unwrap(), SemanticAction::Focus);
    assert_eq!(wakes.load(std::sync::atomic::Ordering::SeqCst), 2);
    scene.flush(400.0, 600.0);
    assert!(semantics.snapshot().nodes[0].focused);
}

#[test]
fn selected_tree_header_restyles_and_semantics_without_layout_or_remount() {
    let selected = Signal::new(false);
    let expanded = Signal::new(true);
    let (mut scene, _, semantics) =
        setup(Element::tree_header("node", &expanded).selected_when(&selected));
    let id = scene.root.unwrap();
    let before = (
        scene.layouts,
        scene.shape_calls,
        scene.nodes[id].text_version,
    );
    selected.update(|value| *value = true);
    scene.flush(400.0, 600.0);
    assert_eq!(
        (
            scene.layouts,
            scene.shape_calls,
            scene.nodes[id].text_version
        ),
        before
    );
    assert_eq!(
        scene.resolved_style(id).0,
        Some(scene.palette().color(ThemeColor::Selection))
    );
    assert_eq!(semantics.snapshot().nodes[0].selected, Some(true));
    assert!(matches!(
        scene.display_list().last(),
        Some(Paint::Rect { .. })
    ));
}

#[test]
fn preedit_and_caret_paint_resolve_new_theme_without_editor_invalidation() {
    let value = Signal::new("abc".to_owned());
    let (mut scene, theme, semantics) = setup(Element::text_input(&value));
    let id = scene.root.unwrap();
    semantics.perform(id, SemanticAction::Focus);
    scene.flush(400.0, 600.0);
    scene.ime_owner = scene.ime_target();
    scene.ime(Ime::Enabled);
    scene.ime(Ime::Preedit("你好".into(), Some((0, 6))));
    theme.set(ThemeMode::Dark);
    scene.flush(400.0, 600.0);
    let mut paints = Vec::new();
    scene.paint_text_selection(id, &mut paints);
    for role in [ThemeColor::Focus, ThemeColor::Selection] {
        assert!(paints.iter().any(|paint| matches!(paint, Paint::Rect { color, .. } if *color == scene.palette().color(role))));
    }
    paints.clear();
    scene.paint_text_caret(id, &mut paints);
    assert!(!paints.is_empty());
    assert!(paints.iter().all(|paint| matches!(paint, Paint::Rect { color, .. } if *color == scene.palette().color(ThemeColor::Text))));
}

#[test]
fn unconsumed_semantics_retains_snapshots_not_every_transient_node_identity() {
    let (element, children) = Element::keyed_column();
    let (mut scene, _, semantics) = setup(element);
    let version = semantics.take_delta().version;
    for _ in 0..100 {
        children.insert("transient", Element::button("transient"));
        scene.flush(400.0, 600.0);
        children.remove(children.node("transient").unwrap());
        scene.flush(400.0, 600.0);
    }
    let delta = semantics.take_delta();
    assert!(delta.version > version);
    assert!(delta.changed.is_empty() && delta.removed.is_empty());
    assert_eq!(semantics.snapshot().nodes.len(), 1);
}

#[test]
fn primary_and_secondary_route_mouse_enter_space_and_focus_opt_out() {
    for secondary in [false, true] {
        let count = Signal::new(0);
        let clicks = count.clone();
        let element = if secondary {
            Element::secondary_button("action")
        } else {
            Element::button("action")
        };
        let (mut scene, _, _) = setup(element.on_click(move || clicks.update(|value| *value += 1)));
        scene.pointer_button(true, 8.0, 8.0);
        scene.pointer_button(false, 8.0, 8.0);
        tap(&mut scene, NamedKey::Enter);
        tap(&mut scene, NamedKey::Space);
        assert_eq!(count.get(), 3);
    }
    let value = Signal::new(false);
    let (mut scene, _, semantics) = setup(Element::checkbox("mouse only", &value).focusable(false));
    scene.pointer_button(true, 8.0, 8.0);
    scene.pointer_button(false, 8.0, 8.0);
    assert!(value.get());
    tap(&mut scene, NamedKey::Tab);
    tap(&mut scene, NamedKey::Enter);
    tap(&mut scene, NamedKey::Space);
    assert!(value.get());
    assert!(!semantics.snapshot().nodes[0].focusable);
}

#[test]
fn installing_semantics_does_not_drain_new_formatter_commands_in_same_flush() {
    let (container, children) = Element::keyed_column();
    let queued = children.clone();
    let value = Signal::new(0);
    let label = Element::bound_text(&value, move |number| {
        if number == 1 {
            queued.insert("next-boundary", Element::label("deferred"));
        }
        number.to_string()
    });
    let (mut scene, _, _) = setup(Element::column().child(container).child(label));
    value.update(|value| *value = 1);
    scene.flush(400.0, 600.0);
    assert!(children.node("next-boundary").is_none());
    scene.flush(400.0, 600.0);
    assert!(children.node("next-boundary").is_some());
}

#[test]
fn color_states_and_semantics_share_the_formal_commit_snapshot() {
    let value = Signal::new(false);
    let (mut scene, _, semantics) = setup(Element::checkbox("check", &value));
    let id = scene.root.unwrap();
    let before = scene.resolved_style(id).0;
    value.update(|value| *value = true);
    assert_eq!(scene.resolved_style(id).0, before);
    assert_eq!(semantics.snapshot().nodes[0].selected, Some(false));
    scene.flush(400.0, 600.0);
    assert_ne!(scene.resolved_style(id).0, before);
    assert_eq!(semantics.snapshot().nodes[0].selected, Some(true));
    assert_eq!(scene.geometry_version, scene.commit_version);
}
