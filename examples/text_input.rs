use chronobreak::{Application, Element, Result, Signal};

fn button(label: &str, action: impl Fn() + 'static) -> Element {
    Element::text(label)
        .font_size(14.0)
        .padding(6.0)
        .background([0.13, 0.21, 0.29, 1.0])
        .on_click(action)
}

fn main() -> Result<()> {
    let value = Signal::new("你好 Chronobreak 👩🏽‍💻 · ASCII / 中文 / emoji".to_owned());
    let readonly = Signal::new(false);
    let disabled = Signal::new(false);
    let toggle_readonly = readonly.clone();
    let toggle_disabled = disabled.clone();
    let replace = value.clone();
    let log = value.subscribe(|value| eprintln!("text-input value={value:?}"));
    let (nested, _) = Element::scroll_view(
        110.0,
        Element::column()
            .child(
                Element::text("Nested vertical scroll: Tab reveals the second field")
                    .font_size(14.0),
            )
            .child(Element::text("↓\n↓\n↓\n↓").font_size(18.0))
            .child(
                Element::text_input(&value)
                    .readonly_when(&readonly)
                    .disabled_when(&disabled)
                    .background([0.08, 0.13, 0.20, 1.0]),
            ),
    );
    let root = Element::column().padding(12.0).gap(8.0)
        .child(Element::text("Single-line TextInput · M3B candidate").font_size(22.0))
        .child(Element::text("Ctrl+A/C/X/V/Z/Y · Shift+arrows · Home/End · Tab\nNative IME: see native_ime example and docs/native-ime.md").font_size(14.0))
        .child(Element::text_input(&value).readonly_when(&readonly).disabled_when(&disabled)
            .background([0.08, 0.13, 0.20, 1.0]))
        .child(Element::bound_text(&value, |value| format!("Controlled sync: {} bytes · two fields, one Signal", value.len())).font_size(14.0))
        .child(button("Toggle readonly", move || toggle_readonly.update(|value| *value = !*value)))
        .child(button("Toggle disabled", move || toggle_disabled.update(|value| *value = !*value)))
        .child(button("External replace / clear history", move || replace.update(|value| *value = "外部替换 External replacement 🦀".to_owned())))
        .child(Element::bound_text(&readonly, |value| format!("readonly={value}")).font_size(14.0))
        .child(Element::bound_text(&disabled, |value| format!("disabled={value}")).font_size(14.0))
        .child(nested);
    let result = Application::new(root)
        .title("Chronobreak TextInput M3B")
        .run();
    drop(log);
    result
}
