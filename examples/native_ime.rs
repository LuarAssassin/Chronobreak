use chronobreak::{Application, Element, Result, Signal};

fn button(label: &str, action: impl Fn() + 'static) -> Element {
    Element::text(label)
        .font_size(16.0)
        .padding(4.0)
        .background([0.13, 0.21, 0.29, 1.0])
        .on_click(action)
}

fn main() -> Result<()> {
    let first = Signal::new(String::new());
    let second = Signal::new(String::new());
    let readonly = Signal::new(false);
    let disabled = Signal::new(false);
    let visible = Signal::new(true);
    let subscriptions = [
        first.subscribe(|text| eprintln!("model first={text:?}")),
        second.subscribe(|text| eprintln!("model second={text:?}")),
    ];
    let (fields, children) = Element::keyed_column();
    let initial_field = Element::text_input(&first)
        .readonly_when(&readonly)
        .disabled_when(&disabled)
        .visible(&visible)
        .background([0.08, 0.13, 0.20, 1.0]);
    let remove_children = children.clone();
    let (inner, _) = Element::scroll_view(
        120.0,
        Element::column()
            .gap(6.0)
            .child(Element::text("Nested viewport / second independent field"))
            .child(Element::text_input(&second).background([0.08, 0.13, 0.20, 1.0]))
            .child(Element::text("scroll\n↓\n↓\n↓\n↓\n↓")),
    );
    let (outer, _) = Element::scroll_view(
        100.0,
        Element::column()
            .child(Element::text("Outer viewport"))
            .child(inner)
            .child(Element::text("↓\n↓\n↓\n↓")),
    );
    let replace = first.clone();
    let root = Element::column()
        .padding(12.0)
        .gap(8.0)
        .child(Element::text("Native Chinese IME · X11 qualification probe").font_size(20.0))
        .child(Element::text("Tab / click to focus · Ctrl+A/C/Z · real system IME").font_size(14.0))
        .child(fields)
        .child(
            Element::bound_text(&first, |text| format!("First committed: {text}")).font_size(14.0),
        )
        .child(outer)
        .child(
            Element::bound_text(&second, |text| format!("Second committed: {text}"))
                .font_size(14.0),
        )
        .child(button("Toggle readonly", move || {
            readonly.update(|value| *value = !*value)
        }))
        .child(button("Toggle disabled", move || {
            disabled.update(|value| *value = !*value)
        }))
        .child(button("Toggle hidden", move || {
            visible.update(|value| *value = !*value)
        }))
        .child(button("Authoritative replace", move || {
            replace.update(|value| *value = "外部 replacement".into())
        }))
        .child(button("Remove first field", move || {
            if let Some(id) = remove_children.node("first") {
                remove_children.remove(id);
            }
        }));
    let app = Application::new(root).title("Chronobreak Native IME");
    children.insert("first", initial_field);
    let result = app.run();
    drop(subscriptions);
    result
}
