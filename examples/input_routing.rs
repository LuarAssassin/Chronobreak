use chronobreak::{
    Application, Element, ElementState, EventContext, EventKind, EventPhase, MouseButton, Result,
    Signal, UiEvent,
    keyboard::{Key, NamedKey},
};

fn log(state: &Signal<Vec<String>>, message: String) {
    eprintln!("demo {message}");
    state.update(|lines| {
        lines.push(message);
        if lines.len() > 4 {
            lines.remove(0);
        }
    });
}

fn routed(mut element: Element, name: &'static str, trace: &Signal<Vec<String>>) -> Element {
    for phase in [EventPhase::Capture, EventPhase::Target, EventPhase::Bubble] {
        let trace = trace.clone();
        element = element.on_event(phase, move |event, context| {
            let interesting = matches!(
                event.kind,
                EventKind::PointerDown(_) | EventKind::PointerUp(_)
            ) || matches!(&event.kind, EventKind::Key(key) if key.state == ElementState::Pressed);
            if interesting {
                log(
                    &trace,
                    format!(
                        "{name} {:?} stop={} default={}",
                        context.phase,
                        context.propagation_stopped(),
                        context.default_prevented()
                    ),
                );
            }
        });
    }
    element
}

fn button(label: &str, action: impl Fn() + 'static) -> Element {
    Element::column()
        .padding(5.0)
        .background([0.14, 0.22, 0.31, 1.0])
        .on_click(action)
        .child(Element::text(label).font_size(16.0))
}

fn main() -> Result<()> {
    let count = Signal::new(0_usize);
    let disabled = Signal::new(false);
    let shown = Signal::new(true);
    let stop = Signal::new(false);
    let prevent = Signal::new(false);
    let trace = Signal::new(Vec::<String>::new());
    let drag_state = Signal::new(String::from(
        "拖动区域：按住左键，移出控件后松开；Esc 显式释放",
    ));
    let (list, children) = Element::keyed_column();
    let removable = {
        let (count, disabled, shown) = (count.clone(), disabled.clone(), shown.clone());
        move || {
            let count = count.clone();
            button("按钮 B：可禁用 / 隐藏 / Ctrl+R 删除", move || {
                count.update(|count| *count += 1);
                eprintln!("demo activate B total={}", count.get());
            })
            .disabled_when(&disabled)
            .visible(&shown)
        }
    };
    let increment = count.clone();
    let disabled_toggle = disabled.clone();
    let shown_toggle = shown.clone();
    let stop_toggle = stop.clone();
    let prevent_toggle = prevent.clone();
    let remove = children.clone();
    let restore = children.clone();
    let shortcut_trace = trace.clone();
    let guard = move |event: &UiEvent, context: &mut EventContext| {
        if let EventKind::Key(key) = &event.kind {
            eprintln!(
                "demo key logical={:?} physical={:?} modifiers={:?} state={:?} repeat={}",
                key.logical_key, key.physical_key, event.modifiers, key.state, key.repeat
            );
            if key.state == ElementState::Pressed && !key.repeat {
                if event.modifiers.control_key() && key.logical_key == Key::Character("r".into()) {
                    if let Some(id) = remove.node("b") {
                        remove.remove(id);
                    }
                    context.prevent_default();
                    log(
                        &shortcut_trace,
                        "根作用域 Ctrl+R：排队删除 B；焦点回退首项".into(),
                    );
                }
                if key.logical_key == Key::Named(NamedKey::Escape) {
                    context.release_pointer();
                    context.prevent_default();
                    log(&shortcut_trace, "Esc：显式释放捕获".into());
                }
            }
        }
    };
    let target_trace = trace.clone();
    let target_stop = stop.clone();
    let target_prevent = prevent.clone();
    let label = Element::text("按钮 A：标签是真正 target；Enter / Space 激活")
        .font_size(16.0)
        .on_event(EventPhase::Target, move |event, context| {
            if matches!(
                event.kind,
                EventKind::PointerDown(_) | EventKind::PointerUp(_)
            ) {
                if target_stop.get() {
                    context.stop_propagation();
                }
                if target_prevent.get() {
                    context.prevent_default();
                }
                log(
                    &target_trace,
                    format!(
                        "标签 Target stop={} default={}",
                        context.propagation_stopped(),
                        context.default_prevented()
                    ),
                );
            }
        });
    let drag_text = drag_state.clone();
    let drag_trace = trace.clone();
    let drag = Element::bound_text(&drag_state, |text| text)
        .font_size(16.0)
        .padding(10.0)
        .focusable(true)
        .background([0.20, 0.19, 0.14, 1.0])
        .on_event(EventPhase::Target, move |event, context| match event.kind {
            EventKind::PointerDown(MouseButton::Left) => {
                context.capture_pointer();
                drag_text.update(|text| *text = "捕获中：移出控件再松开；Esc 释放".into());
                log(&drag_trace, "drag capture".into());
            }
            EventKind::PointerMove if event.position.is_some() => {
                eprintln!("demo drag move {:?}", event.position);
            }
            EventKind::PointerUp(MouseButton::Left) => {
                log(&drag_trace, format!("drag up {:?}", event.position));
            }
            EventKind::PointerCancel | EventKind::CaptureLost => {
                drag_text.update(|text| *text = "捕获已释放 / 取消；可重新拖动".into());
                log(&drag_trace, format!("drag {:?}", event.kind));
            }
            _ => {}
        });
    let root = routed(Element::column().padding(10.0).gap(4.0), "外层", &trace)
        .on_event(EventPhase::Capture, guard)
        .child(Element::text("M2a / 路由、焦点与捕获").font_size(22.0))
        .child(Element::text("Tab / Shift+Tab；Enter / Space。Ctrl+R 删除 B，Esc 释放拖动。无滚动 / 编辑 / IME。").font_size(14.0))
        .child(routed(Element::column().padding(5.0).gap(4.0).background([0.07, 0.10, 0.15, 1.0]), "内层面板", &trace)
            .child(Element::column().padding(5.0).background([0.14, 0.22, 0.31, 1.0])
                .on_click(move || {
                    increment.update(|count| *count += 1);
                    eprintln!("demo activate A total={}", increment.get());
                }).child(label))
            .child(list))
        .child(button("切换 B 禁用", move || disabled_toggle.update(|disabled| *disabled = !*disabled)))
        .child(button("切换 B 隐藏", move || shown_toggle.update(|shown| *shown = !*shown)))
        .child(button("删除 / 重建 B", move || {
            if let Some(id) = restore.node("b") { restore.remove(id); }
            else { restore.insert("b", removable()); }
        }))
        .child(button("切换标签 stop_propagation（不阻止默认动作）", move || stop_toggle.update(|stop| *stop = !*stop)))
        .child(button("切换标签 prevent_default（阻止默认聚焦 / 激活）", move || prevent_toggle.update(|prevent| *prevent = !*prevent)))
        .child(drag)
        .child(Element::bound_text(&count, |count| format!("有效激活总数：{count}")).font_size(16.0))
        .child(Element::bound_text(&disabled, |disabled| format!("B disabled={disabled}")).font_size(14.0))
        .child(Element::bound_text(&trace, |lines| lines.join("\n")).font_size(12.0));
    let app = Application::new(root).title("Chronobreak — Input routing M2a");
    let initial_count = count.clone();
    children.insert(
        "b",
        button("按钮 B：可禁用 / 隐藏 / Ctrl+R 删除", move || {
            initial_count.update(|count| *count += 1);
            eprintln!("demo activate B total={}", initial_count.get());
        })
        .disabled_when(&disabled)
        .visible(&shown),
    );
    app.run()
}
