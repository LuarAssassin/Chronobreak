//! 有界 Send 数据通道；接收器与处理闭包始终留在 UI 线程。
use std::{
    cell::RefCell,
    collections::VecDeque,
    num::NonZeroUsize,
    rc::Rc,
    sync::{Arc, Mutex, Weak},
};

/// 拒绝入队时归还原始负载。
#[derive(Debug, PartialEq, Eq)]
pub enum TrySendError<T> {
    Full(T),
    Closed(T),
}

type WakeHook = Arc<dyn Fn() -> bool + Send + Sync>;

#[derive(Default)]
struct WakeState {
    hook: Option<WakeHook>,
    scheduled: bool,
    closed: bool,
    queues: Vec<Weak<dyn CloseQueue>>,
}

#[derive(Default)]
pub(crate) struct Wake(Mutex<WakeState>);

impl Wake {
    pub(crate) fn install(&self, hook: WakeHook) {
        let (pending, old_hook) = {
            let mut state = self.0.lock().unwrap();
            if state.closed {
                return;
            }
            let old_hook = state.hook.replace(hook);
            (std::mem::take(&mut state.scheduled), old_hook)
        };
        drop(old_hook);
        if pending {
            self.notify();
        }
    }

    pub(crate) fn notify(&self) {
        let hook = {
            let mut state = self.0.lock().unwrap();
            if state.closed || state.scheduled {
                return;
            }
            state.scheduled = true;
            state.hook.clone()
        };
        if let Some(hook) = hook
            && !hook()
        {
            self.close();
        }
    }

    pub(crate) fn acknowledge(&self) {
        self.0.lock().unwrap().scheduled = false;
    }

    pub(crate) fn close(&self) {
        let (queues, hook) = {
            let mut state = self.0.lock().unwrap();
            state.closed = true;
            state.scheduled = false;
            (std::mem::take(&mut state.queues), state.hook.take())
        };
        drop(hook);
        for queue in queues.into_iter().filter_map(|queue| queue.upgrade()) {
            queue.close();
        }
    }

    fn register(&self, queue: Arc<dyn CloseQueue>) {
        let closed = {
            let mut state = self.0.lock().unwrap();
            state.queues.retain(|queue| queue.strong_count() > 0);
            if !state.closed {
                state.queues.push(Arc::downgrade(&queue));
            }
            state.closed
        };
        if closed {
            queue.close();
        }
    }

    fn unregister(&self, queue: Arc<dyn CloseQueue>) {
        let target = Arc::downgrade(&queue);
        self.0
            .lock()
            .unwrap()
            .queues
            .retain(|queue| !queue.ptr_eq(&target));
    }
}

trait CloseQueue: Send + Sync {
    fn close(&self);
}

struct QueueState<T> {
    values: VecDeque<T>,
    closed: bool,
    wake: Weak<Wake>,
}

struct Queue<T> {
    capacity: usize,
    state: Mutex<QueueState<T>>,
}

impl<T: Send> CloseQueue for Queue<T> {
    fn close(&self) {
        let values = {
            let mut state = self.state.lock().unwrap();
            state.closed = true;
            std::mem::take(&mut state.values)
        };
        drop(values);
    }
}

/// 可克隆的工作线程端点，不拥有 UI 处理闭包或节点。
pub struct MessageSender<T: Send + 'static>(Arc<Queue<T>>);

impl<T: Send + 'static> Clone for MessageSender<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Send + 'static> MessageSender<T> {
    /// 不等待队列空位；短临界区使用互斥锁，并非无锁接口。
    /// 入队成功不保证交付：取消、已提交卸载或关闭会丢弃队列中的消息。
    pub fn try_send(&self, value: T) -> Result<(), TrySendError<T>> {
        let wake = {
            let mut state = self.0.state.lock().unwrap();
            if state.closed {
                return Err(TrySendError::Closed(value));
            }
            if state.values.len() == self.0.capacity {
                return Err(TrySendError::Full(value));
            }
            state.values.push_back(value);
            state.wake.upgrade()
        };
        if let Some(wake) = wake {
            wake.notify();
        }
        Ok(())
    }

    pub fn is_closed(&self) -> bool {
        self.0.state.lock().unwrap().closed
    }
}

type Handler<T> = RefCell<Option<Box<dyn FnMut(T)>>>;

struct Local<T: Send + 'static> {
    queue: Arc<Queue<T>>,
    handler: Handler<T>,
}

impl<T: Send + 'static> Local<T> {
    fn close(&self) {
        self.queue.close();
        let wake = self.queue.state.lock().unwrap().wake.upgrade();
        if let Some(wake) = wake {
            wake.unregister(self.queue.clone());
        }
        let handler = self.handler.borrow_mut().take();
        drop(handler);
    }
}

/// UI 线程专属接收器，由 `Element::on_message` 消费。
pub struct MessageReceiver<T: Send + 'static>(Rc<Local<T>>);

/// UI 线程专属取消句柄，弱引用不延长目标生命周期。
pub struct MessageCancellation(Box<dyn Fn()>);

impl MessageCancellation {
    /// 丢弃排队数据并释放处理闭包；已开始的闭包执行至返回，不能被中断。
    pub fn close(&self) {
        (self.0)();
    }
}

impl<T: Send + 'static> MessageReceiver<T> {
    pub fn cancellation(&self) -> MessageCancellation {
        let local = Rc::downgrade(&self.0);
        MessageCancellation(Box::new(move || {
            if let Some(local) = local.upgrade() {
                local.close();
            }
        }))
    }

    pub fn close(&self) {
        self.0.close();
    }

    pub(crate) fn bind(self, handler: impl FnMut(T) + 'static) -> Box<dyn Route> {
        if !self.0.queue.state.lock().unwrap().closed {
            *self.0.handler.borrow_mut() = Some(Box::new(handler));
        }
        Box::new(self)
    }
}

impl<T: Send + 'static> Drop for MessageReceiver<T> {
    fn drop(&mut self) {
        self.close();
    }
}

/// 容量必须为正数。只有负载跨线程；接收器、取消句柄和处理闭包留在 UI 线程。
///
/// ```compile_fail
/// use chronobreak::message_channel;
/// let (_, receiver) = message_channel::<u32>(std::num::NonZeroUsize::new(1).unwrap());
/// std::thread::spawn(move || drop(receiver));
/// ```
///
/// ```compile_fail
/// let model = chronobreak::Signal::new(0);
/// std::thread::spawn(move || model.update(|value| *value += 1));
/// ```
///
/// ```no_run
/// use chronobreak::{Application, Element, Signal, message_channel};
/// use std::num::NonZeroUsize;
/// let progress = Signal::new(0);
/// let (sender, receiver) = message_channel(NonZeroUsize::new(8).unwrap());
/// let model = progress.clone();
/// let root = Element::bound_text(&progress, |value| format!("进度 {value}"))
///     .on_message(receiver, move |value| model.update(|current| *current = value));
/// std::thread::spawn(move || {
///     if let Err(error) = sender.try_send(42) { eprintln!("提交失败：{error:?}"); }
/// });
/// Application::new(root).run().unwrap();
/// ```
pub fn message_channel<T: Send + 'static>(
    capacity: NonZeroUsize,
) -> (MessageSender<T>, MessageReceiver<T>) {
    let queue = Arc::new(Queue {
        capacity: capacity.get(),
        state: Mutex::new(QueueState {
            values: VecDeque::new(),
            closed: false,
            wake: Weak::new(),
        }),
    });
    (
        MessageSender(queue.clone()),
        MessageReceiver(Rc::new(Local {
            queue,
            handler: RefCell::new(None),
        })),
    )
}

pub(crate) trait Route {
    fn attach(&self, wake: &Arc<Wake>);
    fn dispatch_one(&self) -> bool;
    fn pending(&self) -> bool;
}

#[cfg(test)]
#[path = "message_tests.rs"]
mod tests;

impl<T: Send + 'static> Route for MessageReceiver<T> {
    fn attach(&self, wake: &Arc<Wake>) {
        let pending = {
            let mut state = self.0.queue.state.lock().unwrap();
            state.wake = Arc::downgrade(wake);
            !state.values.is_empty()
        };
        wake.register(self.0.queue.clone());
        if pending {
            wake.notify();
        }
    }

    fn dispatch_one(&self) -> bool {
        let value = self.0.queue.state.lock().unwrap().values.pop_front();
        let Some(value) = value else {
            return false;
        };
        let handler = self.0.handler.borrow_mut().take();
        if let Some(mut handler) = handler {
            handler(value);
            if !self.0.queue.state.lock().unwrap().closed {
                *self.0.handler.borrow_mut() = Some(handler);
            }
        }
        true
    }

    fn pending(&self) -> bool {
        !self.0.queue.state.lock().unwrap().values.is_empty()
    }
}
