use chronobreak::{
    Application, Children, Element, EventKind, EventPhase, Result, ScrollHandle, Signal,
    keyboard::{Key, NamedKey},
};

fn button(label: &str, action: impl Fn() + 'static) -> Element {
    Element::text(label)
        .font_size(14.0)
        .padding(5.0)
        .background([0.13, 0.21, 0.29, 1.0])
        .on_click(action)
}

fn populate(children: &Children, prefix: &str, count: usize) {
    for index in 0..count {
        let count = Signal::new(0);
        let increment = count.clone();
        let label = prefix.to_owned();
        let log_label = label.clone();
        children.insert(index.to_string(), Element::bound_text(&count, move |count| {
            format!("{label}第 {index:02} 行 · 独立计数 {count}：中文内容随窗口宽度重排，点击当前移动后的这一行。")
        }).font_size(16.0).padding(5.0).background([0.10, 0.16, 0.23, 1.0])
            .on_click(move || {
                increment.update(|count| *count += 1);
                eprintln!("demo click {log_label}{index:02} count={}", increment.get());
            }).on_event(EventPhase::Target, move |event, _| {
                if let EventKind::Key(key) = &event.kind
                    && key.logical_key == Key::Named(NamedKey::Tab) {
                    eprintln!("demo Tab from {index:02} modifiers={:?}", event.modifiers);
                }
            }));
    }
}

fn scroll_button(label: &str, handle: &ScrollHandle, delta: f32) -> Element {
    let handle = handle.clone();
    button(label, move || {
        handle.scroll_by(delta);
    })
}

fn main() -> Result<()> {
    let prevent = Signal::new(false);
    let (inner_content, inner_rows) = Element::keyed_column();
    let (inner, inner_handle) = Element::scroll_view(135.0, inner_content.gap(3.0));
    let (outer_rows_element, outer_rows) = Element::keyed_column();
    let (outer, outer_handle) = Element::scroll_view(
        255.0,
        Element::column()
            .gap(5.0)
            .child(Element::text("内层视口：先消费滚轮；到边缘后剩余量交给外层 ↓").font_size(14.0))
            .child(inner.background([0.08, 0.12, 0.18, 1.0]))
            .child(Element::text("外层长列表 ↓ Tab 会揭示离屏按钮；不卸载计数状态").font_size(14.0))
            .child(outer_rows_element.gap(3.0)),
    );
    let toggle = prevent.clone();
    let guard = prevent.clone();
    let reset_outer = outer_handle.clone();
    let reset_inner = inner_handle.clone();
    let reveal = outer_handle.clone();
    let reveal_rows = outer_rows.clone();
    let shrink_rows = outer_rows.clone();
    let shrink_inner = inner_rows.clone();
    let restore_rows = outer_rows.clone();
    let restore_inner = inner_rows.clone();
    let root = Element::column()
        .padding(8.0)
        .gap(3.0)
        .on_event(EventPhase::Capture, move |event, context| {
            if let EventKind::Wheel(wheel) = event.kind {
                eprintln!(
                    "demo wheel vertical={} prevent={} raw={:?}",
                    wheel.vertical,
                    guard.get(),
                    wheel.raw
                );
                if guard.get() {
                    context.prevent_default();
                }
            }
        })
        .child(Element::text("M2b 候选 · 仅纵向滚动 / 嵌套链 / 焦点揭示").font_size(20.0))
        .child(
            Element::text("右侧比例条只读；无横向、虚拟化、拖条或动画。缩窄窗口检查中文重排。")
                .font_size(12.0),
        )
        .child(button(
            "切换滚轮 prevent_default（不影响 Tab / 显式揭示）",
            move || {
                toggle.update(|value| *value = !*value);
                eprintln!("demo prevent={}", toggle.get());
            },
        ))
        .child(
            Element::bound_text(&prevent, |value| {
                format!("wheel prevent_default={value}；滚轮在指针实际命中路径消费")
            })
            .font_size(12.0),
        )
        .child(outer.background([0.06, 0.09, 0.14, 1.0]))
        .child(button(
            "显式揭示外层第 20 行（目标被删除则记录）",
            move || {
                if let Some(target) = reveal_rows.node("20") {
                    reveal.scroll_into_view(target);
                    eprintln!("demo reveal outer20");
                } else {
                    eprintln!("demo reveal target removed");
                }
            },
        ))
        .child(button(
            "缩减为各 2 行（同次提交 clamp）",
            move || {
                for rows in [&shrink_rows, &shrink_inner] {
                    for index in 2..30 {
                        if let Some(id) = rows.node(&index.to_string()) {
                            rows.remove(id);
                        }
                    }
                }
                eprintln!("demo shrink");
            },
        ))
        .child(button(
            "重建各列表（计数重置）；偏移复位",
            move || {
                for rows in [&restore_rows, &restore_inner] {
                    for index in 0..30 {
                        if let Some(id) = rows.node(&index.to_string()) {
                            rows.remove(id);
                        }
                    }
                }
                populate(&restore_rows, "外层", 30);
                populate(&restore_inner, "内层", 12);
                reset_outer.scroll_to(0.0);
                reset_inner.scroll_to(0.0);
                eprintln!("demo reset");
            },
        ))
        .child(scroll_button(
            "外层向下 100 逻辑像素（手动滚动不持续吸回焦点）",
            &outer_handle,
            100.0,
        ));
    let app = Application::new(root).title("Chronobreak — Vertical scroll M2b");
    populate(&outer_rows, "外层", 30);
    populate(&inner_rows, "内层", 12);
    app.run()
}
