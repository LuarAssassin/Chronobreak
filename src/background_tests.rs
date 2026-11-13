use super::*;
use crate::{Application, TrySendError, message_channel};
use std::{
    cell::Cell,
    num::NonZeroUsize,
    sync::atomic::{AtomicUsize, Ordering},
};

#[test]
fn finite_round_robin_turns_batch_updates_and_allow_input_between_turns() {
    let (sender, receiver) = message_channel(NonZeroUsize::new(160).unwrap());
    let value = Signal::new(0);
    let model = value.clone();
    let notifications = Rc::new(Cell::new(0));
    let count = notifications.clone();
    let _subscription = value.subscribe(move |_| count.set(count.get() + 1));
    let clicks = Rc::new(Cell::new(0));
    let clicked = clicks.clone();
    let mut scene = Scene::new(
        Element::bound_text(&value, |value| value.to_string())
            .on_message(receiver, move |value| {
                model.update(|current| *current = value)
            })
            .on_click(move || clicked.set(clicked.get() + 1)),
    );
    let wakes = Arc::new(AtomicUsize::new(0));
    let calls = wakes.clone();
    scene.wake.install(Arc::new(move || {
        calls.fetch_add(1, Ordering::SeqCst);
        true
    }));
    std::thread::spawn(move || {
        for value in 1..=150 {
            sender.try_send(value).unwrap();
        }
    })
    .join()
    .unwrap();
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    assert_eq!(scene.drain_messages(), 64);
    assert_eq!(value.get(), 64);
    assert_eq!(notifications.get(), 1);
    scene.flush(500.0, 300.0);
    let root = scene.root.unwrap();
    let bounds = scene.nodes[root].bounds;
    scene.pointer_button(true, bounds.x + 1.0, bounds.y + 1.0);
    scene.pointer_button(false, bounds.x + 1.0, bounds.y + 1.0);
    assert_eq!(clicks.get(), 1);
    assert_eq!(scene.drain_messages(), 64);
    assert_eq!(scene.drain_messages(), 22);
    assert_eq!(value.get(), 150);
    assert_eq!(notifications.get(), 3);
    assert_eq!(wakes.load(Ordering::SeqCst), 3);
    assert_eq!(scene.drain_messages(), 0);
    assert_eq!(wakes.load(Ordering::SeqCst), 3);
}

#[test]
fn inboxes_share_budget_without_starving_other_targets() {
    let (first, receiver) = message_channel(NonZeroUsize::new(100).unwrap());
    let (second, other) = message_channel(NonZeroUsize::new(1).unwrap());
    let observed = Rc::new(Cell::new(false));
    let seen = observed.clone();
    let mut scene = Scene::new(
        Element::column()
            .on_message(receiver, |_: i32| {})
            .child(Element::text("other").on_message(other, move |_: i32| seen.set(true))),
    );
    for value in 0..100 {
        first.try_send(value).unwrap();
    }
    second.try_send(1).unwrap();
    assert_eq!(scene.drain_messages(), 64);
    assert!(observed.get());
    assert_eq!(scene.drain_messages(), 37);
}

#[test]
fn committed_unmount_discards_payloads_and_captures_and_old_sender_cannot_hit_reused_slot() {
    let (root, children) = Element::keyed_column();
    let mut scene = Scene::new(root);
    let (sender, receiver) = message_channel(NonZeroUsize::new(2).unwrap());
    let payload = Arc::new(());
    let captured = Rc::new(());
    let weak = Rc::downgrade(&captured);
    children.insert(
        "target",
        Element::text("old").on_message(receiver, move |_: Arc<()>| {
            let _ = &captured;
            panic!("unmounted handler ran");
        }),
    );
    scene.sync();
    let old = children.node("target").unwrap();
    let slots = scene.nodes.slot_capacity();
    sender.try_send(payload.clone()).unwrap();
    children.remove(old);
    assert!(!sender.is_closed());
    assert_eq!(Arc::strong_count(&payload), 2);
    scene.sync();
    assert!(sender.is_closed());
    assert!(weak.upgrade().is_none());
    assert_eq!(Arc::strong_count(&payload), 1);
    let (fresh, receiver) = message_channel(NonZeroUsize::new(1).unwrap());
    let seen = Rc::new(Cell::new(0));
    let count = seen.clone();
    children.insert(
        "target",
        Element::text("new").on_message(receiver, move |value| count.set(value)),
    );
    scene.sync();
    assert_ne!(children.node("target").unwrap(), old);
    assert_eq!(scene.nodes.slot_capacity(), slots);
    assert!(matches!(
        sender.try_send(payload),
        Err(TrySendError::Closed(_))
    ));
    fresh.try_send(9).unwrap();
    assert_eq!(scene.drain_messages(), 1);
    assert_eq!(seen.get(), 9);
}

#[test]
fn cancellation_and_application_drop_release_pending_payloads_and_ui_captures() {
    for cancel_first in [true, false] {
        let (sender, receiver) = message_channel(NonZeroUsize::new(2).unwrap());
        let cancel = receiver.cancellation();
        let captured = Rc::new(());
        let weak = Rc::downgrade(&captured);
        let payload = Arc::new(());
        let app = Application::new(Element::text("target").on_message(
            receiver,
            move |_: Arc<()>| {
                let _ = &captured;
            },
        ));
        sender.try_send(payload.clone()).unwrap();
        if cancel_first {
            cancel.close();
            assert!(weak.upgrade().is_none());
            assert_eq!(Arc::strong_count(&payload), 1);
            assert!(sender.is_closed());
        }
        drop(app);
        assert!(weak.upgrade().is_none());
        assert_eq!(Arc::strong_count(&payload), 1);
        assert!(matches!(
            sender.try_send(payload),
            Err(TrySendError::Closed(_))
        ));
        cancel.close();
    }
}

#[test]
fn removal_queued_by_handler_commits_after_batch_before_next_delivery() {
    let (root, children) = Element::keyed_column();
    let mut scene = Scene::new(root);
    let (sender, receiver) = message_channel(NonZeroUsize::new(100).unwrap());
    let remove = children.clone();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    children.insert(
        "target",
        Element::text("target").on_message(receiver, move |_: i32| {
            count.set(count.get() + 1);
            if count.get() == 1 {
                remove.remove(remove.node("target").unwrap());
            }
        }),
    );
    scene.sync();
    for value in 0..100 {
        sender.try_send(value).unwrap();
    }
    assert_eq!(scene.drain_messages(), 64);
    assert_eq!(calls.get(), 64);
    assert!(sender.is_closed());
    assert_eq!(scene.drain_messages(), 0);
}

#[test]
fn shutdown_also_closes_receivers_in_not_yet_committed_insertions() {
    let (root, children) = Element::keyed_column();
    let mut scene = Scene::new(root);
    let (sender, receiver) = message_channel(NonZeroUsize::new(1).unwrap());
    let capture = Rc::new(());
    let weak = Rc::downgrade(&capture);
    let receipt = children.insert(
        "pending",
        Element::text("pending").on_message(receiver, move |_: i32| {
            let _ = &capture;
        }),
    );
    sender.try_send(1).unwrap();
    scene.close_messages();
    assert!(sender.is_closed());
    assert!(weak.upgrade().is_none());
    assert_eq!(receipt.result(), Some(Err(TreeError::SceneDropped)));
}
