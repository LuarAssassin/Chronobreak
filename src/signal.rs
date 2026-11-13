use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, VecDeque},
    rc::{Rc, Weak},
};

const PROPAGATION_BUDGET: usize = 1024;

struct Notification {
    run: Option<Box<dyn FnOnce()>>,
    queued: Weak<Cell<bool>>,
    depth: usize,
}

impl Drop for Notification {
    fn drop(&mut self) {
        if let Some(queued) = self.queued.upgrade() {
            queued.set(false);
        }
    }
}

#[derive(Default)]
struct Scheduler {
    queue: VecDeque<Notification>,
    batches: usize,
    draining: bool,
    depth: usize,
}

thread_local! {
    static SCHEDULER: RefCell<Scheduler> = RefCell::default();
}

struct Drain;

impl Drop for Drain {
    fn drop(&mut self) {
        SCHEDULER.with_borrow_mut(|scheduler| {
            scheduler.draining = false;
            scheduler.depth = 0;
            scheduler.queue.clear();
        });
    }
}

fn drain() {
    let start = SCHEDULER.with_borrow_mut(|scheduler| {
        if scheduler.draining || scheduler.batches != 0 {
            return false;
        }
        scheduler.draining = true;
        true
    });
    if !start {
        return;
    }
    let _guard = Drain;
    loop {
        let next = SCHEDULER.with_borrow_mut(|scheduler| scheduler.queue.pop_front());
        let Some(mut notification) = next else {
            break;
        };
        assert!(
            notification.depth < PROPAGATION_BUDGET,
            "Signal propagation budget exceeded (1024 causal notification steps); values retained, pending notifications discarded"
        );
        SCHEDULER.with_borrow_mut(|scheduler| scheduler.depth = notification.depth + 1);
        let run = notification.run.take().unwrap();
        drop(notification);
        run();
    }
}

struct Batch;

impl Drop for Batch {
    fn drop(&mut self) {
        let outermost = SCHEDULER.with_borrow_mut(|scheduler| {
            scheduler.batches -= 1;
            scheduler.batches == 0
        });
        if outermost {
            if std::thread::panicking() {
                SCHEDULER.with_borrow_mut(|scheduler| scheduler.queue.clear());
            } else {
                drain();
            }
        }
    }
}

/// Coalesce notifications until the outermost UI-thread batch returns.
/// Model writes are immediately readable. Returns the closure's result.
/// A panic retains model writes but discards pending notifications; no rollback.
pub fn batch<R>(update: impl FnOnce() -> R) -> R {
    SCHEDULER.with_borrow_mut(|scheduler| scheduler.batches += 1);
    let _guard = Batch;
    update()
}

type Listener<T> = Rc<dyn Fn(&T)>;

struct State<T> {
    value: T,
    version: u64,
    queued: Rc<Cell<bool>>,
    next: usize,
    listeners: BTreeMap<usize, Listener<T>>,
}

pub struct Signal<T>(Rc<RefCell<State<T>>>);

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

pub struct Subscription(Option<Box<dyn FnOnce()>>);

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(cancel) = self.0.take() {
            cancel();
        }
    }
}

impl<T: Clone + PartialEq + 'static> Signal<T> {
    pub fn new(value: T) -> Self {
        Self(Rc::new(RefCell::new(State {
            value,
            version: 0,
            queued: Rc::default(),
            next: 0,
            listeners: BTreeMap::new(),
        })))
    }

    pub fn get(&self) -> T {
        self.0.borrow().value.clone()
    }

    /// Notify synchronously outside batches, without recursive callback dispatch.
    /// Do not read/write this same Signal inside the mutation closure (RefCell borrow).
    pub fn update(&self, update: impl FnOnce(&mut T)) {
        {
            let mut state = self.0.borrow_mut();
            let previous = state.value.clone();
            update(&mut state.value);
            if previous == state.value {
                return;
            }
            state.version = state
                .version
                .checked_add(1)
                .expect("Signal version exhausted");
            if !state.queued.replace(true) {
                let weak = Rc::downgrade(&self.0);
                let queued = Rc::downgrade(&state.queued);
                SCHEDULER.with_borrow_mut(|scheduler| {
                    scheduler.queue.push_back(Notification {
                        depth: scheduler.depth,
                        queued,
                        run: Some(Box::new(move || {
                            let Some(owner) = weak.upgrade() else {
                                return;
                            };
                            let (value, version, tokens) = {
                                let state = owner.borrow();
                                (
                                    state.value.clone(),
                                    state.version,
                                    state.listeners.keys().copied().collect::<Vec<_>>(),
                                )
                            };
                            for token in tokens {
                                let listener = {
                                    let state = owner.borrow();
                                    if state.version != version {
                                        break;
                                    }
                                    state.listeners.get(&token).cloned()
                                };
                                if let Some(listener) = listener {
                                    listener(&value);
                                }
                            }
                        })),
                    });
                });
            }
        }
        drain();
    }

    /// Listen in registration order, without an initial notification.
    /// Cancellation suppresses callbacks not yet started. New subscriptions join
    /// the next dispatch; a newer write abandons the rest of an older dispatch.
    pub fn subscribe(&self, listener: impl Fn(&T) + 'static) -> Subscription {
        let mut state = self.0.borrow_mut();
        let token = state.next;
        state.next += 1;
        state.listeners.insert(token, Rc::new(listener));
        let weak = Rc::downgrade(&self.0);
        Subscription(Some(Box::new(move || {
            if let Some(state) = weak.upgrade() {
                let removed = state.borrow_mut().listeners.remove(&token);
                drop(removed);
            }
        })))
    }

    pub fn subscriber_count(&self) -> usize {
        self.0.borrow().listeners.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[test]
    fn reentrant_notification_never_rewinds_later_listener() {
        let signal = Signal::new(0);
        let writer = signal.clone();
        let _first = signal.subscribe(move |value| {
            if *value == 1 {
                writer.update(|value| *value = 2);
            }
        });
        let values = Rc::new(RefCell::new(Vec::new()));
        let observed = values.clone();
        let _second = signal.subscribe(move |value| observed.borrow_mut().push(*value));
        signal.update(|value| *value = 1);
        assert_eq!(signal.get(), 2);
        assert_eq!(*values.borrow(), [2]);
    }

    #[test]
    fn nested_batch_returns_and_coalesces_in_first_enqueue_order() {
        let left = Signal::new(0);
        let right = Signal::new(0);
        let values = Rc::new(RefCell::new(Vec::new()));
        let observed = values.clone();
        let _left = left.subscribe(move |value| observed.borrow_mut().push(("left", *value)));
        let observed = values.clone();
        let _right = right.subscribe(move |value| observed.borrow_mut().push(("right", *value)));
        let result = batch(|| {
            left.update(|value| *value = 1);
            batch(|| {
                right.update(|value| *value = 7);
                left.update(|value| *value = 2);
                assert_eq!(left.get(), 2);
            });
            assert!(values.borrow().is_empty());
            42
        });
        assert_eq!(result, 42);
        assert_eq!(*values.borrow(), [("left", 2), ("right", 7)]);
    }

    #[test]
    fn round_trip_notifies_once_but_unchanged_mutation_does_not() {
        let signal = Signal::new(0);
        let calls = Rc::new(Cell::new(0));
        let observed = calls.clone();
        let _subscription = signal.subscribe(move |value| {
            assert_eq!(*value, 0);
            observed.set(observed.get() + 1);
        });
        batch(|| {
            signal.update(|value| *value = 1);
            signal.update(|value| *value = 0);
        });
        signal.update(|_| {});
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn cancel_in_flight_and_new_subscription_waits_for_next_dispatch() {
        let signal = Signal::new(0);
        let second = Rc::new(RefCell::new(None));
        let cancelled = second.clone();
        let added = Rc::new(RefCell::new(Vec::new()));
        let subscriptions = added.clone();
        let readable = signal.clone();
        let calls = Rc::new(Cell::new(0));
        let observed = calls.clone();
        let _first = signal.subscribe(move |_| {
            cancelled.borrow_mut().take();
            let observed = observed.clone();
            subscriptions
                .borrow_mut()
                .push(readable.subscribe(move |_| observed.set(observed.get() + 1)));
        });
        *second.borrow_mut() = Some(signal.subscribe(|_| panic!("cancelled listener ran")));
        signal.update(|value| *value = 1);
        assert_eq!(calls.get(), 0);
        signal.update(|value| *value = 2);
        assert_eq!(calls.get(), 1);
        drop(added);
        drop(_first);
        assert_eq!(signal.subscriber_count(), 0);
    }

    #[test]
    fn pending_jobs_do_not_retain_owner_or_cancelled_listener() {
        let signal = Signal::new(0);
        let weak = Rc::downgrade(&signal.0);
        let lifetime = Rc::new(());
        let captured = lifetime.clone();
        let weak_listener = Rc::downgrade(&lifetime);
        let subscription = signal.subscribe(move |_| {
            let _ = &captured;
        });
        drop(lifetime);
        batch(|| {
            signal.update(|value| *value = 1);
            drop(subscription);
            assert!(weak_listener.upgrade().is_none());
            drop(signal);
            assert!(weak.upgrade().is_none());
        });
        SCHEDULER.with_borrow(|scheduler| assert!(scheduler.queue.is_empty()));
    }

    #[test]
    fn finite_reentry_has_constant_callback_stack() {
        let signal = Signal::new(0);
        let writer = signal.clone();
        let active = Rc::new(Cell::new(false));
        let observed = active.clone();
        let _subscription = signal.subscribe(move |value| {
            assert!(!observed.replace(true));
            if *value < 200 {
                writer.update(|value| *value += 1);
            }
            observed.set(false);
        });
        signal.update(|value| *value = 1);
        assert_eq!(signal.get(), 200);
    }

    #[test]
    fn budget_panic_is_observable_and_scheduler_recovers() {
        let left = Signal::new(0_u64);
        let right = Signal::new(0_u64);
        let writer = right.clone();
        let first = left.subscribe(move |value| writer.update(|target| *target = *value + 1));
        let writer = left.clone();
        let second = right.subscribe(move |value| writer.update(|target| *target = *value + 1));
        let error = catch_unwind(AssertUnwindSafe(|| left.update(|value| *value = 1))).unwrap_err();
        let message = error
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| error.downcast_ref::<String>().map(String::as_str))
            .unwrap();
        assert!(message.contains("propagation budget exceeded"));
        drop((first, second));
        let calls = Rc::new(Cell::new(0));
        let observed = calls.clone();
        let _subscription = left.subscribe(move |_| observed.set(observed.get() + 1));
        left.update(|value| *value += 1);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn independent_signals_do_not_consume_a_shared_loop_budget() {
        let signals: Vec<_> = (0..PROPAGATION_BUDGET * 2)
            .map(|_| Signal::new(0))
            .collect();
        let calls = Rc::new(Cell::new(0));
        let subscriptions: Vec<_> = signals
            .iter()
            .map(|signal| {
                let observed = calls.clone();
                signal.subscribe(move |_| observed.set(observed.get() + 1))
            })
            .collect();
        batch(|| {
            for signal in &signals {
                signal.update(|value| *value += 1);
            }
        });
        assert_eq!(calls.get(), subscriptions.len());
    }

    #[test]
    fn panic_discards_pending_without_rollback_and_allows_reuse() {
        let signal = Signal::new(0);
        let calls = Rc::new(Cell::new(0));
        let observed = calls.clone();
        let _subscription = signal.subscribe(move |_| observed.set(observed.get() + 1));
        assert!(
            catch_unwind(AssertUnwindSafe(|| batch(|| {
                signal.update(|value| *value = 1);
                batch(|| panic!("batch failed"));
            })))
            .is_err()
        );
        assert_eq!(signal.get(), 1);
        assert_eq!(calls.get(), 0);
        signal.update(|value| *value = 2);
        assert_eq!(calls.get(), 1);
        let panics = signal.subscribe(|_| panic!("listener failed"));
        assert!(catch_unwind(AssertUnwindSafe(|| signal.update(|value| *value = 3))).is_err());
        drop(panics);
        signal.update(|value| *value = 4);
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn mutation_closure_cannot_reborrow_same_signal() {
        let signal = Signal::new(0);
        assert!(
            catch_unwind(AssertUnwindSafe(|| signal.update(|_| {
                signal.get();
            })))
            .is_err()
        );
        assert!(
            catch_unwind(AssertUnwindSafe(|| signal.update(|_| signal.update(|_| {})))).is_err()
        );
        signal.update(|value| *value = 1);
        assert_eq!(signal.get(), 1);
    }

    #[test]
    fn updates_and_raii_cleanup() {
        let signal = Signal::new(0);
        let values = Rc::new(RefCell::new(Vec::new()));
        let observed = values.clone();
        let subscription = signal.subscribe(move |value| observed.borrow_mut().push(*value));
        signal.update(|value| *value += 1);
        signal.update(|_| {});
        assert_eq!(*values.borrow(), vec![1]);
        drop(subscription);
        assert_eq!(signal.subscriber_count(), 0);
        signal.update(|value| *value += 1);
        assert_eq!(*values.borrow(), vec![1]);
    }

    #[test]
    fn callback_can_read_signal_and_owner_can_drop_first() {
        let signal = Signal::new(0);
        let readable = signal.clone();
        let subscription = signal.subscribe(move |value| assert_eq!(*value, readable.get()));
        signal.update(|value| *value = 3);
        drop(subscription);
        let subscription = signal.subscribe(|_| {});
        drop(signal);
        drop(subscription);
    }
}
