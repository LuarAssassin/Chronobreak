use super::*;
use crate::render::retain_text_resources;
use std::cell::Cell;

fn row(value: &Signal<i32>) -> Element {
    let increment = value.clone();
    Element::column()
        .padding(4.0)
        .on_click(move || increment.update(|value| *value += 1))
        .child(Element::bound_text(value, |value| format!("计数：{value}")))
}

fn insert(scene: &mut Scene, children: &Children, key: &str, element: Element) -> NodeId {
    let receipt = children.insert(key, element);
    assert_eq!(receipt.result(), None);
    scene.sync();
    receipt.result().unwrap().unwrap().unwrap()
}

#[test]
fn keyed_insert_delete_reorder_preserves_survivors_and_resets_rebuilt_key() {
    let (root, children) = Element::keyed_column();
    let mut scene = Scene::new(root);
    let values = [Signal::new(1), Signal::new(2), Signal::new(3)];
    let ids: Vec<_> = ["a", "b", "c"]
        .into_iter()
        .zip(&values)
        .map(|(key, value)| insert(&mut scene, &children, key, row(value)))
        .collect();
    values[0].update(|value| *value = 11);
    let reordered = children.reorder(&[ids[2], ids[0], ids[1]]);
    scene.sync();
    assert_eq!(reordered.result(), Some(Ok(None)));
    assert_eq!(
        scene.nodes[scene.root.unwrap()].children,
        [ids[2], ids[0], ids[1]]
    );
    for (key, id) in ["a", "b", "c"].into_iter().zip(&ids) {
        assert_eq!(children.node(key), Some(*id));
    }
    assert!(texts(&scene).contains(&"计数：11".to_owned()));
    assert!(values.iter().all(|signal| signal.subscriber_count() == 1));
    let extra = insert(&mut scene, &children, "d", Element::text("新增"));
    children.move_before(extra, Some(ids[0]));
    scene.sync();
    assert_eq!(
        scene.nodes[scene.root.unwrap()].children,
        [ids[2], extra, ids[0], ids[1]]
    );
    children.remove(ids[1]);
    scene.sync();
    assert_eq!(values[1].subscriber_count(), 0);
    let fresh = Signal::new(0);
    let rebuilt = insert(&mut scene, &children, "b", row(&fresh));
    assert_ne!(rebuilt, ids[1]);
    assert_eq!(fresh.get(), 0);
    assert_eq!(values[0].get(), 11);
    assert_eq!(values[2].get(), 3);
    assert!(scene.nodes.get(ids[1]).is_none());
}

fn texts(scene: &Scene) -> Vec<String> {
    scene
        .nodes
        .iter()
        .filter_map(|node| node.buffer.as_ref())
        .map(|buffer| buffer.lines[0].text().to_owned())
        .collect()
}

#[test]
fn invalid_commands_are_diagnostic_and_do_not_mutate_either_scope() {
    let (root, children) = Element::keyed_column();
    let (nested_root, nested) = Element::keyed_column();
    let mut scene = Scene::new(root);
    let parent = insert(&mut scene, &children, "parent", nested_root);
    let child = insert(&mut scene, &nested, "child", Element::text("子"));
    let foreign_scene = Scene::new(Element::text("外部"));
    let duplicate = children.insert("parent", Element::text("重复"));
    let foreign = children.remove(foreign_scene.root.unwrap());
    let wrong_scope = children.remove(child);
    let self_move = children.move_before(parent, Some(parent));
    let ancestor_move = nested.move_before(parent, Some(child));
    let self_parent = nested.move_before(parent, None);
    let invalid_order = children.reorder(&[parent, parent]);
    let incomplete = children.reorder(&[]);
    scene.sync();
    for (receipt, error) in [
        (duplicate, TreeError::DuplicateKey("parent".into())),
        (foreign, TreeError::ForeignScene),
        (wrong_scope, TreeError::WrongContainer),
        (self_move, TreeError::InvalidMove),
        (ancestor_move, TreeError::WrongContainer),
        (self_parent, TreeError::WrongContainer),
        (invalid_order, TreeError::InvalidOrder),
        (incomplete, TreeError::InvalidOrder),
    ] {
        assert_eq!(receipt.result(), Some(Err(error)));
    }
    assert_eq!(scene.nodes[parent].children, [child]);
    assert_eq!(scene.nodes[child].parent, Some(parent));
    assert_eq!(scene.nodes.len(), 3);
    assert_eq!(foreign_scene.nodes.len(), 1);
}

#[test]
fn queued_update_unmount_slot_reuse_and_old_release_cannot_hit_new_node() {
    let (root, children) = Element::keyed_column();
    let mut scene = Scene::new(root);
    let old_value = Signal::new(0);
    let old = insert(&mut scene, &children, "a", row(&old_value));
    let old_text = scene.nodes[old].children[0];
    scene.layout(400.0, 600.0);
    scene.pointer_button(true, 5.0, 5.0);
    old_value.update(|value| *value = 100);
    children.remove(old);
    let new_value = Signal::new(0);
    let replacement = children.insert("a", row(&new_value));
    scene.sync();
    let new = replacement.result().unwrap().unwrap().unwrap();
    assert_ne!(old, new);
    assert_eq!(scene.nodes.slot_capacity(), 3);
    scene.pending.borrow_mut().insert(
        old_text,
        Update {
            text: Some(Box::new(999_i32)),
            visible: Some(false),
        },
    );
    assert_eq!(scene.sync(), 0);
    scene.layout(400.0, 600.0);
    assert!(!scene.pointer_button(false, 5.0, 5.0));
    assert_eq!(new_value.get(), 0);
    assert_eq!(old_value.subscriber_count(), 0);
    assert_eq!(scene.pending.borrow().len(), 0);
    let stale = children.remove(old);
    scene.sync();
    assert_eq!(stale.result(), Some(Err(TreeError::StaleNode)));
    assert_eq!(children.node("a"), Some(new));
    assert_eq!(texts(&scene), ["计数：0"]);
}

#[test]
fn already_copied_signal_listener_can_enqueue_after_unmount_but_is_ignored() {
    let (root, children) = Element::keyed_column();
    let scene = Rc::new(RefCell::new(Scene::new(root)));
    let source = Signal::new(0);
    let old_id = Rc::new(Cell::new(None));
    let latest = Signal::new(7);
    let weak_scene = Rc::downgrade(&scene);
    let callback_children = children.clone();
    let callback_old = old_id.clone();
    let callback_latest = latest.clone();
    let early_listener = source.subscribe(move |_| {
        callback_children.remove(callback_old.get().unwrap());
        callback_children.insert("a", row(&callback_latest));
        weak_scene.upgrade().unwrap().borrow_mut().sync();
    });
    let old = insert(&mut scene.borrow_mut(), &children, "a", row(&source));
    old_id.set(Some(old));
    source.update(|value| *value = 1);
    assert_eq!(source.subscriber_count(), 1);
    assert_eq!(scene.borrow().pending.borrow().len(), 1);
    assert_eq!(scene.borrow_mut().sync(), 0);
    assert_eq!(texts(&scene.borrow()), ["计数：7"]);
    drop(early_listener);
    assert_eq!(source.subscriber_count(), 0);
}

#[test]
fn subtree_unmount_drops_descendant_subscriptions_formatters_and_queued_payloads() {
    let (root, children) = Element::keyed_column();
    let mut scene = Scene::new(root);
    let (nested_root, nested) = Element::keyed_column();
    let signal = Signal::new(1);
    let formatter_lifetime = Rc::new(());
    let formatter_weak = Rc::downgrade(&formatter_lifetime);
    let parent = insert(
        &mut scene,
        &children,
        "parent",
        nested_root.child(Element::bound_text(&signal, move |value| {
            let _owner = &formatter_lifetime;
            format!("{value}")
        })),
    );
    signal.update(|value| *value = 2);
    let payload_lifetime = Rc::new(());
    let payload_weak = Rc::downgrade(&payload_lifetime);
    children.remove(parent);
    let queued = nested.insert(
        "never",
        Element::column().on_click(move || {
            let _owner = &payload_lifetime;
        }),
    );
    assert_eq!(scene.nodes.len(), 3);
    scene.sync();
    assert_eq!(queued.result(), Some(Err(TreeError::StaleNode)));
    assert_eq!(signal.subscriber_count(), 0);
    assert!(formatter_weak.upgrade().is_none());
    assert!(payload_weak.upgrade().is_none());
    assert_eq!(scene.nodes.len(), 1);
    assert_eq!(scene.tree.total_node_count(), 1);
    assert!(scene.pending.borrow().is_empty());
    assert!(scene.commands.borrow().is_empty());
    assert!(nested.node("never").is_none());
    let late = nested.insert("late", Element::text("迟到"));
    scene.sync();
    assert_eq!(late.result(), Some(Err(TreeError::StaleNode)));
}

#[test]
fn hidden_and_clipped_texts_skip_frame_resources_but_hidden_identity_survives() {
    let (root, children) = Element::keyed_column();
    let shown = Signal::new(true);
    let count = Signal::new(4);
    let mut scene = Scene::new(root);
    let id = insert(&mut scene, &children, "a", row(&count).visible(&shown));
    scene.layout(400.0, 600.0);
    let text_id = scene.nodes[id].children[0];
    assert!(
        scene
            .display_list()
            .iter()
            .any(|paint| matches!(paint, Paint::Text(node) if *node == text_id))
    );
    let mut resources = BTreeMap::from([(text_id, ())]);
    shown.update(|shown| *shown = false);
    scene.sync();
    scene.layout(400.0, 600.0);
    retain_text_resources(&mut resources, &BTreeSet::new());
    assert!(resources.is_empty());
    assert!(scene.display_list().is_empty());
    count.update(|count| *count = 9);
    shown.update(|shown| *shown = true);
    scene.sync();
    scene.layout(400.0, 600.0);
    assert_eq!(children.node("a"), Some(id));
    assert_eq!(texts(&scene), ["计数：9"]);
    assert_eq!(count.subscriber_count(), 1);
    scene.layout(400.0, 0.0);
    assert!(scene.display_list().is_empty());
}

#[test]
fn thousand_mount_unmount_cycles_bound_slots_and_logical_resources() {
    let (root, children) = Element::keyed_column();
    let mut scene = Scene::new(root);
    let baseline = scene.nodes.len();
    let mut resources = BTreeMap::new();
    let mut resource_high_water = 0;
    for _ in 0..1000 {
        let count = Signal::new(0);
        let shown = Signal::new(true);
        let lifetime = Rc::new(());
        let weak = Rc::downgrade(&lifetime);
        let element =
            row(&count)
                .visible(&shown)
                .child(Element::bound_text(&count, move |count| {
                    let _owner = &lifetime;
                    format!("中文 {count}")
                }));
        let id = insert(&mut scene, &children, "same", element);
        scene.layout(400.0, 600.0);
        let drawable: BTreeSet<_> = scene
            .display_list()
            .into_iter()
            .filter_map(|paint| match paint {
                Paint::Text(id) => Some(id),
                _ => None,
            })
            .collect();
        retain_text_resources(&mut resources, &drawable);
        for id in drawable {
            resources.insert(id, ());
        }
        resource_high_water = resource_high_water.max(resources.len());
        assert_eq!(count.subscriber_count(), 2);
        count.update(|count| *count += 1);
        children.remove(id);
        scene.sync();
        retain_text_resources(&mut resources, &BTreeSet::new());
        assert_eq!(scene.nodes.len(), baseline);
        assert_eq!(scene.tree.total_node_count(), baseline);
        assert_eq!(count.subscriber_count(), 0);
        assert_eq!(shown.subscriber_count(), 0);
        assert!(weak.upgrade().is_none());
        assert!(scene.pending.borrow().is_empty());
        assert!(scene.commands.borrow().is_empty());
        assert!(resources.is_empty());
        assert_eq!(scene.nodes.slot_capacity(), 4);
    }
    assert_eq!(resource_high_water, 2);
    eprintln!(
        "logical 1000 cycles: {} logical_text_cache=0 logical_text_high_water={resource_high_water}",
        scene.stats()
    );
}

#[test]
fn handles_and_receipts_report_unmounted_and_dropped_scenes_without_retention() {
    let (root, children) = Element::keyed_column();
    assert_eq!(
        children.insert("a", Element::text("before")).result(),
        Some(Err(TreeError::NotMounted))
    );
    let scene = Scene::new(root);
    let queued = children.insert("a", Element::text("pending"));
    drop(scene);
    assert_eq!(queued.result(), Some(Err(TreeError::SceneDropped)));
    assert_eq!(
        children.insert("b", Element::text("late")).result(),
        Some(Err(TreeError::SceneDropped))
    );
}

#[test]
fn callback_structure_changes_wait_for_commit_and_queued_reorder_refreshes_hits() {
    let (root, children) = Element::keyed_column();
    let mut scene = Scene::new(root);
    let own_id = Rc::new(Cell::new(None));
    let callback_id = own_id.clone();
    let callback_children = children.clone();
    let old = insert(
        &mut scene,
        &children,
        "self",
        Element::column()
            .padding(8.0)
            .child(Element::text("卸载自己"))
            .on_click(move || {
                let id = callback_id.get().unwrap();
                callback_children.remove(id);
                callback_children.insert("self", Element::text("新身份"));
                assert_eq!(callback_children.node("self"), Some(id));
            }),
    );
    own_id.set(Some(old));
    scene.layout(400.0, 600.0);
    scene.pointer_button(true, 5.0, 5.0);
    assert!(scene.pointer_button(false, 5.0, 5.0));
    assert_eq!(children.node("self"), Some(old));
    assert_eq!(scene.commands.borrow().len(), 2);
    scene.sync();
    assert_ne!(children.node("self"), Some(old));
    let left = Signal::new(0);
    let right = Signal::new(0);
    let first = insert(&mut scene, &children, "first", row(&left));
    let second = insert(&mut scene, &children, "second", row(&right));
    scene.layout(400.0, 600.0);
    let bounds = scene.nodes[first].bounds;
    children.move_before(second, Some(first));
    scene.pointer_button(true, bounds.x + 5.0, bounds.y + 5.0);
    scene.pointer_button(false, bounds.x + 5.0, bounds.y + 5.0);
    assert_eq!((left.get(), right.get()), (0, 1));
}
