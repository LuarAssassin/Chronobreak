use super::*;
use crate::batch;
use std::cell::Cell;

fn button(clicks: &Rc<Cell<usize>>, label: &str) -> Element {
    let clicks = clicks.clone();
    Element::text(label)
        .padding(8.0)
        .on_click(move || clicks.set(clicks.get() + 1))
}

fn click(scene: &mut Scene, x: f32, y: f32) -> bool {
    scene.pointer_button(true, x, y);
    scene.pointer_button(false, x, y)
}

#[test]
fn first_listener_reentry_before_binding_commits_only_two() {
    let signal = Signal::new(0);
    let writer = signal.clone();
    let _first = signal.subscribe(move |value| {
        if *value == 1 {
            writer.update(|value| *value = 2);
        }
    });
    let mut scene = Scene::new(Element::bound_text(&signal, |value| value.to_string()));
    scene.flush(400.0, 600.0);
    signal.update(|value| *value = 1);
    assert_eq!(signal.get(), 2);
    assert_eq!(scene.flush(400.0, 600.0), 1);
    let node = &scene.nodes[scene.root.unwrap()];
    assert_eq!(node.text.as_deref(), Some("2"));
    assert_eq!(scene.formats, 1);
    assert_eq!(scene.sync(), 0);
    assert_eq!(scene.geometry_version, scene.commit_version);
}

#[test]
fn multi_signal_properties_and_commands_share_one_layout() {
    let text = Signal::new(0);
    let shown = Signal::new(true);
    let (dynamic, children) = Element::keyed_column();
    let mut scene = Scene::new(
        Element::column()
            .child(Element::bound_text(&text, |value| format!("{value}")).visible(&shown))
            .child(Element::text("unchanged"))
            .child(dynamic),
    );
    scene.flush(400.0, 600.0);
    scene.presented();
    let root = scene.root.unwrap();
    let bound = scene.nodes[root].children[0];
    let unaffected = scene.nodes[root].children[1];
    let old_bounds = scene.nodes[unaffected].bounds;
    batch(|| {
        text.update(|value| *value = 1);
        batch(|| text.update(|value| *value = 2));
        text.update(|value| *value = 3);
        shown.update(|value| *value = false);
        children.insert("new", Element::text("inserted"));
    });
    assert_eq!(scene.flush(400.0, 600.0), 2);
    assert_eq!(scene.formats, 1);
    assert_eq!(scene.layouts, 2);
    assert_eq!(scene.nodes[bound].revision, 1);
    assert_eq!(scene.nodes[unaffected].revision, 0);
    assert_ne!(scene.nodes[unaffected].bounds, old_bounds);
    assert_eq!(scene.nodes[unaffected].text.as_deref(), Some("unchanged"));
    assert!(scene.needs_present());
    scene.presented();
    let version = scene.geometry_version;
    assert_eq!(scene.flush(400.0, 600.0), 0);
    assert_eq!(scene.layouts, 2);
    assert_eq!(scene.geometry_version, version);
    assert!(!scene.needs_present());
}

#[test]
fn round_trip_and_equal_formatted_text_do_not_relayout_or_present() {
    let signal = Signal::new(0);
    let mut scene = Scene::new(Element::bound_text(&signal, |value| {
        format!("{}", value / 10)
    }));
    scene.flush(400.0, 600.0);
    scene.presented();
    batch(|| {
        signal.update(|value| *value = 1);
        signal.update(|value| *value = 0);
    });
    assert_eq!(scene.flush(400.0, 600.0), 0);
    signal.update(|value| *value = 2);
    assert_eq!(scene.flush(400.0, 600.0), 0);
    assert_eq!(scene.formats, 2);
    assert_eq!(scene.layouts, 1);
    assert!(!scene.needs_present());
}

#[test]
fn text_reflow_before_input_uses_latest_geometry_without_redraw() {
    let text = Signal::new(String::from("short"));
    let clicks = Rc::new(Cell::new(0));
    let mut scene = Scene::new(
        Element::column()
            .child(Element::bound_text(&text, |value| value))
            .child(button(&clicks, "target")),
    );
    scene.flush(180.0, 800.0);
    let target = scene.nodes[scene.root.unwrap()].children[1];
    let old = scene.nodes[target].bounds;
    scene.pointer_button(true, old.x + 2.0, old.y + 2.0);
    text.update(|value| {
        *value = "wrapped text moves the button below several lines of measured content ".repeat(4)
    });
    assert!(!scene.pointer_button(false, old.x + 2.0, old.y + 2.0));
    let new = scene.nodes[target].bounds;
    assert!(new.y > old.y + old.height);
    assert_eq!(scene.geometry_version, scene.commit_version);
    assert_eq!(scene.layouts, 2);
    assert!(!click(&mut scene, old.x + 2.0, old.y + 2.0));
    assert!(click(&mut scene, new.x + 2.0, new.y + 2.0));
    assert_eq!(clicks.get(), 1);
    assert_eq!(scene.layouts, 2);
}

#[test]
fn hidden_pressed_ancestor_cannot_activate_and_other_hide_preserves_press() {
    let shown = Signal::new(true);
    let other_shown = Signal::new(true);
    let clicks = Rc::new(Cell::new(0));
    let mut scene = Scene::new(
        Element::column()
            .child(
                Element::column()
                    .visible(&shown)
                    .child(button(&clicks, "target")),
            )
            .child(Element::text("other").visible(&other_shown)),
    );
    scene.flush(400.0, 600.0);
    scene.pointer_button(true, 2.0, 2.0);
    other_shown.update(|value| *value = false);
    assert!(scene.pointer_button(false, 2.0, 2.0));
    scene.pointer_button(true, 2.0, 2.0);
    shown.update(|value| *value = false);
    assert!(!scene.pointer_button(false, 2.0, 2.0));
    shown.update(|value| *value = true);
    assert!(!scene.pointer_button(false, 2.0, 2.0));
    assert_eq!(clicks.get(), 1);
}

#[test]
fn queued_insert_reorder_and_unmount_are_flushed_before_button_input() {
    let (root, children) = Element::keyed_column();
    let left = Rc::new(Cell::new(0));
    let right = Rc::new(Cell::new(0));
    let mut scene = Scene::new(root);
    children.insert("left", button(&left, "left"));
    children.insert("right", button(&right, "right"));
    scene.flush(400.0, 600.0);
    let left_id = children.node("left").unwrap();
    let right_id = children.node("right").unwrap();
    children.reorder(&[right_id, left_id]);
    assert!(click(&mut scene, 2.0, 2.0));
    assert_eq!((left.get(), right.get()), (0, 1));
    let inserted = children.insert("prefix", Element::text("prefix"));
    scene.flush(400.0, 600.0);
    children.move_before(inserted.result().unwrap().unwrap().unwrap(), Some(right_id));
    assert!(!click(&mut scene, 2.0, 2.0));
    let right_bounds = scene.nodes[right_id].bounds;
    scene.pointer_button(true, right_bounds.x + 2.0, right_bounds.y + 2.0);
    children.remove(right_id);
    assert!(!scene.pointer_button(false, right_bounds.x + 2.0, right_bounds.y + 2.0));
    assert_eq!((left.get(), right.get()), (0, 1));
    assert_eq!(scene.geometry_version, scene.commit_version);
}

#[test]
fn viewport_change_and_noop_structure_respect_commit_boundary() {
    let (root, children) = Element::keyed_column();
    let mut scene = Scene::new(root.child(Element::text("static")));
    let inserted = children.insert("button", Element::text("dynamic"));
    scene.flush(400.0, 600.0);
    let target = inserted.result().unwrap().unwrap().unwrap();
    scene.presented();
    children.move_before(target, None);
    assert_eq!(scene.flush(400.0, 600.0), 0);
    assert!(!scene.needs_present());
    let layouts = scene.layouts;
    scene.flush(120.0, 600.0);
    assert_eq!(scene.layouts, layouts + 1);
    assert_eq!(scene.geometry_version, scene.commit_version);
    assert!(scene.needs_present());
}

#[test]
fn pending_insert_and_visibility_collapse_move_target_before_input() {
    let shown = Signal::new(true);
    let clicks = Rc::new(Cell::new(0));
    let (prefix, children) = Element::keyed_column();
    let mut scene = Scene::new(
        Element::column()
            .child(Element::text("visible prefix").visible(&shown))
            .child(prefix)
            .child(button(&clicks, "target")),
    );
    scene.flush(400.0, 600.0);
    let target = scene.nodes[scene.root.unwrap()].children[2];
    let old = scene.nodes[target].bounds;
    children.insert("new", Element::text("inserted prefix"));
    assert!(!click(&mut scene, 2.0, old.y + 2.0));
    let shifted = scene.nodes[target].bounds;
    assert!(shifted.y > old.y);
    shown.update(|value| *value = false);
    assert!(click(&mut scene, 2.0, old.y + 2.0));
    assert_eq!(scene.nodes[target].bounds.y, old.y);
    assert_eq!(clicks.get(), 1);
}

#[test]
fn hidden_updates_commit_without_redraw_then_show_latest_content() {
    let shown = Signal::new(false);
    let value = Signal::new(0);
    let (root, children) = Element::keyed_column();
    let mut scene = Scene::new(
        root.visible(&shown)
            .child(Element::bound_text(&value, |value| value.to_string())),
    );
    scene.flush(400.0, 600.0);
    scene.presented();
    value.update(|value| *value = 2);
    children.insert("hidden", Element::text("latest child"));
    assert_eq!(scene.flush(400.0, 600.0), 2);
    assert_eq!(scene.layouts, 1);
    assert!(!scene.needs_present());
    assert_eq!(scene.geometry_version, scene.commit_version);
    shown.update(|value| *value = true);
    scene.flush(400.0, 600.0);
    assert_eq!(scene.layouts, 2);
    assert!(scene.needs_present());
    assert_eq!(
        scene
            .nodes
            .iter()
            .filter_map(|node| node.text.as_deref())
            .collect::<Vec<_>>(),
        ["2", "latest child"]
    );
}
