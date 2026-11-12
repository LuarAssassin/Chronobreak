use chronobreak::{Application, Element, Result, Signal};

fn main() -> Result<()> {
    let count = Signal::new(0_u64);
    let increment = count.clone();
    let root = Element::column().padding(28.0).child(
        Element::column().max_width(640.0).padding(24.0).gap(18.0)
            .background([0.075, 0.10, 0.16, 1.0])
            .child(Element::text("Chronobreak / 自绘 GUI").font_size(28.0))
            .child(Element::text("这是一个由 Rust 构建的保留模式信息卡片。文字使用真实字体排版，调整窗口宽度即可观察中文自动换行与 Flexbox 布局变化。"))
            .child(Element::text("原生 Rust builder · 响应式状态 · GPU 绘制").font_size(15.0))
            .child(Element::column().padding(16.0).background([0.12, 0.28, 0.52, 1.0])
                .on_click(move || increment.update(|value| *value += 1))
                .child(Element::bound_text(&count, |value| format!("点击计数：{value}  /  点击这里 +1"))))
    );
    Application::new(root).title("Chronobreak — Counter").run()
}
