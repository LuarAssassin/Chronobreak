use chronobreak::{Application, Element, NodeId, Result, Signal};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn button(label: &str, click: impl Fn() + 'static) -> Element {
    Element::column()
        .padding(6.0)
        .background([0.12, 0.28, 0.52, 1.0])
        .on_click(click)
        .child(Element::text(label))
}

fn row(key: &str) -> Element {
    let count = Signal::new(0);
    let increment = count.clone();
    let label = key.to_owned();
    let trace_key = key.to_owned();
    Element::column()
        .padding(6.0)
        .background([0.075, 0.10, 0.16, 1.0])
        .on_click(move || {
            increment.update(|value| *value += 1);
            if std::env::var_os("CHRONOBREAK_TRACE").is_some() {
                eprintln!("row key={trace_key} count={}", increment.get());
            }
        })
        .child(Element::bound_text(&count, move |value| {
            format!("行 {label}：{value} / 点击 +1")
        }))
}

fn main() -> Result<()> {
    let (rows, children) = Element::keyed_column();
    let shown = Signal::new(true);
    let order = Rc::new(RefCell::new(vec![
        "1".to_owned(),
        "2".to_owned(),
        "3".to_owned(),
    ]));
    let next = Rc::new(Cell::new(4));
    let removed = Rc::new(RefCell::new(None::<String>));
    let stale = Rc::new(Cell::new(None::<NodeId>));
    let add = {
        let (children, order, removed) = (children.clone(), order.clone(), removed.clone());
        move || {
            let key = removed.borrow_mut().take().unwrap_or_else(|| {
                let key = next.get().to_string();
                next.set(next.get() + 1);
                key
            });
            children.insert(&key, row(&key));
            order.borrow_mut().push(key);
        }
    };
    let remove = {
        let (children, order, removed, stale) = (
            children.clone(),
            order.clone(),
            removed.clone(),
            stale.clone(),
        );
        move || {
            if let Some(key) = order.borrow_mut().pop()
                && let Some(id) = children.node(&key)
            {
                stale.set(Some(id));
                children.remove(id);
                *removed.borrow_mut() = Some(key);
            }
        }
    };
    let reverse = {
        let (children, order) = (children.clone(), order.clone());
        move || {
            order.borrow_mut().reverse();
            let ids: Vec<_> = order
                .borrow()
                .iter()
                .filter_map(|key| children.node(key))
                .collect();
            children.reorder(&ids);
        }
    };
    let toggle = shown.clone();
    let probe = children.clone();
    let root = Element::column()
        .padding(16.0)
        .gap(6.0)
        .child(Element::text("Chronobreak / 动态节点").font_size(24.0))
        .child(Element::text(
            "每行独立计数；隐藏保留，删除后同 key 重建归零。缩窄可观察中文换行与裁剪。",
        ))
        .child(button("新增 / 重建已删除行", add))
        .child(button("删除末行（真正卸载）", remove))
        .child(button("反转顺序（保留计数）", reverse))
        .child(button("隐藏 / 恢复所有行", move || {
            toggle.update(|shown| *shown = !*shown)
        }))
        .child(button("尝试旧 ID 删除（应拒绝）", move || {
            if let Some(id) = stale.get() {
                probe.remove(id);
            }
        }))
        .child(rows.gap(6.0).visible(&shown));
    let app = Application::new(root).title("Chronobreak — Node lifecycle");
    for key in ["1", "2", "3"] {
        children.insert(key, row(key));
    }
    app.run()
}
