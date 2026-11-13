use super::*;
use std::cell::Cell;
use winit::keyboard::{KeyCode, KeyLocation, ModifiersState, PhysicalKey};

fn setup(value: &Signal<String>) -> (Scene, NodeId) {
    let mut scene = Scene::new(Element::text_input(value));
    scene.flush(240.0, 100.0);
    tap(&mut scene, Key::Named(NamedKey::Tab), None);
    let id = scene.root.unwrap();
    assert_eq!(scene.interaction.focused, Some(id));
    (scene, id)
}

fn input(logical_key: Key, text: Option<&str>) -> KeyInput {
    KeyInput {
        logical_key,
        text: text.map(str::to_owned),
        physical_key: PhysicalKey::Code(KeyCode::KeyQ),
        location: KeyLocation::Standard,
        state: ElementState::Pressed,
        repeat: false,
        synthetic: false,
    }
}

fn tap(scene: &mut Scene, logical_key: Key, text: Option<&str>) {
    let key = input(logical_key, text);
    scene.keyboard(key.clone());
    scene.keyboard(KeyInput {
        state: ElementState::Released,
        ..key
    });
    scene.flush(240.0, 100.0);
}

fn named(scene: &mut Scene, key: NamedKey) {
    tap(scene, Key::Named(key), None);
}
fn type_text(scene: &mut Scene, text: &str) {
    tap(scene, Key::Character("not-the-payload".into()), Some(text));
}
fn shortcut(scene: &mut Scene, key: &str, shift: bool) {
    scene.modifiers(
        ModifiersState::CONTROL
            | if shift {
                ModifiersState::SHIFT
            } else {
                ModifiersState::empty()
            },
    );
    tap(scene, Key::Character(key.into()), Some("\u{1}"));
    scene.modifiers(ModifiersState::empty());
}
fn editor(scene: &Scene, id: NodeId) -> &Editor {
    scene.nodes[id].editor.as_ref().unwrap()
}
fn ticket(scene: &Scene, id: NodeId, action: ClipboardAction) -> Ticket {
    Ticket {
        id,
        epoch: editor(scene, id).epoch,
        action,
        expires: Instant::now() + Duration::from_secs(5),
    }
}
fn reply(scene: &mut Scene, ticket: Ticket, text: &str) {
    scene.apply_clipboard(ClipboardReply {
        ticket,
        result: Ok(text.to_owned()),
    });
    scene.flush(240.0, 100.0);
}

#[test]
fn normalization_is_field_local() {
    assert_eq!(single_line("a\r\nb\nc\rd\u{2028}e"), "a b c d e");
}

#[test]
fn controlled_edit_publishes_once_and_equal_echo_preserves_history_and_caret() {
    let value = Signal::new("a".to_owned());
    let notifications = Rc::new(Cell::new(0));
    let observed = notifications.clone();
    let _subscription = value.subscribe(move |_| observed.set(observed.get() + 1));
    let (mut scene, id) = setup(&value);
    type_text(&mut scene, "你好");
    assert_eq!(value.get(), "a你好");
    assert_eq!(notifications.get(), 1);
    named(&mut scene, NamedKey::ArrowLeft);
    let selection = editor(&scene, id).model.selection();
    value.update(|text| *text = "a你好".into());
    scene.flush(240.0, 100.0);
    assert_eq!(editor(&scene, id).model.selection(), selection);
    assert!(editor(&scene, id).model.can_undo());
    shortcut(&mut scene, "z", false);
    assert_eq!(value.get(), "a");
    shortcut(&mut scene, "z", true);
    assert_eq!(value.get(), "a你好");
    assert_eq!(notifications.get(), 3);
}

#[test]
fn external_replacement_is_history_barrier_and_grapheme_clamps_endpoints() {
    let value = Signal::new("abc".into());
    let (mut scene, id) = setup(&value);
    type_text(&mut scene, "def");
    value.update(|text| *text = "👩🏽‍💻x".into());
    scene.flush(240.0, 100.0);
    assert_eq!(editor(&scene, id).model.text(), "👩🏽‍💻x");
    assert_eq!(editor(&scene, id).model.selection().focus(), 0);
    assert!(!editor(&scene, id).model.can_undo());
    shortcut(&mut scene, "z", false);
    assert_eq!(value.get(), "👩🏽‍💻x");
}

#[test]
fn routed_programmatic_update_is_seen_before_default_edit() {
    let value = Signal::new("abc".into());
    let change = value.clone();
    let mut scene = Scene::new(Element::text_input(&value).on_event(
        EventPhase::Target,
        move |event, _| {
            if let EventKind::Key(key) = &event.kind
                && key.state == ElementState::Pressed
                && key.text.as_deref() == Some("!")
            {
                change.update(|text| *text = "XYZ".into());
            }
        },
    ));
    scene.flush(240.0, 100.0);
    named(&mut scene, NamedKey::Tab);
    type_text(&mut scene, "!");
    assert_eq!(value.get(), "XYZ!");
    shortcut(&mut scene, "z", false);
    assert_eq!(value.get(), "XYZ");
}

#[test]
fn shared_controlled_value_syncs_other_field_without_focus_or_caret_theft() {
    let value = Signal::new("a".into());
    let mut scene = Scene::new(
        Element::column()
            .child(Element::text_input(&value))
            .child(Element::text_input(&value)),
    );
    scene.flush(240.0, 100.0);
    named(&mut scene, NamedKey::Tab);
    let root = scene.root.unwrap();
    let first = scene.nodes[root].children[0];
    let second = scene.nodes[root].children[1];
    type_text(&mut scene, "b");
    assert_eq!(scene.interaction.focused, Some(first));
    assert_eq!(editor(&scene, first).model.selection().focus(), 2);
    assert_eq!(editor(&scene, second).model.text(), "ab");
    assert_eq!(editor(&scene, second).model.selection().focus(), 1);
    assert!(!editor(&scene, second).model.can_undo());
}

#[test]
fn real_text_payload_not_logical_key_and_repeats_edit_without_button_repeat() {
    let value = Signal::new(String::new());
    let (mut scene, _) = setup(&value);
    let key = input(Key::Character("q".into()), Some("Ω"));
    scene.keyboard(key.clone());
    scene.keyboard(KeyInput {
        repeat: true,
        ..key.clone()
    });
    scene.keyboard(KeyInput {
        state: ElementState::Released,
        ..key
    });
    assert_eq!(value.get(), "ΩΩ");
    let key = input(Key::Named(NamedKey::Backspace), None);
    scene.keyboard(key.clone());
    scene.keyboard(KeyInput {
        repeat: true,
        ..key
    });
    assert_eq!(value.get(), "");
}

#[test]
fn shortcuts_synthetic_composition_and_enter_do_not_insert_control_text() {
    let value = Signal::new("a".into());
    let (mut scene, _) = setup(&value);
    shortcut(&mut scene, "a", false);
    assert_eq!(value.get(), "a");
    named(&mut scene, NamedKey::Enter);
    let key = input(Key::Character("x".into()), Some("x"));
    scene.keyboard(KeyInput {
        synthetic: true,
        ..key.clone()
    });
    scene.composing(true);
    scene.keyboard(key);
    scene.composing(false);
    assert_eq!(value.get(), "a");
    scene.modifiers(ModifiersState::ALT);
    type_text(&mut scene, "x");
    assert_eq!(value.get(), "a");
}

#[test]
fn prevent_default_stops_edit_but_stop_propagation_alone_does_not() {
    for prevent in [true, false] {
        let value = Signal::new("a".into());
        let mut scene = Scene::new(Element::text_input(&value).on_event(
            EventPhase::Target,
            move |event, context| {
                if matches!(event.kind, EventKind::Key(_)) {
                    context.stop_propagation();
                    if prevent
                        && let EventKind::Key(key) = &event.kind
                        && key.text.is_some()
                    {
                        context.prevent_default();
                    }
                }
            },
        ));
        scene.flush(240.0, 100.0);
        named(&mut scene, NamedKey::Tab);
        type_text(&mut scene, "x");
        assert_eq!(value.get(), if prevent { "a" } else { "ax" });
    }
}

#[test]
fn unicode_navigation_selection_deletion_and_history_share_m3a() {
    let value = Signal::new("中e\u{301}👩🏽‍💻".into());
    let (mut scene, id) = setup(&value);
    scene.modifiers(ModifiersState::SHIFT);
    named(&mut scene, NamedKey::ArrowLeft);
    assert_eq!(editor(&scene, id).model.selected_text(), "👩🏽‍💻");
    scene.modifiers(ModifiersState::empty());
    named(&mut scene, NamedKey::Backspace);
    assert_eq!(value.get(), "中e\u{301}");
    named(&mut scene, NamedKey::ArrowLeft);
    named(&mut scene, NamedKey::Delete);
    assert_eq!(value.get(), "中");
    shortcut(&mut scene, "z", false);
    assert_eq!(value.get(), "中e\u{301}");
    shortcut(&mut scene, "y", false);
    assert_eq!(value.get(), "中");
}

#[test]
fn word_arrows_home_end_shift_and_select_all() {
    let value = Signal::new("hello 世界!".into());
    let (mut scene, id) = setup(&value);
    named(&mut scene, NamedKey::Home);
    scene.modifiers(ModifiersState::CONTROL | ModifiersState::SHIFT);
    named(&mut scene, NamedKey::ArrowRight);
    assert_eq!(editor(&scene, id).model.selected_text(), "hello");
    scene.modifiers(ModifiersState::SHIFT);
    named(&mut scene, NamedKey::End);
    assert_eq!(editor(&scene, id).model.selected_text(), value.get());
    scene.modifiers(ModifiersState::empty());
    named(&mut scene, NamedKey::Home);
    shortcut(&mut scene, "a", false);
    assert_eq!(editor(&scene, id).model.selected_text(), value.get());
}

#[test]
fn programmatic_newlines_project_without_writing_signal_or_raw_model() {
    let value = Signal::new("a\r\nb\u{2029}c".into());
    let (mut scene, id) = setup(&value);
    assert_eq!(editor(&scene, id).model.text(), "a b c");
    assert_eq!(value.get(), "a\r\nb\u{2029}c");
    named(&mut scene, NamedKey::Home);
    assert_eq!(value.get(), "a\r\nb\u{2029}c");
    assert_eq!(TextEditModel::new(value.get()).text(), "a\r\nb\u{2029}c");
    let ticket = ticket(&scene, id, ClipboardAction::Paste);
    reply(&mut scene, ticket, "x\r\ny");
    assert_eq!(value.get(), "x ya b c");
}

#[test]
fn shaped_geometry_handles_variable_width_unicode_ligatures_and_bidi() {
    for text in [
        "Wi i",
        "你好e\u{301}👩🏽‍💻",
        "office ffi",
        "abc אבג xyz",
        "العربية abc",
    ] {
        let value = Signal::new(text.to_owned());
        let (scene, id) = setup(&value);
        let geometry = Geometry::new(scene.nodes[id].buffer.as_ref().unwrap(), text);
        let boundaries: Vec<_> = text
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .chain(std::iter::once(text.len()))
            .collect();
        for index in boundaries {
            assert!(
                geometry.stops.iter().any(|stop| stop.index == index),
                "{text:?} missing {index}"
            );
        }
        for stop in &geometry.stops {
            assert!(
                TextEditModel::new(text)
                    .set_selection(stop.index, stop.index)
                    .is_ok()
            );
            assert!((geometry.hit(stop.x).x - stop.x).abs() < 0.01);
        }
        assert!(geometry.spans.iter().all(|span| span.width >= 0.0));
        if text == "Wi i" {
            assert!(
                geometry.caret(1, Affinity::Before)
                    > geometry.caret(2, Affinity::Before) - geometry.caret(1, Affinity::Before)
            );
        }
    }
}

#[test]
fn capture_drag_clamps_outside_line_and_releases_safely() {
    let value = Signal::new("中e\u{301}👩🏽‍💻 abc".into());
    let (mut scene, id) = setup(&value);
    scene.mouse_input(true, winit::event::MouseButton::Left, Some((4.0, 5.0)));
    assert_eq!(scene.interaction.captured, Some(id));
    scene.pointer_move(10000.0, -100.0);
    assert_eq!(
        editor(&scene, id).model.selection().focus(),
        value.get().len()
    );
    scene.pointer_move(-10000.0, 1000.0);
    assert_eq!(editor(&scene, id).model.selection().focus(), 0);
    scene.mouse_input(
        false,
        winit::event::MouseButton::Left,
        Some((-10000.0, 1000.0)),
    );
    assert_eq!(scene.interaction.captured, None);
    assert!(!editor(&scene, id).dragging);
    scene.mouse_input(true, winit::event::MouseButton::Left, Some((10.0, 5.0)));
    scene.cancel_pointer();
    assert!(!editor(&scene, id).dragging);
}

#[test]
fn pointer_default_prevention_blocks_focus_selection_and_capture() {
    let value = Signal::new("abc".into());
    let mut scene = Scene::new(Element::text_input(&value).on_event(
        EventPhase::Target,
        |event, context| {
            if matches!(event.kind, EventKind::PointerDown(_)) {
                context.prevent_default();
            }
        },
    ));
    scene.flush(240.0, 100.0);
    let id = scene.root.unwrap();
    scene.mouse_input(true, winit::event::MouseButton::Left, Some((5.0, 5.0)));
    assert_eq!(scene.interaction.focused, None);
    assert_eq!(scene.interaction.captured, None);
    assert_eq!(editor(&scene, id).model.selection().focus(), 3);
}

#[test]
fn focus_loss_stops_drag_and_blink_and_reentry_restarts_edit_deadline() {
    let value = Signal::new("abc".into());
    let (mut scene, id) = setup(&value);
    scene.mouse_input(true, winit::event::MouseButton::Left, Some((5.0, 5.0)));
    scene.focus_lost();
    assert!(!editor(&scene, id).dragging);
    assert_eq!(scene.interaction.captured, None);
    assert_eq!(scene.blink_deadline(), None);
    named(&mut scene, NamedKey::Tab);
    scene.update_blink(Instant::now(), true);
    let previous = scene.blink_deadline().unwrap();
    named(&mut scene, NamedKey::ArrowRight);
    assert!(scene.blink_deadline().unwrap() >= previous);
    assert!(editor(&scene, id).caret_visible);
}

#[test]
fn long_field_tracks_caret_without_general_horizontal_scroll_or_relayout_on_selection() {
    let value = Signal::new("你好 office 👩🏽‍💻 ".repeat(30));
    let (mut scene, id) = setup(&value);
    assert!(editor(&scene, id).offset > 100.0);
    let layouts = scene.layouts;
    for _ in 0..8 {
        named(&mut scene, NamedKey::ArrowLeft);
    }
    assert_eq!(scene.layouts, layouts);
    let (origin, clip) = scene.text_geometry(id);
    let geometry = Geometry::new(
        scene.nodes[id].buffer.as_ref().unwrap(),
        editor(&scene, id).model.text(),
    );
    let caret = origin.0
        + geometry.caret(
            editor(&scene, id).model.selection().focus(),
            editor(&scene, id).affinity,
        );
    assert!(caret >= clip.x && caret <= clip.x + clip.width);
    named(&mut scene, NamedKey::Home);
    assert_eq!(editor(&scene, id).offset, 0.0);
    scene.flush(75.0, 100.0);
    scene.nodes[id].editor.as_mut().unwrap().model.select_all();
    scene.update_blink(Instant::now(), true);
    let paints = scene.display_list();
    let text = paints
        .iter()
        .position(|paint| matches!(paint, Paint::Text(_)))
        .unwrap();
    assert!(
        paints[..text]
            .iter()
            .any(|paint| matches!(paint, Paint::Rect { clip, .. } if clip.width <= 67.0))
    );
    assert!(
        paints[text + 1..]
            .iter()
            .any(|paint| matches!(paint, Paint::Rect { clip, .. } if clip.x >= 4.0))
    );
    assert_eq!(
        scene.nodes[id]
            .buffer
            .as_ref()
            .unwrap()
            .layout_runs()
            .count(),
        1
    );
}

#[test]
fn readonly_allows_selection_copy_not_mutation_and_dynamic_change_cancels_drag() {
    let value = Signal::new("你好 abc".into());
    let readonly = Signal::new(false);
    let mut scene = Scene::new(Element::text_input(&value).readonly_when(&readonly));
    scene.flush(240.0, 100.0);
    named(&mut scene, NamedKey::Tab);
    let id = scene.root.unwrap();
    type_text(&mut scene, "!");
    scene.mouse_input(true, winit::event::MouseButton::Left, Some((8.0, 8.0)));
    readonly.update(|value| *value = true);
    scene.flush(240.0, 100.0);
    assert!(!editor(&scene, id).dragging);
    assert_eq!(scene.interaction.captured, None);
    shortcut(&mut scene, "a", false);
    assert_eq!(editor(&scene, id).model.selected_text(), "你好 abc!");
    for key in [NamedKey::Backspace, NamedKey::Delete] {
        named(&mut scene, key);
    }
    type_text(&mut scene, "x");
    shortcut(&mut scene, "z", false);
    scene.update_blink(Instant::now(), true);
    assert_eq!(scene.blink_deadline(), None);
    assert_eq!(value.get(), "你好 abc!");
}

#[test]
fn disabled_skips_focus_and_dynamic_disable_cleans_capture_blink_and_edits() {
    let value = Signal::new("abc".into());
    let disabled = Signal::new(false);
    let mut scene = Scene::new(
        Element::column()
            .child(Element::text_input(&value).disabled_when(&disabled))
            .child(Element::text("next").focusable(true)),
    );
    scene.flush(240.0, 100.0);
    named(&mut scene, NamedKey::Tab);
    let root = scene.root.unwrap();
    let id = scene.nodes[root].children[0];
    scene.mouse_input(true, winit::event::MouseButton::Left, Some((8.0, 8.0)));
    disabled.update(|value| *value = true);
    scene.flush(240.0, 100.0);
    assert!(!editor(&scene, id).dragging);
    assert_eq!(scene.interaction.captured, None);
    assert_ne!(scene.interaction.focused, Some(id));
    type_text(&mut scene, "x");
    assert_eq!(value.get(), "abc");
    named(&mut scene, NamedKey::Tab);
    assert_ne!(scene.interaction.focused, Some(id));
}

#[test]
fn lifecycle_releases_subscription_history_and_rejects_old_clipboard_after_same_key_remount() {
    let value = Signal::new("a".into());
    let (root, children) = Element::keyed_column();
    let mut scene = Scene::new(root);
    children.insert("field", Element::text_input(&value));
    scene.flush(240.0, 100.0);
    named(&mut scene, NamedKey::Tab);
    let old = children.node("field").unwrap();
    type_text(&mut scene, "b");
    let ticket = ticket(&scene, old, ClipboardAction::Paste);
    children.remove(old);
    scene.flush(240.0, 100.0);
    assert_eq!(value.subscriber_count(), 0);
    assert!(scene.nodes.get(old).is_none());
    children.insert("field", Element::text_input(&value));
    scene.flush(240.0, 100.0);
    named(&mut scene, NamedKey::Tab);
    reply(&mut scene, ticket, "late");
    assert_eq!(value.get(), "ab");
    let new = children.node("field").unwrap();
    assert_ne!(new, old);
    assert!(!editor(&scene, new).model.can_undo());
}

#[test]
fn clipboard_cut_only_mutates_after_success_and_failed_or_empty_paste_is_noop() {
    let value = Signal::new("abc".into());
    let (mut scene, id) = setup(&value);
    shortcut(&mut scene, "a", false);
    let failed = ticket(&scene, id, ClipboardAction::Cut);
    scene.apply_clipboard(ClipboardReply {
        ticket: failed,
        result: Err("service unavailable".into()),
    });
    assert_eq!(value.get(), "abc");
    let cut = ticket(&scene, id, ClipboardAction::Cut);
    reply(&mut scene, cut, "");
    assert_eq!(value.get(), "");
    shortcut(&mut scene, "z", false);
    let paste = ticket(&scene, id, ClipboardAction::Paste);
    reply(&mut scene, paste, "");
    assert_eq!(value.get(), "abc");
}

#[test]
fn late_clipboard_rejected_after_edit_selection_external_focus_readonly_disable_or_expiry() {
    for condition in 0..7 {
        let value = Signal::new("abc".into());
        let (mut scene, id) = setup(&value);
        let mut request = ticket(&scene, id, ClipboardAction::Paste);
        match condition {
            0 => type_text(&mut scene, "!"),
            1 => named(&mut scene, NamedKey::Home),
            2 => value.update(|text| *text = "external".into()),
            3 => scene.focus_lost(),
            4 => {
                let editor = scene.nodes[id].editor.as_mut().unwrap();
                editor.readonly = true;
                editor.invalidate();
            }
            5 => scene.nodes[id].disabled = true,
            _ => request.expires = Instant::now() - Duration::from_secs(1),
        }
        let before = value.get();
        reply(&mut scene, request, "LATE");
        assert_eq!(value.get(), before, "condition={condition}");
    }
}

#[test]
fn blink_stops_for_zero_width_inner_content_with_positive_outer_clip_and_resumes() {
    let value = Signal::new("abc".into());
    let mut scene = Scene::new(Element::text_input(&value).max_width(8.0));
    scene.flush(240.0, 100.0);
    named(&mut scene, NamedKey::Tab);
    let id = scene.interaction.focused.unwrap();
    assert!(scene.nodes[id].clip.width > 0.0);
    assert!(scene.nodes[id].clip.height > 0.0);
    assert_eq!(scene.text_geometry(id).1.width, 0.0);
    let now = Instant::now();
    assert_clipped_blink_settles(&mut scene, id, now);

    let layout_id = scene.nodes[id].layout_id;
    let mut style = scene.tree.style(layout_id).unwrap().clone();
    style.max_size.width = length(240.0_f32);
    scene.tree.set_style(layout_id, style).unwrap();
    scene.dirty.structure = true;
    scene.flush(240.0, 100.0);
    assert_blink_resumes_without_layout(&mut scene, id, now + Duration::from_secs(10));
}

#[test]
fn blink_stops_for_padding_only_ancestor_clip_and_resumes() {
    let value = Signal::new("abc".into());
    let (view, handle) = Element::scroll_view(
        60.0,
        Element::column()
            .child(Element::text_input(&value))
            .child(Element::column().height(200.0)),
    );
    let mut scene = Scene::new(view);
    scene.flush(240.0, 100.0);
    named(&mut scene, NamedKey::Tab);
    let id = scene.interaction.focused.unwrap();
    let now = Instant::now();
    scene.update_blink(now, true);
    assert!(scene.blink_deadline().is_some());
    for padding in [1.0, 2.0, 3.0, 4.0] {
        handle.scroll_to(scene.nodes[id].bounds.height - padding);
        scene.flush(240.0, 100.0);
        assert_eq!(scene.interaction.focused, Some(id));
        assert!(scene.nodes[id].clip.width > 0.0);
        assert_eq!(scene.nodes[id].clip.height, padding);
        assert_eq!(scene.text_geometry(id).1.height, 0.0);
        assert_clipped_blink_settles(&mut scene, id, now);

        handle.scroll_to(0.0);
        scene.flush(240.0, 100.0);
        assert_blink_resumes_without_layout(&mut scene, id, now + Duration::from_secs(10));
    }
}

fn assert_clipped_blink_settles(scene: &mut Scene, id: NodeId, now: Instant) {
    scene.update_blink(now, true);
    assert_eq!(scene.blink_deadline(), None);
    assert!(!editor(scene, id).caret_visible);
    scene.presented();
    let counts = (scene.layouts, scene.formats);
    for interval in 1..=6 {
        scene.update_blink(now + BLINK_INTERVAL * interval, true);
        scene.flush(240.0, 100.0);
        assert_eq!(scene.blink_deadline(), None);
        assert!(!scene.needs_present());
        assert_eq!((scene.layouts, scene.formats), counts);
    }
}

fn assert_blink_resumes_without_layout(scene: &mut Scene, id: NodeId, now: Instant) {
    let clip = scene.text_geometry(id).1;
    assert!(clip.width > 0.0 && clip.height > 0.0);
    let counts = (scene.layouts, scene.formats);
    scene.presented();
    scene.update_blink(now, true);
    assert!(editor(scene, id).caret_visible);
    assert_eq!(scene.blink_deadline(), Some(now + BLINK_INTERVAL));
    assert!(scene.needs_present());
    scene.flush(240.0, 100.0);
    scene.presented();
    scene.update_blink(now + BLINK_INTERVAL - Duration::from_millis(1), true);
    assert!(!scene.needs_present());
    scene.update_blink(now + BLINK_INTERVAL, true);
    assert!(!editor(scene, id).caret_visible);
    assert!(scene.needs_present());
    assert_eq!(scene.blink_deadline(), Some(now + BLINK_INTERVAL * 2));
    scene.flush(240.0, 100.0);
    assert_eq!((scene.layouts, scene.formats), counts);
}

#[test]
fn blink_uses_deadlines_paint_only_and_suspends_when_occluded_zero_clipped_or_unfocused() {
    let value = Signal::new("abc".into());
    let (mut scene, id) = setup(&value);
    let now = Instant::now();
    scene.update_blink(now, true);
    let deadline = scene.blink_deadline().unwrap();
    let layouts = scene.layouts;
    scene.presented();
    scene.update_blink(deadline - Duration::from_millis(1), true);
    assert!(!scene.needs_present());
    scene.update_blink(deadline, true);
    assert!(scene.needs_present());
    assert!(!editor(&scene, id).caret_visible);
    assert_eq!(scene.blink_deadline(), Some(deadline + BLINK_INTERVAL));
    scene.flush(240.0, 100.0);
    assert_eq!(scene.layouts, layouts);
    scene.update_blink(deadline, false);
    assert_eq!(scene.blink_deadline(), None);
    scene.presented();
    scene.update_blink(deadline + Duration::from_secs(10), false);
    assert!(!scene.needs_present());
    scene.update_blink(deadline + Duration::from_secs(11), true);
    assert!(editor(&scene, id).caret_visible);
    assert!(scene.blink_deadline().is_some());
    scene.nodes[id].clip.width = 0.0;
    scene.update_blink(deadline, true);
    assert_eq!(scene.blink_deadline(), None);
    scene.focus_lost();
    scene.presented();
    scene.update_blink(deadline + Duration::from_secs(100), true);
    assert!(!scene.needs_present());
    assert_eq!(scene.blink_deadline(), None);
}

#[test]
fn tab_reveals_nested_focus_while_pointer_focus_does_not_shift_click_geometry() {
    let value = Signal::new("long ".repeat(100));
    let content = Element::column()
        .child(Element::text("top").focusable(true))
        .child(Element::text("spacer\n".repeat(8)))
        .child(Element::text_input(&value));
    let (view, handle) = Element::scroll_view(70.0, content);
    let mut scene = Scene::new(view);
    scene.flush(240.0, 100.0);
    named(&mut scene, NamedKey::Tab);
    named(&mut scene, NamedKey::Tab);
    assert!(handle.snapshot().offset > 0.0);
    let id = scene.interaction.focused.unwrap();
    assert!(editor(&scene, id).offset > 0.0);
    scene.focus_lost();
    handle.scroll_to(0.0);
    scene.flush(240.0, 100.0);
    let offset = handle.snapshot().offset;
    scene.mouse_input(true, winit::event::MouseButton::Left, Some((5.0, 5.0)));
    scene.mouse_input(false, winit::event::MouseButton::Left, Some((5.0, 5.0)));
    scene.flush(240.0, 100.0);
    assert_eq!(handle.snapshot().offset, offset);
}
