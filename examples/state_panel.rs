use chronobreak::{Application, Element, Result, Signal};

fn button(label: Element, click: impl Fn() + 'static) -> Element {
    Element::column()
        .padding(12.0)
        .background([0.12, 0.28, 0.52, 1.0])
        .on_click(click)
        .child(label)
}

fn counter(label: &str) -> Element {
    let count = Signal::new(0_i32);
    let increment = count.clone();
    let label = label.to_owned();
    button(
        Element::bound_text(&count, move |value| format!("{label}：{value} / 点击 +1")),
        move || increment.update(|value| *value += 1),
    )
}

fn main() -> Result<()> {
    let message = Signal::new(String::from(
        "这是一段可变的中文说明，缩窄窗口可以观察自动换行。",
    ));
    let visible = Signal::new(true);
    let edit = message.clone();
    let toggle = visible.clone();
    let root = Element::column().padding(24.0).child(
        Element::column()
            .max_width(640.0)
            .padding(20.0)
            .gap(12.0)
            .background([0.075, 0.10, 0.16, 1.0])
            .child(Element::text("Chronobreak / 状态面板").font_size(26.0))
            .child(Element::bound_text(&message, |text| text))
            .child(counter("计数器 A"))
            .child(counter("计数器 B").visible(&visible))
            .child(button(Element::text("切换说明文字"), move || {
                edit.update(|text| {
                    *text = if text.starts_with("这是") {
                        "说明已切换：两个计数器彼此独立，隐藏内容再次显示时保留原有计数。".into()
                    } else {
                        "这是一段可变的中文说明，缩窄窗口可以观察自动换行。".into()
                    };
                })
            }))
            .child(button(
                Element::bound_text(&visible, |shown| {
                    if shown {
                        "隐藏计数器 B"
                    } else {
                        "恢复计数器 B"
                    }
                    .into()
                }),
                move || toggle.update(|shown| *shown = !*shown),
            )),
    );
    Application::new(root)
        .title("Chronobreak — State panel")
        .run()
}
