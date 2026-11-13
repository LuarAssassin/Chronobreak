use super::*;
use std::{
    cell::Cell,
    sync::{
        Barrier,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
};

fn channel<T: Send + 'static>(capacity: usize) -> (MessageSender<T>, MessageReceiver<T>) {
    message_channel(NonZeroUsize::new(capacity).unwrap())
}

fn counted_wake() -> (Arc<Wake>, Arc<AtomicUsize>) {
    let wake = Arc::new(Wake::default());
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    wake.install(Arc::new(move || {
        calls.fetch_add(1, Ordering::SeqCst);
        true
    }));
    (wake, count)
}

#[test]
fn worker_fifo_capacity_and_closed_preserve_rejected_payloads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MessageSender<String>>();
    let ui = thread::current().id();
    let (sender, receiver) = channel::<String>(2);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let values = seen.clone();
    let route = receiver.bind(move |value| {
        assert_eq!(thread::current().id(), ui);
        values.borrow_mut().push(value);
    });
    let worker = thread::spawn(move || {
        assert_ne!(thread::current().id(), ui);
        sender.try_send("first".into()).unwrap();
        sender.try_send("second".into()).unwrap();
        assert_eq!(
            sender.try_send("third".into()),
            Err(TrySendError::Full("third".into()))
        );
        sender
    });
    let sender = worker.join().unwrap();
    assert!(seen.borrow().is_empty());
    assert!(route.dispatch_one());
    assert!(route.dispatch_one());
    assert!(!route.dispatch_one());
    assert_eq!(*seen.borrow(), ["first", "second"]);
    drop(route);
    assert_eq!(
        sender.try_send("late".into()),
        Err(TrySendError::Closed("late".into()))
    );
}

#[test]
fn pre_mount_and_pre_hook_messages_coalesce_without_polling() {
    let (sender, receiver) = channel(4);
    sender.try_send(1).unwrap();
    let route = receiver.bind(|_| {});
    let wake = Arc::new(Wake::default());
    route.attach(&wake);
    sender.try_send(2).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    wake.install(Arc::new(move || {
        calls.fetch_add(1, Ordering::SeqCst);
        true
    }));
    sender.try_send(3).unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    wake.acknowledge();
    while route.dispatch_one() {}
    assert!(!route.pending());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    sender.try_send(4).unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn deterministic_sends_on_each_side_of_acknowledge_and_final_pop() {
    let (sender, receiver) = channel(8);
    let route = receiver.bind(|_| {});
    let (wake, count) = counted_wake();
    route.attach(&wake);
    let (step, steps) = mpsc::channel();
    let (done, completed) = mpsc::channel();
    let worker = thread::spawn(move || {
        for value in steps {
            sender.try_send(value).unwrap();
            done.send(()).unwrap();
        }
    });
    step.send(1).unwrap();
    completed.recv().unwrap();
    step.send(2).unwrap();
    completed.recv().unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    wake.acknowledge();
    step.send(3).unwrap();
    completed.recv().unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);
    for _ in 0..3 {
        assert!(route.dispatch_one());
    }
    assert!(!route.pending());
    step.send(4).unwrap();
    completed.recv().unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);
    wake.acknowledge();
    assert!(route.dispatch_one());
    step.send(5).unwrap();
    completed.recv().unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 3);
    assert!(route.dispatch_one());
    drop(step);
    worker.join().unwrap();
}

#[test]
fn producer_can_enqueue_while_wake_hook_is_in_flight() {
    let (sender, receiver) = channel(4);
    let route = receiver.bind(|_| {});
    let wake = Arc::new(Wake::default());
    route.attach(&wake);
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let hook_entered = entered.clone();
    let hook_release = release.clone();
    wake.install(Arc::new(move || {
        hook_entered.wait();
        hook_release.wait();
        true
    }));
    let other = sender.clone();
    let worker = thread::spawn(move || sender.try_send(1));
    entered.wait();
    other.try_send(2).unwrap();
    assert!(route.dispatch_one());
    assert!(route.dispatch_one());
    release.wait();
    worker.join().unwrap().unwrap();
}

#[test]
fn callbacks_and_payload_drops_run_without_queue_or_wake_locks() {
    struct Probe(Box<dyn Fn() + Send>);
    impl Drop for Probe {
        fn drop(&mut self) {
            (self.0)();
        }
    }
    let (sender, receiver) = channel::<Probe>(2);
    let queue = Arc::downgrade(&sender.0);
    let route = receiver.bind(move |_| {
        assert!(queue.upgrade().unwrap().state.try_lock().is_ok());
    });
    let (wake, _) = counted_wake();
    route.attach(&wake);
    for _ in 0..2 {
        let queue = Arc::downgrade(&sender.0);
        let wake = wake.clone();
        assert!(
            sender
                .try_send(Probe(Box::new(move || {
                    assert!(queue.upgrade().unwrap().state.try_lock().is_ok());
                    assert!(wake.0.try_lock().is_ok());
                })))
                .is_ok()
        );
    }
    assert!(route.dispatch_one());
    drop(route);
    assert!(sender.is_closed());
}

#[test]
fn cancellation_in_handler_is_immediate_and_releases_capture() {
    let (sender, receiver) = channel(3);
    let cancel = receiver.cancellation();
    let captured = Rc::new(Cell::new(0));
    let weak = Rc::downgrade(&captured);
    let route = receiver.bind(move |_| {
        captured.set(captured.get() + 1);
        cancel.close();
    });
    sender.try_send(1).unwrap();
    sender.try_send(2).unwrap();
    assert!(route.dispatch_one());
    assert!(!route.dispatch_one());
    assert!(weak.upgrade().is_none());
    assert_eq!(sender.try_send(3), Err(TrySendError::Closed(3)));
}

#[test]
fn failed_proxy_closes_all_registered_queues_without_rearm_or_payload_leaks() {
    let drops = Arc::new(());
    let (first, receiver) = channel(2);
    let (second, other) = channel(2);
    let route = receiver.bind(|_: Arc<()>| {});
    let other_route = other.bind(|_: Arc<()>| {});
    let wake = Arc::new(Wake::default());
    route.attach(&wake);
    other_route.attach(&wake);
    first.try_send(drops.clone()).unwrap();
    second.try_send(drops.clone()).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    wake.install(Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
        false
    }));
    assert!(first.is_closed() && second.is_closed());
    assert_eq!(Arc::strong_count(&drops), 1);
    for _ in 0..3 {
        wake.acknowledge();
        wake.notify();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!route.pending() && !other_route.pending());
    let (late, receiver) = channel(1);
    receiver.bind(|_: i32| {}).attach(&wake);
    assert_eq!(late.try_send(0), Err(TrySendError::Closed(0)));
}

#[test]
fn cancellation_before_binding_drops_capture_and_does_not_wake() {
    let (sender, receiver) = channel::<i32>(1);
    let cancel = receiver.cancellation();
    sender.try_send(1).unwrap();
    cancel.close();
    let capture = Rc::new(());
    let weak = Rc::downgrade(&capture);
    let route = receiver.bind(move |_| {
        let _ = &capture;
    });
    let (wake, count) = counted_wake();
    route.attach(&wake);
    assert!(!route.dispatch_one());
    assert!(weak.upgrade().is_none());
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(sender.try_send(2), Err(TrySendError::Closed(2)));
}

#[test]
fn retained_senders_do_not_accumulate_dead_routes_in_the_wake_registry() {
    let (wake, _) = counted_wake();
    let mut senders = Vec::new();
    for _ in 0..100 {
        let (sender, receiver) = channel::<i32>(1);
        let route = receiver.bind(|_| {});
        route.attach(&wake);
        drop(route);
        senders.push(sender);
    }
    assert!(wake.0.lock().unwrap().queues.is_empty());
    assert!(senders.iter().all(MessageSender::is_closed));
}
