use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

type Listener<T> = Rc<dyn Fn(&T)>;

struct State<T> {
    value: T,
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
            next: 0,
            listeners: BTreeMap::new(),
        })))
    }

    pub fn get(&self) -> T {
        self.0.borrow().value.clone()
    }

    pub fn update(&self, update: impl FnOnce(&mut T)) {
        let (value, listeners) = {
            let mut state = self.0.borrow_mut();
            let previous = state.value.clone();
            update(&mut state.value);
            if previous == state.value {
                return;
            }
            (
                state.value.clone(),
                state.listeners.values().cloned().collect::<Vec<_>>(),
            )
        };
        for listener in listeners {
            listener(&value);
        }
    }

    pub fn subscribe(&self, listener: impl Fn(&T) + 'static) -> Subscription {
        let mut state = self.0.borrow_mut();
        let token = state.next;
        state.next += 1;
        state.listeners.insert(token, Rc::new(listener));
        let weak = Rc::downgrade(&self.0);
        Subscription(Some(Box::new(move || {
            if let Some(state) = weak.upgrade() {
                state.borrow_mut().listeners.remove(&token);
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
