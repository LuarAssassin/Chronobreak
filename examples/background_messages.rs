use chronobreak::{
    Application, Element, MessageCancellation, MessageSender, Result, Signal, TrySendError,
    message_channel,
};
use std::{
    cell::{Cell, RefCell},
    num::NonZeroUsize,
    rc::Rc,
    thread,
    time::Duration,
};

struct Progress {
    request: u64,
    step: usize,
    total: usize,
}

struct Target {
    sender: MessageSender<Progress>,
    cancel: MessageCancellation,
    request: Rc<Cell<u64>>,
    status: Signal<String>,
}

fn target() -> (Element, Target) {
    let status = Signal::new("等待后台线程；无需点击、移动或缩放窗口。".to_owned());
    let model = status.clone();
    let request = Rc::new(Cell::new(0));
    let active = request.clone();
    let (sender, receiver) = message_channel(NonZeroUsize::new(128).unwrap());
    let cancel = receiver.cancellation();
    let element = Element::bound_text(&status, |value| value)
        .padding(10.0)
        .background([0.10, 0.27, 0.23, 1.0])
        .on_message(receiver, move |progress: Progress| {
            if progress.request != active.get() {
                return;
            }
            if progress.total == 10 || progress.step.is_multiple_of(1000) || progress.step == progress.total {
                eprintln!("callback request={} step={}/{}", progress.request, progress.step, progress.total);
            }
            model.update(|value| *value = format!(
                "请求 {}：进度 {}/{}。{}",
                progress.request, progress.step, progress.total,
                if progress.step == progress.total { "后台完成，窗口重新休眠。" }
                else { "中文段落会随窗口宽度自动换行；后台只发送数据，模型、几何与绘制仍归 UI 线程。" }
            ));
        });
    (
        element,
        Target {
            sender,
            cancel,
            request,
            status,
        },
    )
}

fn spawn(target: &Target, burst: bool) {
    if target.sender.is_closed() {
        eprintln!("spawn rejected: target closed; remove and restore it");
        return;
    }
    let request = target.request.get() + 1;
    target.request.set(request);
    let sender = target.sender.clone();
    thread::spawn(move || {
        let total = if burst { 20_000 } else { 10 };
        let mut full = 0;
        eprintln!("worker start request={request} burst={burst}");
        for _ in 0..30 {
            thread::sleep(Duration::from_millis(100));
            if sender.is_closed() {
                eprintln!("worker closed during delay request={request}");
                return;
            }
        }
        for step in 0..=total {
            let mut message = Progress {
                request,
                step,
                total,
            };
            loop {
                match sender.try_send(message) {
                    Ok(()) => break,
                    Err(TrySendError::Full(rejected)) => {
                        full += 1;
                        message = rejected;
                        thread::sleep(Duration::from_millis(16));
                    }
                    Err(TrySendError::Closed(_)) => {
                        eprintln!("worker closed request={request} step={step} full={full}");
                        return;
                    }
                }
            }
            if !burst {
                thread::sleep(Duration::from_millis(280));
            }
        }
        eprintln!("worker complete request={request} total={total} full={full}");
    });
}

fn control(label: &str, action: impl Fn() + 'static) -> Element {
    Element::text(label)
        .padding(6.0)
        .background([0.12, 0.25, 0.43, 1.0])
        .on_click(action)
}

fn main() -> Result<()> {
    let (inbox, initial) = target();
    let current = Rc::new(RefCell::new(Some(initial)));
    let (list, children) = Element::keyed_column();
    let start = current.clone();
    let cancel = current.clone();
    let remove = current.clone();
    let burst = current.clone();
    let container = children.clone();
    let expanded = Signal::new(false);
    let expand = expanded.clone();
    let clicks = Signal::new(0);
    let click = clicks.clone();
    let root = Element::column().padding(16.0).gap(6.0)
        .child(Element::text("M1c / 后台消息").font_size(24.0))
        .child(Element::text("启动后先休眠三秒，再由真实工作线程更新。可在后台突发期间点击、调整宽度，或移除目标。"))
        .child(control("新请求（忽略旧请求结果）", move || {
            if let Some(target) = start.borrow().as_ref() { spawn(target, false); }
        }))
        .child(control("取消收件箱", move || {
            if let Some(target) = cancel.borrow().as_ref() {
                target.cancel.close();
                target.status.update(|value| *value = "已取消；移除再恢复可创建新目标。".into());
                eprintln!("control cancel");
            }
        }))
        .child(control("移除／恢复目标", move || {
            if remove.borrow().is_some() {
                if let Some(id) = container.node("target") { container.remove(id); }
                remove.borrow_mut().take();
                eprintln!("control remove");
            } else {
                let (element, target) = target();
                container.insert("target", element);
                *remove.borrow_mut() = Some(target);
                eprintln!("control restore");
            }
        }))
        .child(control("有界突发 20,001 条（Full 保留并重试）", move || {
            if let Some(target) = burst.borrow().as_ref() { spawn(target, true); }
        }))
        .child(control("展开／收起中文段落", move || expand.update(|value| *value = !*value)))
        .child(control("响应测试：计数 +1", move || {
            click.update(|value| *value += 1);
            eprintln!("control click={}", click.get());
        }))
        .child(Element::bound_text(&clicks, |value| format!("点击计数：{value}")))
        .child(Element::bound_text(&expanded, |value| if value {
            "动态中文段落：后台消息与输入共享同一 UI 提交边界；展开后先完成必要布局，再按当前几何快照命中。".repeat(2)
        } else { "收起状态：保留目标与订阅。".into() }))
        .child(list);
    let app = Application::new(root).title("Chronobreak — Background messages");
    children.insert("target", inbox);
    spawn(current.borrow().as_ref().unwrap(), false);
    let result = app.run();
    if let Some(target) = current.borrow().as_ref() {
        let late = target.sender.try_send(Progress {
            request: 0,
            step: 0,
            total: 0,
        });
        eprintln!(
            "shutdown closed={} late_closed={}",
            target.sender.is_closed(),
            matches!(late, Err(TrySendError::Closed(_)))
        );
    }
    result
}
