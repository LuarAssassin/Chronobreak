use super::*;
use crate::{MouseScrollDelta, ScrollError, TouchPhase};
use std::cell::Cell;
use winit::{
    dpi::PhysicalPosition,
    event::ElementState,
    keyboard::{Key, KeyCode, KeyLocation, ModifiersState, NamedKey, PhysicalKey},
};

fn row(height: f32) -> Element {
    Element::column().height(height).background([0.2; 4])
}

fn wheel(scene: &mut Scene, delta: f32) {
    scene.wheel(WheelInput::from_winit(
        MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, -f64::from(delta))),
        TouchPhase::Moved,
        1.0,
    ));
}

fn tab(scene: &mut Scene, reverse: bool) {
    scene.modifiers(if reverse {
        ModifiersState::SHIFT
    } else {
        ModifiersState::empty()
    });
    let input = KeyInput {
        logical_key: Key::Named(NamedKey::Tab),
        text: None,
        physical_key: PhysicalKey::Code(KeyCode::Tab),
        location: KeyLocation::Standard,
        state: ElementState::Pressed,
        repeat: false,
        synthetic: false,
    };
    scene.keyboard(input.clone());
    scene.keyboard(KeyInput {
        state: ElementState::Released,
        ..input
    });
    let (width, height) = scene.viewport.unwrap();
    scene.flush(width, height);
}

fn nested() -> (Scene, ScrollHandle, ScrollHandle) {
    let (inner_view, inner) = Element::scroll_view(100.0, row(300.0));
    let (outer_view, outer) =
        Element::scroll_view(150.0, Element::column().child(inner_view).child(row(500.0)));
    let mut scene = Scene::new(outer_view);
    scene.flush(300.0, 600.0);
    scene.pointer_position(Some((10.0, 10.0)));
    (scene, outer, inner)
}

#[test]
fn finite_viewport_measures_full_content_and_keeps_its_background_fixed() {
    let (view, handle) = Element::scroll_view(100.0, row(400.0));
    let mut scene = Scene::new(view.background([1.0; 4]));
    scene.flush(200.0, 500.0);
    let viewport = handle.id().unwrap();
    let content = scene.nodes[viewport].children[0];
    assert_eq!(
        handle.snapshot(),
        ScrollSnapshot {
            offset: 0.0,
            max_offset: 300.0,
            viewport_height: 100.0
        }
    );
    let bounds = scene.nodes[viewport].bounds;
    handle.scroll_to(80.0);
    scene.flush(200.0, 500.0);
    assert_eq!(scene.nodes[viewport].bounds, bounds);
    assert_eq!(scene.nodes[content].bounds.y, -80.0);
    assert_eq!(scene.nodes[content].bounds.height, 400.0);
    assert_eq!(scene.nodes[content].clip, bounds);
    assert_eq!(scene.nodes.len(), 2);
}

#[test]
fn line_pixel_dpi_sign_phase_and_raw_units_are_preserved() {
    let line = WheelInput::from_winit(
        MouseScrollDelta::LineDelta(7.0, -2.0),
        TouchPhase::Started,
        1.5,
    );
    assert_eq!(line.vertical, 80.0);
    assert_eq!(line.raw, MouseScrollDelta::LineDelta(7.0, -2.0));
    assert_eq!(line.phase, TouchPhase::Started);
    let pixel = WheelInput::from_winit(
        MouseScrollDelta::PixelDelta(PhysicalPosition::new(99.0, 60.0)),
        TouchPhase::Moved,
        1.5,
    );
    assert_eq!(pixel.vertical, -40.0);
    let same = WheelInput::from_winit(
        MouseScrollDelta::LineDelta(0.0, 1.0),
        TouchPhase::Ended,
        2.0,
    );
    assert_eq!(same.vertical, pixel.vertical);
}

#[test]
fn nested_partial_remainder_and_both_edges_chain_in_order() {
    let (mut scene, outer, inner) = nested();
    inner.scroll_to(180.0);
    scene.flush(300.0, 600.0);
    wheel(&mut scene, 50.0);
    assert_eq!(inner.snapshot().offset, 200.0);
    assert_eq!(outer.snapshot().offset, 30.0);
    wheel(&mut scene, 20.0);
    assert_eq!(outer.snapshot().offset, 50.0);
    wheel(&mut scene, -230.0);
    assert_eq!(inner.snapshot().offset, 0.0);
    assert_eq!(outer.snapshot().offset, 20.0);
    wheel(&mut scene, -40.0);
    assert_eq!(outer.snapshot().offset, 0.0);
}

#[test]
fn stopping_bubbling_does_not_stop_default_chain_but_prevention_does() {
    let prevent = Signal::new(false);
    let bubbles = Rc::new(Cell::new(0));
    let bubble_count = bubbles.clone();
    let guard = prevent.clone();
    let content = row(400.0).on_event(EventPhase::Target, move |event, context| {
        if matches!(event.kind, EventKind::Wheel(_)) {
            context.stop_propagation();
            if guard.get() {
                context.prevent_default();
            }
        }
    });
    let (view, handle) = Element::scroll_view(100.0, content);
    let mut scene = Scene::new(view.on_event(EventPhase::Bubble, move |event, _| {
        if matches!(event.kind, EventKind::Wheel(_)) {
            bubble_count.set(bubble_count.get() + 1);
        }
    }));
    scene.flush(200.0, 500.0);
    scene.pointer_position(Some((5.0, 5.0)));
    wheel(&mut scene, 50.0);
    assert_eq!(bubbles.get(), 0);
    assert_eq!(handle.snapshot().offset, 50.0);
    prevent.update(|value| *value = true);
    wheel(&mut scene, 50.0);
    assert_eq!(handle.snapshot().offset, 50.0);
}

#[test]
fn missing_pointer_does_not_fabricate_an_origin_hit() {
    let (mut scene, outer, inner) = nested();
    scene.pointer_position(None);
    wheel(&mut scene, 80.0);
    assert_eq!(inner.snapshot().offset, 0.0);
    assert_eq!(outer.snapshot().offset, 0.0);
}

#[test]
fn wheel_uses_hit_path_not_capture_and_pointer_coordinates_stay_window_local() {
    let positions = Rc::new(RefCell::new(Vec::new()));
    let observed = positions.clone();
    let capture = row(30.0).on_event(EventPhase::Target, move |event, context| match event.kind {
        EventKind::PointerDown(_) => context.capture_pointer(),
        EventKind::PointerMove => observed.borrow_mut().push(event.position),
        _ => {}
    });
    let (view, handle) = Element::scroll_view(100.0, row(500.0));
    let mut scene = Scene::new(Element::column().child(capture).child(view));
    scene.flush(200.0, 500.0);
    scene.pointer_button(true, 10.0, 10.0);
    scene.pointer_move(10.0, 50.0);
    wheel(&mut scene, 80.0);
    assert_eq!(handle.snapshot().offset, 80.0);
    scene.pointer_move(20.0, 60.0);
    assert_eq!(
        *positions.borrow(),
        vec![Some((10.0, 50.0)), Some((20.0, 60.0))]
    );
    scene.pointer_button(false, 20.0, 60.0);
}

#[test]
fn hide_remove_and_content_shrink_clamp_in_the_same_flush() {
    let shown = Signal::new(true);
    let (content, children) = Element::keyed_column();
    let (view, handle) = Element::scroll_view(100.0, content);
    let mut scene = Scene::new(view);
    children.insert("a", row(200.0));
    children.insert("b", row(200.0).visible(&shown));
    scene.flush(200.0, 500.0);
    handle.scroll_to(300.0);
    scene.flush(200.0, 500.0);
    shown.update(|value| *value = false);
    scene.flush(200.0, 500.0);
    assert_eq!(handle.snapshot().offset, 100.0);
    children.remove(children.node("a").unwrap());
    scene.flush(200.0, 500.0);
    assert_eq!(handle.snapshot().offset, 0.0);
    assert_eq!(handle.snapshot().max_offset, 0.0);
    assert_eq!(scene.geometry_version, scene.commit_version);
}

#[test]
fn pre_growth_scroll_to_uses_final_geometry_not_old_maximum() {
    let (content, children) = Element::keyed_column();
    let (view, handle) = Element::scroll_view(100.0, content);
    let mut scene = Scene::new(view);
    scene.flush(200.0, 500.0);
    let receipt = handle.scroll_to(250.0);
    children.insert("new", row(500.0));
    scene.flush(200.0, 500.0);
    assert_eq!(receipt.result(), Some(Ok(())));
    assert_eq!(handle.snapshot().offset, 250.0);
    assert_eq!(handle.snapshot().max_offset, 400.0);
}

#[test]
fn reflow_and_viewport_resize_remeasure_then_clamp() {
    let (view, handle) = Element::scroll_view(80.0, Element::text("中文重排边界".repeat(40)));
    let mut scene = Scene::new(view);
    scene.flush(100.0, 500.0);
    handle.scroll_to(10000.0);
    scene.flush(100.0, 500.0);
    let old = handle.snapshot().offset;
    scene.flush(500.0, 500.0);
    assert!(handle.snapshot().offset < old);
    assert_eq!(handle.snapshot().offset, handle.snapshot().max_offset);
    let layout_id = scene.nodes[handle.id().unwrap()].layout_id;
    let mut style = scene.tree.style(layout_id).unwrap().clone();
    style.size.height = length(3000.0_f32);
    scene.tree.set_style(layout_id, style).unwrap();
    scene.dirty.structure = true;
    scene.flush(500.0, 4000.0);
    assert_eq!(handle.snapshot().offset, 0.0);
    assert_eq!(handle.snapshot().viewport_height, 3000.0);
}

#[test]
fn moved_click_and_composed_clips_are_current_before_redraw() {
    let clicked = Rc::new(Cell::new(false));
    let clicks = clicked.clone();
    let (inner, inner_handle) = Element::scroll_view(
        100.0,
        Element::column()
            .child(row(100.0))
            .child(row(40.0).on_click(move || clicks.set(true)))
            .child(row(200.0)),
    );
    let (outer, outer_handle) =
        Element::scroll_view(80.0, Element::column().child(inner).child(row(300.0)));
    let mut scene = Scene::new(outer);
    scene.flush(200.0, 500.0);
    inner_handle.scroll_to(90.0);
    outer_handle.scroll_to(10.0);
    scene.pointer_button(true, 10.0, 10.0);
    assert!(scene.pointer_button(false, 10.0, 10.0));
    assert!(clicked.get());
    let target = scene.hit_test(10.0, 10.0).unwrap();
    assert_eq!(scene.nodes[target].bounds.y, 0.0);
    assert!(scene.nodes[target].clip.height <= 80.0);
    assert!(scene.hit_test(10.0, 90.0).is_none());
    assert_eq!(scene.geometry_version, scene.commit_version);
}

#[test]
fn pointer_partial_direct_rows_keep_the_clicked_slice_at_both_edges() {
    for (offset, pointer_y) in [(70.0, 10.0), (0.0, 70.0)] {
        let clicks = Rc::new(Cell::new(0));
        let observed = clicks.clone();
        let (content, children) = Element::keyed_column();
        let (view, handle) = Element::scroll_view(80.0, content);
        let mut scene = Scene::new(view);
        children.insert("before", row(60.0));
        children.insert(
            "target",
            row(40.0).on_click(move || observed.set(observed.get() + 1)),
        );
        children.insert("after", row(200.0));
        scene.flush(200.0, 500.0);
        handle.scroll_to(offset);
        scene.flush(200.0, 500.0);
        let target = children.node("target").unwrap();
        assert_eq!(
            scene.nodes[target].clip.height,
            if offset == 0.0 { 20.0 } else { 30.0 }
        );
        assert_eq!(scene.hit_test(10.0, pointer_y), Some(target));
        let bounds = scene.nodes[target].bounds;
        let clip = scene.nodes[target].clip;
        scene.pointer_button(true, 10.0, pointer_y);
        assert!(scene.pointer_button(false, 10.0, pointer_y));
        assert_eq!(clicks.get(), 1);
        assert_eq!(scene.nodes[target].bounds, bounds);
        assert_eq!(scene.nodes[target].clip, clip);
        assert_eq!(handle.snapshot().offset, offset);
        assert!(scene.focus_reveal.is_none());
        assert_eq!(scene.hit_test(10.0, pointer_y), Some(target));
        assert_eq!(scene.geometry_version, scene.commit_version);
    }
}

#[test]
fn pointer_nested_oversized_rows_keep_the_clicked_slice_with_non_scroll_clipping() {
    for clip_height in [20.0, 40.0] {
        let clicks = Rc::new(Cell::new(0));
        let observed = clicks.clone();
        let (content, children) = Element::keyed_column();
        let (inner_view, inner) = Element::scroll_view(40.0, content);
        let (outer_view, outer) = Element::scroll_view(
            100.0,
            Element::column()
                .child(row(200.0))
                .child(Element::column().height(clip_height).child(inner_view))
                .child(row(200.0)),
        );
        let mut scene = Scene::new(outer_view);
        children.insert(
            "target",
            row(200.0).on_click(move || observed.set(observed.get() + 1)),
        );
        scene.flush(200.0, 600.0);
        inner.scroll_to(160.0);
        outer.scroll_to(120.0);
        scene.flush(200.0, 600.0);
        let target = children.node("target").unwrap();
        assert_eq!(scene.nodes[target].clip.height, 20.0);
        assert_eq!(scene.hit_test(10.0, 90.0), Some(target));
        let bounds = scene.nodes[target].bounds;
        let clip = scene.nodes[target].clip;
        scene.pointer_button(true, 10.0, 90.0);
        assert!(scene.pointer_button(false, 10.0, 90.0));
        assert_eq!(clicks.get(), 1);
        assert_eq!(inner.snapshot().offset, 160.0);
        assert_eq!(outer.snapshot().offset, 120.0);
        assert_eq!(scene.nodes[target].bounds, bounds);
        assert_eq!(scene.nodes[target].clip, clip);
        assert!(scene.focus_reveal.is_none());
        assert_eq!(scene.hit_test(10.0, 90.0), Some(target));
        assert!(scene.nodes[target].clip.height <= clip_height);
    }
}

fn pointer_child_with_distinct_focus_ancestor(offset: f32, pointer_y: f32) {
    let clicks = Rc::new(Cell::new(0));
    let observed = clicks.clone();
    let (content, children) = Element::keyed_column();
    let (view, handle) = Element::scroll_view(80.0, content);
    let mut scene = Scene::new(view);
    children.insert("before", row(60.0));
    let child = row(20.0)
        .on_click(move || observed.set(observed.get() + 1))
        .focusable(false);
    let ancestor = if offset == 0.0 {
        row(60.0).focusable(true).child(child).child(row(40.0))
    } else {
        row(60.0).focusable(true).child(row(40.0)).child(child)
    };
    children.insert("ancestor", ancestor);
    children.insert("after", row(200.0));
    scene.flush(200.0, 500.0);
    handle.scroll_to(offset);
    scene.flush(200.0, 500.0);
    let ancestor = children.node("ancestor").unwrap();
    let target = scene.hit_test(10.0, pointer_y).unwrap();
    assert_ne!(target, ancestor);
    assert!(!scene.nodes[target].focusable);
    let bounds = scene.nodes[target].bounds;
    let clip = scene.nodes[target].clip;
    scene.pointer_button(true, 10.0, pointer_y);
    let activated = scene.pointer_button(false, 10.0, pointer_y);
    assert!(
        activated && clicks.get() == 1,
        "offset {offset} -> {}; child y {} -> {}; clicks={}",
        handle.snapshot().offset,
        bounds.y,
        scene.nodes[target].bounds.y,
        clicks.get()
    );
    assert_eq!(scene.nodes[target].bounds, bounds);
    assert_eq!(scene.nodes[target].clip, clip);
    assert_eq!(handle.snapshot().offset, offset);
    assert_eq!(scene.geometry_version, scene.commit_version);
}

#[test]
fn pointer_bottom_child_does_not_reveal_a_different_focus_rectangle() {
    pointer_child_with_distinct_focus_ancestor(0.0, 70.0);
}

#[test]
fn pointer_top_child_does_not_reveal_a_different_focus_rectangle() {
    pointer_child_with_distinct_focus_ancestor(100.0, 10.0);
}

#[test]
fn pointer_explicit_event_focus_still_reveals_and_uses_current_release_geometry() {
    for (phase, child_focusable) in [
        (EventPhase::Capture, false),
        (EventPhase::Bubble, false),
        (EventPhase::Bubble, true),
    ] {
        let clicks = Rc::new(Cell::new(0));
        let observed = clicks.clone();
        let (content, children) = Element::keyed_column();
        let (view, handle) = Element::scroll_view(80.0, content);
        let mut scene = Scene::new(view);
        children.insert("before", row(60.0));
        children.insert(
            "ancestor",
            row(60.0)
                .focusable(true)
                .on_event(phase, |event, context| {
                    if matches!(event.kind, EventKind::PointerDown(_)) {
                        context.focus();
                    }
                })
                .child(
                    row(20.0)
                        .on_click(move || observed.set(observed.get() + 1))
                        .focusable(child_focusable),
                )
                .child(row(40.0)),
        );
        children.insert("after", row(200.0));
        scene.flush(200.0, 500.0);
        let ancestor = children.node("ancestor").unwrap();
        scene.pointer_button(true, 10.0, 70.0);
        scene.flush(200.0, 500.0);
        assert_eq!(handle.snapshot().offset, 40.0);
        assert_eq!(scene.nodes[ancestor].bounds.y, 20.0);
        assert!(!scene.pointer_button(false, 10.0, 70.0));
        assert_eq!(clicks.get(), 0);
    }
}

#[test]
fn pointer_explicit_target_focus_reveals_a_direct_row() {
    let clicks = Rc::new(Cell::new(0));
    let observed = clicks.clone();
    let (content, children) = Element::keyed_column();
    let (view, handle) = Element::scroll_view(80.0, content);
    let mut scene = Scene::new(view);
    children.insert("before", row(60.0));
    children.insert(
        "target",
        row(40.0)
            .on_click(move || observed.set(observed.get() + 1))
            .on_event(EventPhase::Target, |event, context| {
                if matches!(event.kind, EventKind::PointerDown(_)) {
                    context.focus();
                }
            }),
    );
    children.insert("after", row(200.0));
    scene.flush(200.0, 500.0);
    scene.pointer_button(true, 10.0, 70.0);
    assert!(scene.pointer_button(false, 10.0, 70.0));
    assert_eq!(clicks.get(), 1);
    assert_eq!(handle.snapshot().offset, 20.0);
    assert_eq!(scene.nodes[children.node("target").unwrap()].bounds.y, 40.0);
}

#[test]
fn pointer_default_focus_then_explicit_reveal_still_moves_the_same_control() {
    let clicks = Rc::new(Cell::new(0));
    let observed = clicks.clone();
    let (content, children) = Element::keyed_column();
    let (view, handle) = Element::scroll_view(80.0, content);
    let mut scene = Scene::new(view);
    children.insert("before", row(60.0));
    children.insert(
        "target",
        row(40.0).on_click(move || observed.set(observed.get() + 1)),
    );
    children.insert("after", row(200.0));
    scene.flush(200.0, 500.0);
    scene.pointer_button(true, 10.0, 70.0);
    assert!(scene.pointer_button(false, 10.0, 70.0));
    assert_eq!(clicks.get(), 1);
    let target = children.node("target").unwrap();
    let receipt = handle.scroll_into_view(target);
    scene.flush(200.0, 500.0);
    assert_eq!(receipt.result(), Some(Ok(())));
    assert_eq!(handle.snapshot().offset, 20.0);
    assert_eq!(scene.nodes[target].bounds.y, 40.0);
    handle.scroll_to(0.0);
    for _ in 0..3 {
        scene.flush(200.0, 500.0);
    }
    assert_eq!(handle.snapshot().offset, 0.0);
}

#[test]
fn explicit_keyboard_event_focus_still_reveals_after_pointer_default_focus() {
    let clicks = Rc::new(Cell::new(0));
    let observed = clicks.clone();
    let (view, handle) = Element::scroll_view(80.0, Element::column()
        .child(row(60.0))
        .child(row(60.0).focusable(true)
            .on_event(EventPhase::Capture, |event, context| {
                if matches!(&event.kind, EventKind::Key(input) if input.logical_key == Key::Named(NamedKey::F1)) {
                    context.focus();
                }
            })
            .child(row(20.0).on_click(move || observed.set(observed.get() + 1)))
            .child(row(40.0)))
        .child(row(200.0)));
    let mut scene = Scene::new(view);
    scene.flush(200.0, 500.0);
    scene.pointer_button(true, 10.0, 70.0);
    assert!(scene.pointer_button(false, 10.0, 70.0));
    assert_eq!(clicks.get(), 1);
    assert_eq!(handle.snapshot().offset, 0.0);
    scene.keyboard(KeyInput {
        logical_key: Key::Named(NamedKey::F1),
        text: None,
        physical_key: PhysicalKey::Code(KeyCode::F1),
        location: KeyLocation::Standard,
        state: ElementState::Pressed,
        repeat: false,
        synthetic: false,
    });
    scene.flush(200.0, 500.0);
    assert_eq!(handle.snapshot().offset, 40.0);
}

#[test]
fn stationary_pointer_reconciles_hover_after_scroll_and_tree_changes() {
    let entries = Rc::new(Cell::new(0));
    let observed = entries.clone();
    let (content, children) = Element::keyed_column();
    let (view, handle) = Element::scroll_view(80.0, content);
    let mut scene = Scene::new(view);
    children.insert("a", row(100.0));
    children.insert(
        "b",
        row(100.0).on_event(EventPhase::Target, move |event, _| {
            if event.kind == EventKind::PointerEnter {
                observed.set(observed.get() + 1);
            }
        }),
    );
    scene.flush(200.0, 500.0);
    scene.pointer_move(10.0, 10.0);
    handle.scroll_to(100.0);
    scene.flush(200.0, 500.0);
    assert_eq!(entries.get(), 1);
    children.remove(children.node("a").unwrap());
    scene.flush(200.0, 500.0);
    assert_eq!(handle.snapshot().offset, 20.0);
    assert_eq!(scene.deepest_hit(10.0, 10.0), children.node("b"));
}

#[test]
fn tab_and_reverse_tab_reveal_nested_offscreen_controls_without_repeated_snapback() {
    let (inner, inner_handle) = Element::scroll_view(
        80.0,
        Element::column()
            .child(row(20.0).focusable(true))
            .child(row(250.0))
            .child(row(20.0).focusable(true)),
    );
    let (outer, outer_handle) = Element::scroll_view(
        100.0,
        Element::column()
            .child(row(180.0))
            .child(inner)
            .child(row(300.0)),
    );
    let mut scene = Scene::new(outer);
    scene.flush(200.0, 600.0);
    tab(&mut scene, false);
    assert!(outer_handle.snapshot().offset > 0.0);
    tab(&mut scene, false);
    assert!(inner_handle.snapshot().offset > 0.0);
    inner_handle.scroll_to(0.0);
    outer_handle.scroll_to(0.0);
    for _ in 0..3 {
        scene.flush(200.0, 600.0);
    }
    assert_eq!(inner_handle.snapshot().offset, 0.0);
    assert_eq!(outer_handle.snapshot().offset, 0.0);
    tab(&mut scene, true);
    assert_eq!(inner_handle.snapshot().offset, 0.0);
    assert!(outer_handle.snapshot().offset > 0.0);
    tab(&mut scene, true);
    assert!(inner_handle.snapshot().offset > 0.0);
}

#[test]
fn explicit_reveal_validates_scope_foreign_stale_and_hidden_targets() {
    let shown = Signal::new(true);
    let (content, children) = Element::keyed_column();
    let (view, handle) = Element::scroll_view(80.0, content);
    let mut scene = Scene::new(Element::column().child(view).child(row(20.0)));
    children.insert("spacer", row(200.0));
    children.insert("target", row(30.0).visible(&shown));
    scene.flush(200.0, 500.0);
    let target = children.node("target").unwrap();
    let receipt = handle.scroll_into_view(target);
    scene.flush(200.0, 500.0);
    assert_eq!(receipt.result(), Some(Ok(())));
    assert_eq!(handle.snapshot().offset, 150.0);
    let foreign = Scene::new(row(1.0)).root.unwrap();
    let foreign_receipt = handle.scroll_into_view(foreign);
    let outside = handle.scroll_into_view(scene.root.unwrap());
    shown.update(|value| *value = false);
    let hidden = handle.scroll_into_view(target);
    scene.flush(200.0, 500.0);
    assert_eq!(
        foreign_receipt.result(),
        Some(Err(ScrollError::ForeignScene))
    );
    assert_eq!(outside.result(), Some(Err(ScrollError::OutsideScope)));
    assert_eq!(hidden.result(), Some(Err(ScrollError::Hidden)));
    children.remove(target);
    children.insert("target2", row(50.0));
    let stale = handle.scroll_into_view(target);
    scene.flush(200.0, 500.0);
    assert_eq!(stale.result(), Some(Err(ScrollError::StaleTarget)));
}

#[test]
fn oversized_target_has_finite_nearest_policy_and_non_scroll_clipping_is_not_undone() {
    let (content, children) = Element::keyed_column();
    let (view, handle) = Element::scroll_view(80.0, content);
    let mut scene = Scene::new(Element::column().height(40.0).child(view));
    children.insert("spacer", row(100.0));
    children.insert("large", row(200.0));
    scene.flush(200.0, 500.0);
    let target = children.node("large").unwrap();
    handle.scroll_into_view(target);
    scene.flush(200.0, 500.0);
    assert_eq!(handle.snapshot().offset, 100.0);
    assert_eq!(scene.nodes[handle.id().unwrap()].clip.height, 40.0);
    handle.scroll_to(150.0);
    scene.flush(200.0, 500.0);
    handle.scroll_into_view(target);
    scene.flush(200.0, 500.0);
    assert_eq!(handle.snapshot().offset, 150.0);
}

fn nested_oversized(clip_height: Option<f32>) -> (Scene, ScrollHandle, ScrollHandle, NodeId) {
    let (content, children) = Element::keyed_column();
    let (inner_view, inner) = Element::scroll_view(40.0, content);
    let inner_view = if let Some(height) = clip_height {
        Element::column().height(height).child(inner_view)
    } else {
        inner_view
    };
    let (outer_view, outer) = Element::scroll_view(
        100.0,
        Element::column()
            .child(row(200.0))
            .child(inner_view)
            .child(row(200.0)),
    );
    let mut scene = Scene::new(outer_view);
    children.insert("target", row(200.0).focusable(true));
    scene.flush(200.0, 600.0);
    let target = children.node("target").unwrap();
    inner.scroll_to(160.0);
    scene.flush(200.0, 600.0);
    assert_eq!(inner.snapshot().offset, 160.0);
    assert_eq!(outer.snapshot().offset, 0.0);
    assert_eq!(scene.nodes[target].clip.height, 0.0);
    (scene, outer, inner, target)
}

fn assert_nested_oversized_revealed(
    scene: &Scene,
    outer: &ScrollHandle,
    inner: &ScrollHandle,
    target: NodeId,
) {
    assert_eq!(inner.snapshot().offset, 160.0);
    assert_eq!(outer.snapshot().offset, 140.0);
    let viewport = &scene.nodes[inner.id().unwrap()];
    assert_eq!(viewport.bounds.y, 60.0);
    assert_eq!(viewport.clip, viewport.bounds);
    assert_eq!(viewport.clip.height, 40.0);
    assert_eq!(scene.nodes[target].clip, viewport.clip);
}

#[test]
fn explicit_nested_oversized_reveal_propagates_only_the_visible_slice() {
    let (mut scene, outer, inner, target) = nested_oversized(None);
    let receipt = outer.scroll_into_view(target);
    scene.flush(200.0, 600.0);
    assert_eq!(receipt.result(), Some(Ok(())));
    assert_nested_oversized_revealed(&scene, &outer, &inner, target);
    let placements = scene.placements;
    outer.scroll_into_view(target);
    scene.flush(200.0, 600.0);
    assert_nested_oversized_revealed(&scene, &outer, &inner, target);
    assert_eq!(scene.placements, placements);
}

#[test]
fn tab_nested_oversized_reveal_propagates_only_the_visible_slice_without_snapback() {
    let (mut scene, outer, inner, target) = nested_oversized(None);
    tab(&mut scene, false);
    assert_nested_oversized_revealed(&scene, &outer, &inner, target);
    outer.scroll_to(0.0);
    for _ in 0..3 {
        scene.flush(200.0, 600.0);
    }
    assert_eq!(outer.snapshot().offset, 0.0);
    assert_eq!(inner.snapshot().offset, 160.0);
    assert_eq!(scene.nodes[target].clip.height, 0.0);
}

#[test]
fn nested_reveal_preserves_intermediate_non_scroll_clipping_without_offset_churn() {
    for height in [20.0, 0.0] {
        let (mut scene, outer, inner, target) = nested_oversized(Some(height));
        for _ in 0..3 {
            let receipt = outer.scroll_into_view(target);
            scene.flush(200.0, 600.0);
            assert_eq!(receipt.result(), Some(Ok(())));
            assert_eq!(inner.snapshot().offset, 160.0);
            assert_eq!(
                outer.snapshot().offset,
                if height > 0.0 { 120.0 } else { 0.0 }
            );
            assert_eq!(scene.nodes[target].clip.height, height);
            assert_eq!(scene.nodes[inner.id().unwrap()].clip.height, height);
        }
        let placements = scene.placements;
        tab(&mut scene, false);
        assert_eq!(scene.nodes[target].clip.height, height);
        assert_eq!(scene.placements, placements);
    }
}

#[test]
fn no_op_offsets_do_not_layout_place_or_present_and_real_offsets_only_place() {
    let (mut scene, outer, inner) = nested();
    scene.flush(300.0, 600.0);
    scene.presented();
    let initial = (scene.layouts, scene.placements, scene.commit_version);
    inner.scroll_to(-100.0);
    outer.scroll_by(0.0);
    inner.scroll_by(100.0);
    inner.scroll_by(-100.0);
    scene.flush(300.0, 600.0);
    wheel(&mut scene, -100.0);
    assert_eq!(
        (scene.layouts, scene.placements, scene.commit_version),
        initial
    );
    assert!(!scene.needs_present());
    inner.scroll_by(25.0);
    scene.flush(300.0, 600.0);
    assert_eq!(scene.layouts, initial.0);
    assert_eq!(scene.placements, initial.1 + 1);
    assert!(scene.needs_present());
    assert_eq!(scene.geometry_version, scene.commit_version);
}

#[test]
fn viewport_hide_and_removal_release_state_and_cannot_move_replacement() {
    let shown = Signal::new(true);
    let (host, children) = Element::keyed_column();
    let (view, handle) = Element::scroll_view(80.0, row(400.0));
    let mut scene = Scene::new(host);
    children.insert("viewport", view.visible(&shown));
    scene.flush(200.0, 500.0);
    handle.scroll_to(200.0);
    scene.flush(200.0, 500.0);
    shown.update(|value| *value = false);
    let hidden = handle.scroll_by(50.0);
    scene.flush(200.0, 500.0);
    assert_eq!(handle.snapshot().offset, 0.0);
    assert_eq!(hidden.result(), Some(Err(ScrollError::Hidden)));
    let queued = handle.scroll_to(100.0);
    children.remove(children.node("viewport").unwrap());
    let (replacement, next) = Element::scroll_view(80.0, row(500.0));
    children.insert("viewport", replacement);
    scene.flush(200.0, 500.0);
    assert_eq!(queued.result(), Some(Err(ScrollError::StaleViewport)));
    assert_eq!(
        handle.scroll_to(1.0).result(),
        Some(Err(ScrollError::StaleViewport))
    );
    assert_eq!(handle.snapshot(), ScrollSnapshot::default());
    assert_eq!(next.snapshot().offset, 0.0);
    let pending = next.scroll_to(100.0);
    drop(scene);
    assert_eq!(pending.result(), Some(Err(ScrollError::SceneDropped)));
    assert_eq!(
        next.scroll_to(1.0).result(),
        Some(Err(ScrollError::SceneDropped))
    );
}

#[test]
fn invalid_commands_and_unmounted_handle_have_clear_errors() {
    assert_eq!(
        ScrollHandle::default().scroll_to(1.0).result(),
        Some(Err(ScrollError::NotMounted))
    );
    let (view, handle) = Element::scroll_view(80.0, row(400.0));
    let mut scene = Scene::new(view);
    let nan = handle.scroll_to(f32::NAN);
    let infinite = handle.scroll_by(f32::INFINITY);
    scene.flush(200.0, 500.0);
    assert_eq!(nan.result(), Some(Err(ScrollError::NonFiniteOffset)));
    assert_eq!(infinite.result(), Some(Err(ScrollError::NonFiniteOffset)));
    assert_eq!(handle.snapshot().offset, 0.0);
}

#[test]
fn indicator_is_proportional_clipped_and_after_content() {
    let (view, handle) = Element::scroll_view(100.0, row(400.0));
    let mut scene = Scene::new(view);
    handle.scroll_to(150.0);
    scene.flush(200.0, 500.0);
    let list = scene.display_list();
    let Paint::Rect { bounds, clip, .. } = list.last().unwrap() else {
        panic!("indicator");
    };
    assert_eq!(bounds.height, 25.0);
    assert_eq!(bounds.y, 37.5);
    assert_eq!(bounds.x, 196.0);
    assert_eq!(clip.height, 100.0);
    assert!(scene.hit_test(198.0, 50.0).is_none());
}

#[test]
fn explicit_reveal_walks_inner_to_scope_but_does_not_move_ancestors_outside_scope() {
    let (content, children) = Element::keyed_column();
    let (inner, inner_handle) = Element::scroll_view(80.0, content);
    let (outer, outer_handle) = Element::scroll_view(
        100.0,
        Element::column()
            .child(row(200.0))
            .child(inner)
            .child(row(200.0)),
    );
    let mut scene = Scene::new(outer);
    children.insert("spacer", row(300.0));
    children.insert("target", row(20.0));
    scene.flush(200.0, 600.0);
    let target = children.node("target").unwrap();
    inner_handle.scroll_into_view(target);
    scene.flush(200.0, 600.0);
    assert_eq!(inner_handle.snapshot().offset, 240.0);
    assert_eq!(outer_handle.snapshot().offset, 0.0);
    outer_handle.scroll_into_view(target);
    scene.flush(200.0, 600.0);
    assert_eq!(outer_handle.snapshot().offset, 180.0);
    assert_eq!(scene.nodes[target].clip.height, 20.0);
}

#[test]
fn bound_text_shrink_uses_untranslated_extent_and_retains_clipped_subscriptions() {
    let value = Signal::new("中文文本".repeat(100));
    let (view, handle) = Element::scroll_view(80.0, Element::bound_text(&value, |text| text));
    let mut scene = Scene::new(view);
    scene.flush(120.0, 500.0);
    handle.scroll_to(100000.0);
    scene.flush(120.0, 500.0);
    assert!(handle.snapshot().offset > 100.0);
    assert_eq!(value.subscriber_count(), 1);
    value.update(|text| *text = "短文".into());
    scene.flush(120.0, 500.0);
    assert_eq!(handle.snapshot().offset, 0.0);
    assert_eq!(handle.snapshot().max_offset, 0.0);
    assert_eq!(value.subscriber_count(), 1);
    assert_eq!(scene.nodes.len(), 2);
}

#[test]
fn a_captured_scrolled_control_keeps_capture_and_window_coordinates() {
    let positions = Rc::new(RefCell::new(Vec::new()));
    let observed = positions.clone();
    let captured = row(40.0).on_event(EventPhase::Target, move |event, context| match event.kind {
        EventKind::PointerDown(_) => context.capture_pointer(),
        EventKind::PointerMove | EventKind::PointerUp(_) => {
            observed.borrow_mut().push(event.position)
        }
        _ => {}
    });
    let (view, handle) =
        Element::scroll_view(80.0, Element::column().child(captured).child(row(500.0)));
    let mut scene = Scene::new(view);
    scene.flush(200.0, 500.0);
    scene.pointer_button(true, 10.0, 10.0);
    handle.scroll_to(100.0);
    scene.flush(200.0, 500.0);
    scene.pointer_move(15.0, 250.0);
    scene.pointer_button(false, 15.0, 250.0);
    assert_eq!(
        *positions.borrow(),
        vec![Some((15.0, 250.0)), Some((15.0, 250.0))]
    );
}

#[test]
fn focus_reveal_waits_for_commit_and_prevented_tab_does_not_reveal() {
    let prevent = Signal::new(false);
    let guard = prevent.clone();
    let (view, handle) = Element::scroll_view(
        80.0,
        Element::column()
            .child(row(20.0).focusable(true))
            .child(row(200.0))
            .child(row(20.0).focusable(true)),
    );
    let mut scene = Scene::new(view.on_event(EventPhase::Capture, move |event, context| {
        if matches!(event.kind, EventKind::Key(_)) && guard.get() {
            context.prevent_default();
        }
    }));
    scene.flush(200.0, 500.0);
    tab(&mut scene, false);
    prevent.update(|value| *value = true);
    tab(&mut scene, false);
    assert_eq!(handle.snapshot().offset, 0.0);
    prevent.update(|value| *value = false);
    scene.keyboard(KeyInput {
        logical_key: Key::Named(NamedKey::Tab),
        text: None,
        physical_key: PhysicalKey::Code(KeyCode::Tab),
        location: KeyLocation::Standard,
        state: ElementState::Pressed,
        repeat: false,
        synthetic: false,
    });
    assert_eq!(handle.snapshot().offset, 0.0);
    scene.flush(200.0, 500.0);
    assert_eq!(handle.snapshot().offset, 160.0);
    scene.pointer_position(Some((10.0, 10.0)));
    wheel(&mut scene, -160.0);
    for _ in 0..3 {
        scene.flush(200.0, 500.0);
    }
    assert_eq!(handle.snapshot().offset, 0.0);
}

#[test]
fn only_visible_text_is_painted_after_scroll_without_unmount_or_relayout() {
    let (content, children) = Element::keyed_column();
    let (view, handle) = Element::scroll_view(40.0, content);
    let mut scene = Scene::new(view);
    for index in 0..10 {
        children.insert(
            index.to_string(),
            Element::text(format!("行 {index}")).height(40.0),
        );
    }
    scene.flush(200.0, 500.0);
    let before: Vec<_> = scene
        .display_list()
        .iter()
        .filter_map(|paint| {
            if let Paint::Text(id) = paint {
                Some(*id)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(before, vec![children.node("0").unwrap()]);
    let layouts = scene.layouts;
    handle.scroll_to(200.0);
    scene.flush(200.0, 500.0);
    let after: Vec<_> = scene
        .display_list()
        .iter()
        .filter_map(|paint| {
            if let Paint::Text(id) = paint {
                Some(*id)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(after, vec![children.node("5").unwrap()]);
    assert_eq!(scene.layouts, layouts);
    assert_eq!(scene.nodes.len(), 12);
}
