use super::*;
use std::cell::Cell;
use winit::keyboard::KeyCode;

fn scene(root: Element) -> Scene {
    let mut scene = Scene::new(root);
    scene.flush(400.0, 600.0);
    scene
}

fn button(count: &Signal<usize>) -> Element {
    let count = count.clone();
    Element::text("button")
        .padding(8.0)
        .on_click(move || count.update(|count| *count += 1))
}

fn key(logical: NamedKey, down: bool) -> KeyInput {
    let physical = match logical {
        NamedKey::Tab => KeyCode::Tab,
        NamedKey::Enter => KeyCode::Enter,
        NamedKey::Space => KeyCode::Space,
        _ => KeyCode::Escape,
    };
    KeyInput {
        logical_key: Key::Named(logical),
        physical_key: PhysicalKey::Code(physical),
        location: KeyLocation::Standard,
        state: if down {
            ElementState::Pressed
        } else {
            ElementState::Released
        },
        repeat: false,
        synthetic: false,
    }
}

fn tap(scene: &mut Scene, logical: NamedKey) {
    scene.keyboard(key(logical, true));
    scene.keyboard(key(logical, false));
}

#[test]
fn exact_three_level_route_keeps_deepest_target_and_current_target() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let listen = |mut element: Element, name| {
        for phase in [EventPhase::Capture, EventPhase::Target, EventPhase::Bubble] {
            let trace = trace.clone();
            element = element.on_event(phase, move |event, context| {
                if matches!(event.kind, EventKind::PointerDown(_)) {
                    trace.borrow_mut().push((
                        name,
                        context.phase,
                        context.target,
                        context.current_target,
                    ));
                }
            });
        }
        element
    };
    let count = Signal::new(0);
    let mut scene = scene(
        listen(Element::column(), "root")
            .child(listen(button(&count), "parent").child(listen(Element::text("label"), "leaf"))),
    );
    let root = scene.root.unwrap();
    let parent = scene.nodes[root].children[0];
    let leaf = scene.nodes[parent].children[0];
    let bounds = scene.nodes[leaf].bounds;
    scene.pointer_button(true, bounds.x + 1.0, bounds.y + 1.0);
    assert_eq!(
        *trace.borrow(),
        vec![
            ("root", EventPhase::Capture, leaf, root),
            ("parent", EventPhase::Capture, leaf, parent),
            ("leaf", EventPhase::Target, leaf, leaf),
            ("parent", EventPhase::Bubble, leaf, parent),
            ("root", EventPhase::Bubble, leaf, root),
        ]
    );
    assert!(scene.pointer_button(false, bounds.x + 1.0, bounds.y + 1.0));
    assert_eq!(count.get(), 1);
    assert_eq!(scene.interaction.focused, Some(parent));
}

#[test]
fn propagation_and_default_are_independent_and_controls_do_not_double_activate() {
    for stop in [false, true] {
        for prevent in [false, true] {
            let outer = Signal::new(0);
            let inner = Signal::new(0);
            let visits = Rc::new(Cell::new(0));
            let seen = visits.clone();
            let mut scene = scene(
                button(&outer)
                    .on_event(EventPhase::Capture, move |event, context| {
                        if matches!(
                            event.kind,
                            EventKind::PointerDown(_) | EventKind::PointerUp(_)
                        ) {
                            if stop {
                                context.stop_propagation();
                            }
                            if prevent {
                                context.prevent_default();
                            }
                            assert_eq!(context.propagation_stopped(), stop);
                            assert_eq!(context.default_prevented(), prevent);
                        }
                    })
                    .child(
                        button(&inner).on_event(EventPhase::Target, move |event, _| {
                            if matches!(event.kind, EventKind::PointerDown(_)) {
                                seen.set(seen.get() + 1);
                            }
                        }),
                    ),
            );
            let child = scene.nodes[scene.root.unwrap()].children[0];
            let bounds = scene.nodes[child].bounds;
            scene.pointer_button(true, bounds.x + 1.0, bounds.y + 1.0);
            scene.pointer_button(false, bounds.x + 1.0, bounds.y + 1.0);
            assert_eq!(inner.get(), usize::from(!prevent));
            assert_eq!(outer.get(), 0);
            assert_eq!(visits.get(), usize::from(!stop));
            assert_eq!(
                scene.interaction.focused,
                if prevent { None } else { Some(child) }
            );
        }
    }
}

#[test]
fn handler_structure_commands_are_deferred_through_bubble_and_default() {
    let (list, children) = Element::keyed_column();
    let count = Signal::new(0);
    let receipt = Rc::new(RefCell::new(None::<crate::CommandReceipt>));
    let saved = receipt.clone();
    let remove = children.clone();
    let seen_bubble = Rc::new(Cell::new(false));
    let seen = seen_bubble.clone();
    let mut scene = scene(list.on_event(EventPhase::Bubble, move |event, _| {
        if matches!(event.kind, EventKind::PointerDown(_)) {
            assert!(saved.borrow().as_ref().unwrap().result().is_none());
            seen.set(true);
        }
    }));
    children.insert(
        "old",
        button(&count).on_event(EventPhase::Target, move |event, context| {
            if matches!(event.kind, EventKind::PointerDown(_)) {
                *receipt.borrow_mut() = Some(remove.remove(context.current_target));
            }
        }),
    );
    scene.flush(400.0, 600.0);
    let old = children.node("old").unwrap();
    scene.pointer_button(true, 1.0, 1.0);
    assert!(seen_bubble.get());
    assert!(scene.nodes.get(old).is_some());
    scene.flush(400.0, 600.0);
    assert!(scene.nodes.get(old).is_none());
    children.insert("old", button(&count));
    scene.flush(400.0, 600.0);
    assert_ne!(children.node("old"), Some(old));
    assert!(!scene.pointer_button(false, 1.0, 1.0));
    assert_eq!(count.get(), 0);
}

fn drag(
    count: &Signal<usize>,
    events: &Rc<RefCell<Vec<EventKind>>>,
    release_on_move: bool,
) -> Element {
    let events = events.clone();
    button(count).on_event(EventPhase::Target, move |event, context| {
        match event.kind {
            EventKind::PointerDown(MouseButton::Left) => context.capture_pointer(),
            EventKind::PointerMove if release_on_move => context.release_pointer(),
            _ => {}
        }
        events.borrow_mut().push(event.kind.clone());
    })
}

#[test]
fn capture_delivers_outside_moves_and_up_but_not_release_anywhere_clicks() {
    let count = Signal::new(0);
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut scene = scene(drag(&count, &events, false).max_width(100.0));
    let root = scene.root.unwrap();
    scene.pointer_button(true, 1.0, 1.0);
    assert_eq!(scene.interaction.captured, Some(root));
    scene.cursor_left();
    assert_eq!(scene.interaction.captured, Some(root));
    assert_eq!(scene.pressed, Some(root));
    scene.pointer_move(450.0, 700.0);
    assert!(!scene.pointer_button(false, 450.0, 700.0));
    assert_eq!(count.get(), 0);
    assert!(scene.interaction.captured.is_none());
    let events = events.borrow();
    assert!(events.contains(&EventKind::PointerMove));
    assert!(events.contains(&EventKind::PointerUp(MouseButton::Left)));
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == EventKind::CaptureLost)
            .count(),
        1
    );
}

#[test]
fn explicit_release_and_system_loss_clean_capture_and_key_state() {
    let count = Signal::new(0);
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut scene = scene(drag(&count, &events, true));
    scene.pointer_button(true, 1.0, 1.0);
    scene.pointer_move(2.0, 2.0);
    assert!(scene.interaction.captured.is_none());
    assert!(events.borrow().contains(&EventKind::CaptureLost));
    scene.pointer_button(true, 1.0, 1.0);
    scene.keyboard(key(NamedKey::Space, true));
    scene.modifiers(ModifiersState::SHIFT);
    scene.focus_lost();
    assert!(scene.interaction.captured.is_none());
    assert!(scene.interaction.focused.is_none());
    assert!(scene.interaction.hovered.is_empty());
    assert!(scene.interaction.held.is_empty());
    assert!(scene.interaction.modifiers.is_empty());
    assert!(events.borrow().contains(&EventKind::PointerCancel));
    scene.keyboard(key(NamedKey::Space, false));
    assert_eq!(count.get(), 0);
}

#[test]
fn committed_visibility_disabled_and_unmount_clear_old_interactions() {
    for mode in 0..3 {
        let (list, children) = Element::keyed_column();
        let shown = Signal::new(true);
        let disabled = Signal::new(false);
        let count = Signal::new(0);
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut scene = scene(list.child(button(&count)));
        children.insert(
            "drag",
            drag(&count, &events, false)
                .visible(&shown)
                .disabled_when(&disabled),
        );
        scene.flush(400.0, 600.0);
        let old = children.node("drag").unwrap();
        let bounds = scene.nodes[old].bounds;
        scene.pointer_button(true, bounds.x + 1.0, bounds.y + 1.0);
        scene.keyboard(key(NamedKey::Space, true));
        let before = events.borrow().len();
        match mode {
            0 => shown.update(|shown| *shown = false),
            1 => disabled.update(|disabled| *disabled = true),
            _ => {
                children.remove(old);
            }
        }
        scene.flush(400.0, 600.0);
        assert!(scene.interaction.captured.is_none());
        assert!(scene.pressed.is_none());
        assert!(scene.interaction.space.is_none());
        assert!(!scene.interaction.hovered.contains(&old));
        assert_eq!(
            scene.interaction.focused,
            Some(scene.nodes[scene.root.unwrap()].children[0])
        );
        assert_eq!(events.borrow().len(), before);
        if mode == 2 {
            children.insert("drag", drag(&count, &events, false));
            scene.flush(400.0, 600.0);
            assert_ne!(children.node("drag"), Some(old));
        }
        assert!(!scene.pointer_button(false, bounds.x + 1.0, bounds.y + 1.0));
        scene.keyboard(key(NamedKey::Space, false));
        assert_eq!(count.get(), 0);
    }
}

#[test]
fn tab_order_skips_hidden_disabled_parents_and_explicit_opt_out() {
    let count = Signal::new(0);
    let shown = Signal::new(false);
    let mut scene = scene(
        Element::column()
            .child(button(&count))
            .child(Element::column().disabled(true).child(button(&count)))
            .child(button(&count).visible(&shown))
            .child(button(&count).focusable(false))
            .child(Element::text("focus only").focusable(true)),
    );
    let order = scene.focus_order();
    assert_eq!(order.len(), 2);
    tap(&mut scene, NamedKey::Tab);
    assert_eq!(scene.interaction.focused, Some(order[0]));
    tap(&mut scene, NamedKey::Tab);
    assert_eq!(scene.interaction.focused, Some(order[1]));
    tap(&mut scene, NamedKey::Tab);
    assert_eq!(scene.interaction.focused, Some(order[0]));
    scene.modifiers(ModifiersState::SHIFT);
    tap(&mut scene, NamedKey::Tab);
    assert_eq!(scene.interaction.focused, Some(order[1]));
    scene.set_focus(None);
    tap(&mut scene, NamedKey::Tab);
    assert_eq!(scene.interaction.focused, Some(order[1]));
    scene.pointer_button(
        true,
        1.0,
        scene.nodes[scene.nodes[scene.root.unwrap()].children[1]]
            .bounds
            .y
            + 1.0,
    );
    assert_ne!(
        scene.interaction.focused,
        Some(scene.nodes[scene.nodes[scene.root.unwrap()].children[1]].children[0])
    );
    let mut empty = self::scene(Element::column());
    tap(&mut empty, NamedKey::Tab);
    empty.modifiers(ModifiersState::SHIFT);
    tap(&mut empty, NamedKey::Tab);
    assert!(empty.interaction.focused.is_none());
}

#[test]
fn reorder_preserves_focus_identity_and_changes_subsequent_tab_order() {
    let (list, children) = Element::keyed_column();
    let count = Signal::new(0);
    let mut scene = scene(list);
    for name in ["a", "b", "c"] {
        children.insert(name, button(&count));
    }
    scene.flush(400.0, 600.0);
    let first = children.node("a").unwrap();
    let second = children.node("b").unwrap();
    let third = children.node("c").unwrap();
    tap(&mut scene, NamedKey::Tab);
    children.reorder(&[third, first, second]);
    scene.flush(400.0, 600.0);
    assert_eq!(scene.interaction.focused, Some(first));
    tap(&mut scene, NamedKey::Tab);
    assert_eq!(scene.interaction.focused, Some(second));
    children.remove(second);
    scene.flush(400.0, 600.0);
    assert_eq!(scene.interaction.focused, Some(third));
}

#[test]
fn enter_and_space_activate_once_per_gesture_independent_of_pointer() {
    let count = Signal::new(0);
    let mut scene = scene(button(&count));
    scene.pointer_move(900.0, 900.0);
    tap(&mut scene, NamedKey::Tab);
    for logical in [NamedKey::Enter, NamedKey::Space] {
        let before = count.get();
        scene.keyboard(key(logical, true));
        let mut repeat = key(logical, true);
        repeat.repeat = true;
        scene.keyboard(repeat);
        scene.keyboard(key(logical, true));
        assert_eq!(
            count.get(),
            before + usize::from(logical == NamedKey::Enter)
        );
        scene.keyboard(key(logical, false));
        scene.keyboard(key(logical, false));
        assert_eq!(count.get(), before + 1);
    }
}

#[test]
fn space_focus_transfer_even_back_to_original_never_activates_new_owner() {
    let count = Signal::new(0);
    let mut scene = scene(
        Element::column()
            .child(button(&count))
            .child(button(&count)),
    );
    tap(&mut scene, NamedKey::Tab);
    let original = scene.interaction.focused;
    scene.keyboard(key(NamedKey::Space, true));
    tap(&mut scene, NamedKey::Tab);
    scene.keyboard(key(NamedKey::Space, true));
    tap(&mut scene, NamedKey::Tab);
    assert_eq!(scene.interaction.focused, original);
    scene.keyboard(key(NamedKey::Space, false));
    assert_eq!(count.get(), 0);
    tap(&mut scene, NamedKey::Space);
    assert_eq!(count.get(), 1);
}

#[test]
fn key_prevent_default_cancels_tab_enter_and_space_but_stop_does_not() {
    for prevent in [false, true] {
        let count = Signal::new(0);
        let mut scene = scene(button(&count).on_event(
            EventPhase::Target,
            move |event, context| {
                if matches!(event.kind, EventKind::Key(_)) {
                    context.stop_propagation();
                    if prevent {
                        context.prevent_default();
                    }
                }
            },
        ));
        tap(&mut scene, NamedKey::Tab);
        assert_eq!(scene.interaction.focused.is_none(), prevent);
        scene.set_focus(scene.root);
        tap(&mut scene, NamedKey::Enter);
        tap(&mut scene, NamedKey::Space);
        assert_eq!(count.get(), if prevent { 0 } else { 2 });
    }
}

#[test]
fn payload_preserves_logical_physical_location_modifiers_repeat_and_synthetic() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    let mut scene = scene(Element::text("scope").focusable(true).on_event(
        EventPhase::Target,
        move |event, _| {
            if matches!(event.kind, EventKind::Key(_)) {
                log.borrow_mut().push(event.clone());
            }
        },
    ));
    scene.modifiers(ModifiersState::CONTROL | ModifiersState::SHIFT);
    let input = KeyInput {
        logical_key: Key::Character("κ".into()),
        physical_key: PhysicalKey::Code(KeyCode::KeyK),
        location: KeyLocation::Left,
        state: ElementState::Pressed,
        repeat: true,
        synthetic: true,
    };
    scene.keyboard(input.clone());
    let seen = seen.borrow();
    assert_eq!(seen[0].kind, EventKind::Key(input));
    assert_eq!(
        seen[0].modifiers,
        ModifiersState::CONTROL | ModifiersState::SHIFT
    );
}

#[test]
fn composition_guard_blocks_scoped_shortcuts_and_cancels_space_without_claiming_ime() {
    let count = Signal::new(0);
    let keys = Rc::new(Cell::new(0));
    let seen = keys.clone();
    let mut scene = scene(
        button(&count).on_event(EventPhase::Target, move |event, _| {
            if matches!(event.kind, EventKind::Key(_)) {
                seen.set(seen.get() + 1);
            }
        }),
    );
    tap(&mut scene, NamedKey::Tab);
    scene.keyboard(key(NamedKey::Space, true));
    let before = keys.get();
    scene.composing(true);
    tap(&mut scene, NamedKey::Enter);
    scene.keyboard(key(NamedKey::Space, false));
    assert_eq!(keys.get(), before);
    scene.composing(false);
    scene.keyboard(key(NamedKey::Space, false));
    assert_eq!(count.get(), 0);
    scene.modifiers(ModifiersState::CONTROL);
    tap(&mut scene, NamedKey::Enter);
    assert_eq!(count.get(), 0);
    scene.modifiers(ModifiersState::empty());
    let mut synthetic = key(NamedKey::Enter, true);
    synthetic.synthetic = true;
    scene.keyboard(synthetic);
    scene.keyboard(key(NamedKey::Enter, false));
    assert_eq!(count.get(), 0);
}

#[test]
fn focus_and_hover_paint_in_bounds_without_text_relayout_or_idle_frames() {
    let count = Signal::new(0);
    let mut scene = scene(button(&count));
    let layouts = scene.layouts;
    let formats = scene.formats;
    let initial = scene.display_list().len();
    scene.presented();
    tap(&mut scene, NamedKey::Tab);
    scene.flush(400.0, 600.0);
    assert!(scene.needs_present());
    let paints = scene.display_list();
    assert_eq!(paints.len(), initial + 4);
    let bounds = scene.nodes[scene.root.unwrap()].bounds;
    for paint in paints.into_iter().skip(initial) {
        let Paint::Rect {
            bounds: edge,
            clip,
            color,
        } = paint
        else {
            panic!("outline rectangle")
        };
        assert_eq!(clip.intersect(edge), edge);
        assert_eq!(bounds.intersect(edge), edge);
        assert_eq!(color, [1.0, 0.80, 0.30, 1.0]);
    }
    scene.presented();
    scene.pointer_move(1.0, 1.0);
    assert!(scene.pointer_cursor());
    scene.flush(400.0, 600.0);
    scene.presented();
    scene.pointer_move(2.0, 2.0);
    scene.flush(400.0, 600.0);
    assert!(!scene.needs_present());
    assert_eq!((scene.layouts, scene.formats), (layouts, formats));
    scene.cursor_left();
    assert!(!scene.pointer_cursor());
}

#[test]
fn hover_reconciles_under_stationary_pointer_after_geometry_changes() {
    let shown = Signal::new(true);
    let count = Signal::new(0);
    let leaves = Rc::new(Cell::new(0));
    let seen = leaves.clone();
    let mut scene = scene(
        Element::column()
            .child(Element::text("spacer").visible(&shown))
            .child(
                button(&count).on_event(EventPhase::Target, move |event, _| {
                    if event.kind == EventKind::PointerLeave {
                        seen.set(seen.get() + 1);
                    }
                }),
            ),
    );
    let root = scene.root.unwrap();
    let button = scene.nodes[root].children[1];
    let bounds = scene.nodes[button].bounds;
    scene.pointer_move(2.0, bounds.y + bounds.height - 1.0);
    assert_eq!(scene.interaction.hovered.last(), Some(&button));
    shown.update(|shown| *shown = false);
    scene.flush(400.0, 600.0);
    assert!(!scene.interaction.hovered.contains(&button));
    assert_eq!(leaves.get(), 1);
}

#[test]
fn nested_deepest_hit_respects_clip_and_current_geometry_after_commit() {
    let text = Signal::new(String::from("short"));
    let count = Signal::new(0);
    let mut scene = scene(
        Element::column()
            .child(Element::bound_text(&text, |text| text))
            .child(button(&count).child(Element::text("中文标签"))),
    );
    let root = scene.root.unwrap();
    let button = scene.nodes[root].children[1];
    let leaf = scene.nodes[button].children[0];
    let old = scene.nodes[leaf].bounds;
    assert_eq!(scene.deepest_hit(old.x + 1.0, old.y + 1.0), Some(leaf));
    assert_eq!(scene.hit_test(old.x + 1.0, old.y + 1.0), Some(button));
    text.update(|text| *text = "中文自动换行与最新几何快照。".repeat(30));
    scene.pointer_button(true, old.x + 1.0, old.y + 1.0);
    assert_ne!(scene.interaction.focused, Some(button));
    assert_eq!(scene.geometry_version, scene.commit_version);
    scene.flush(100.0, 40.0);
    assert!(scene.deepest_hit(1.0, 41.0).is_none());
    assert!(scene.deepest_hit(101.0, 1.0).is_none());
}

#[test]
fn disabled_subtree_blocks_clickable_ancestor_instead_of_becoming_transparent() {
    let count = Signal::new(0);
    let mut scene =
        scene(button(&count).child(Element::column().disabled(true).child(button(&count))));
    let root = scene.root.unwrap();
    let disabled = scene.nodes[root].children[0];
    let bounds = scene.nodes[disabled].bounds;
    scene.pointer_button(true, bounds.x + 1.0, bounds.y + 1.0);
    assert!(!scene.pointer_button(false, bounds.x + 1.0, bounds.y + 1.0));
    assert_eq!(count.get(), 0);
    assert!(scene.interaction.focused.is_none());
    assert!(!scene.pointer_cursor());
}

#[test]
fn preventing_only_pointer_or_space_release_cancels_activation() {
    let count = Signal::new(0);
    let mut scene = scene(button(&count).on_event(EventPhase::Target, |event, context| {
        if matches!(&event.kind, EventKind::PointerUp(_))
            || matches!(&event.kind, EventKind::Key(input) if input.state == ElementState::Released) {
            context.prevent_default();
        }
    }));
    scene.pointer_button(true, 1.0, 1.0);
    assert!(scene.interaction.focused.is_some());
    assert!(!scene.pointer_button(false, 1.0, 1.0));
    tap(&mut scene, NamedKey::Space);
    assert_eq!(count.get(), 0);
    assert!(scene.interaction.space.is_none());
}

#[test]
fn non_left_buttons_route_and_release_capture_without_activation() {
    let count = Signal::new(0);
    let mut scene = scene(
        button(&count).on_event(EventPhase::Target, |event, context| {
            if matches!(event.kind, EventKind::PointerDown(_)) {
                context.capture_pointer();
            }
        }),
    );
    scene.mouse_button(true, MouseButton::Right, 1.0, 1.0);
    assert_eq!(scene.interaction.captured, scene.root);
    scene.mouse_button(false, MouseButton::Right, 900.0, 900.0);
    assert!(scene.interaction.captured.is_none());
    assert_eq!(count.get(), 0);
}

#[test]
fn coordinate_less_release_after_cursor_left_reaches_capture_without_fabricated_position() {
    let count = Signal::new(0);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    let mut scene = scene(
        button(&count).on_event(EventPhase::Target, move |event, context| {
            if matches!(event.kind, EventKind::PointerDown(_)) {
                context.capture_pointer();
            }
            if matches!(event.kind, EventKind::PointerUp(_)) {
                log.borrow_mut().push(event.position);
            }
        }),
    );
    scene.pointer_button(true, 1.0, 1.0);
    scene.cursor_left();
    assert!(!scene.mouse_input(false, MouseButton::Left, None));
    assert_eq!(*seen.borrow(), vec![None]);
    assert!(scene.interaction.captured.is_none());
    assert_eq!(count.get(), 0);
}
