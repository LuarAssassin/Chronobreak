use chronobreak::{Application, Element, Result, Signal, batch};

fn control(label: &str, action: impl Fn() + 'static) -> Element {
    Element::text(label)
        .padding(8.0)
        .background([0.12, 0.28, 0.52, 1.0])
        .on_click(action)
}

fn main() -> Result<()> {
    let reentrant = Signal::new(0);
    let writer = reentrant.clone();
    let _first = reentrant.subscribe(move |value| {
        if *value == 1 {
            writer.update(|value| *value = 2);
        }
    });
    let reenter = reentrant.clone();
    let batched = Signal::new(0);
    let increment = batched.clone();
    let expanded = Signal::new(false);
    let expand = expanded.clone();
    let clicks = Signal::new(0);
    let clicked = clicks.clone();
    let (prefix, children) = Element::keyed_column();
    let root = Element::column()
        .padding(16.0)
        .gap(8.0)
        .child(Element::text("M1b / Batched updates").font_size(24.0))
        .child(control("1. Reenter: write 1, listener writes 2", move || {
            reenter.update(|value| *value = 1);
            eprintln!("model reentry={}", reenter.get());
        }))
        .child(Element::bound_text(&reentrant, |value| {
            eprintln!("format reentry={value}");
            format!("Reentry final: {value}")
        }))
        .child(control("2. Batch: three writes, one format", move || {
            batch(|| {
                for _ in 0..3 {
                    increment.update(|value| *value += 1);
                }
                eprintln!("model batch={} (inside batch)", increment.get());
            });
        }))
        .child(Element::bound_text(&batched, |value| {
            eprintln!("format batch={value}");
            format!("Batch final: {value}")
        }))
        .child(control("3. Toggle wrapping text", move || {
            expand.update(|value| *value = !*value);
        }))
        .child(control("4. Insert/remove prefix above target", move || {
            if let Some(node) = children.node("prefix") {
                children.remove(node);
            } else {
                children.insert("prefix", Element::text("Inserted prefix / 动态插入后先布局再命中"));
            }
        }))
        .child(Element::bound_text(&expanded, |value| {
            if value {
                "Wrapping moves the target. Resize narrow, then click its NEW position. 中文换行与最新几何快照一致。".repeat(2)
            } else {
                "Short text / 短文字".into()
            }
        }))
        .child(prefix)
        .child(
            Element::column()
                .padding(8.0)
                .background([0.12, 0.42, 0.28, 1.0])
                .on_click(move || {
                    clicked.update(|value| *value += 1);
                    eprintln!("model target={}", clicked.get());
                })
                .child(Element::bound_text(&clicks, |value| format!("Target clicks: {value}"))),
        );
    Application::new(root)
        .title("Chronobreak — Batched updates")
        .run()
}
