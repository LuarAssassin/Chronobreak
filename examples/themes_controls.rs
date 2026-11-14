use chronobreak::{
    Application, Element, EventKind, EventPhase, Result, SemanticAction, Semantics, Signal,
    ThemeColor, ThemeController, ThemeMode,
    keyboard::{Key, NamedKey},
    message_channel,
};
use std::{num::NonZeroUsize, time::Duration};

fn main() -> Result<()> {
    let verify = std::env::args().any(|argument| argument == "--verify");
    let theme = ThemeController::default();
    let semantics = Semantics::default();
    let enabled = Signal::new(true);
    let expanded = Signal::new(true);
    let invalid = Signal::new(true);
    let first = Signal::new("Knowledge source / 中文资料".to_owned());
    let second = Signal::new("Read only / 保留源文".to_owned());
    let progress = Signal::new(0_u32);
    let (sender, receiver) = message_channel(NonZeroUsize::new(16).unwrap());
    let worker = std::thread::spawn(move || {
        for step in 1..=100 {
            std::thread::sleep(Duration::from_millis(120));
            if sender.try_send(step).is_err() {
                break;
            }
        }
    });
    let toggle_theme = theme.clone();
    let switch = move || {
        toggle_theme.set(match toggle_theme.mode() {
            ThemeMode::Light => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::Light,
        })
    };
    let key_theme = theme.clone();
    let key_semantics = semantics.clone();
    let key_progress = progress.clone();
    let (tree, children) = Element::keyed_column();
    let tree = Element::column()
        .gap(4.0)
        .child(
            Element::tree_header("Sources / 展开知识树", &expanded)
                .selected_when(&Signal::new(true)),
        )
        .child(
            Element::column()
                .padding(12.0)
                .visible(&expanded)
                .child(tree),
        );
    let (scroll, _) = Element::scroll_view(240.0, tree);
    let updates = progress.clone();
    let handler = std::rc::Rc::new(
        move |event: &chronobreak::UiEvent, context: &mut chronobreak::EventContext| {
            if let EventKind::Key(input) = &event.kind
                && input.state.is_pressed()
                && !input.repeat
            {
                if input.logical_key == Key::Named(NamedKey::F6) {
                    key_theme.set(match key_theme.mode() {
                        ThemeMode::Light => ThemeMode::Dark,
                        ThemeMode::Dark => ThemeMode::Light,
                    });
                    context.prevent_default();
                }
                if input.logical_key == Key::Named(NamedKey::F8)
                    && let Some(node) = key_semantics
                        .snapshot()
                        .nodes
                        .iter()
                        .find(|node| node.name == "clipboard-target")
                {
                    key_semantics.perform(node.id, SemanticAction::Focus);
                    context.prevent_default();
                }
                if input.logical_key == Key::Named(NamedKey::F7)
                    && let Some(node) = key_semantics
                        .snapshot()
                        .nodes
                        .iter()
                        .find(|node| node.name == "clipped-focus")
                {
                    key_semantics.perform(node.id, SemanticAction::Focus);
                    context.prevent_default();
                }
                if verify && input.logical_key == Key::Named(NamedKey::F9) {
                    eprintln!(
                        "probe theme={:?} progress={}",
                        key_theme.mode(),
                        key_progress.get()
                    );
                    for node in key_semantics.snapshot().nodes {
                        eprintln!(
                            "probe node name={:?} id={:?} role={:?} bounds={:?} value={:?} focused={} selected={:?} expanded={:?}",
                            node.name,
                            node.id,
                            node.role,
                            node.bounds,
                            node.value,
                            node.focused,
                            node.selected,
                            node.expanded
                        );
                    }
                    eprintln!("probe end");
                    context.prevent_default();
                }
            }
        },
    );
    let capture_handler = handler.clone();
    let root = Element::canvas(760.0, 520.0)
        .theme_background(ThemeColor::Background)
        .on_message(receiver, move |step| updates.update(|value| *value = step))
        .on_event(EventPhase::Capture, move |event, context| {
            capture_handler(event, context)
        })
        .on_event(EventPhase::Target, move |event, context| {
            handler(event, context)
        })
        .child(
            Element::label("Chronobreak / Themes & controls")
                .font_size(24.0)
                .positioned(16.0, 12.0)
                .width(720.0)
                .height(34.0),
        )
        .child(
            Element::canvas(120.0, 28.0)
                .positioned(624.0, 12.0)
                .theme_background(ThemeColor::Accent)
                .child(
                    Element::label("F6 • Theme")
                        .font_size(16.0)
                        .positioned(8.0, 2.0)
                        .width(104.0)
                        .theme_color(ThemeColor::OnAccent),
                ),
        )
        .child(
            Element::button("Light / Dark · 切换主题 (F6)")
                .positioned(16.0, 56.0)
                .width(344.0)
                .height(40.0)
                .accessible_name("theme-switch")
                .on_click(switch),
        )
        .child(
            Element::secondary_button("Secondary / 整理资料")
                .positioned(380.0, 56.0)
                .width(364.0)
                .height(40.0)
                .accessible_name("secondary"),
        )
        .child(
            Element::checkbox("Include sources / 包含源文", &enabled)
                .positioned(16.0, 108.0)
                .width(344.0)
                .height(36.0)
                .accessible_name("include-sources"),
        )
        .child(
            Element::text_input(&first)
                .positioned(16.0, 156.0)
                .width(344.0)
                .height(38.0)
                .accessible_name("source"),
        )
        .child(
            Element::text_input(&second)
                .positioned(380.0, 156.0)
                .width(364.0)
                .height(38.0)
                .readonly(true)
                .accessible_name("clipboard-target"),
        )
        .child(
            Element::text_input(&Signal::new("Invalid / 待校验".to_owned()))
                .positioned(16.0, 208.0)
                .width(344.0)
                .height(38.0)
                .invalid_when(&invalid)
                .accessible_name("invalid-source"),
        )
        .child(
            Element::label("! Error / 错误：请核对源文")
                .positioned(16.0, 258.0)
                .width(344.0)
                .height(28.0)
                .theme_color(ThemeColor::Error)
                .accessible_name("error"),
        )
        .child(
            Element::label("Ready / 已保留有效资料")
                .positioned(16.0, 300.0)
                .width(344.0)
                .height(28.0)
                .theme_color(ThemeColor::Success)
                .accessible_name("success"),
        )
        .child(
            Element::button("Disabled / 不可用")
                .positioned(16.0, 348.0)
                .width(344.0)
                .height(36.0)
                .disabled(true),
        )
        .child(
            Element::bound_text(&progress, |step| {
                format!("Worker / 后台进度: {step:03}/100")
            })
            .positioned(16.0, 400.0)
            .width(344.0)
            .height(28.0)
            .theme_color(ThemeColor::Muted)
            .accessible_name("progress"),
        )
        .child(scroll.positioned(380.0, 208.0).width(364.0))
        .child(
            Element::scroll_view(
                42.0,
                Element::secondary_button("Clipped focus / 裁剪焦点 (F7)")
                    .height(90.0)
                    .accessible_name("clipped-focus"),
            )
            .0
            .positioned(16.0, 456.0)
            .width(344.0),
        )
        .child(
            Element::canvas(364.0, 18.0)
                .positioned(380.0, 468.0)
                .child(Element::connector([0.0, 9.0], [360.0, 9.0], 2.0)),
        );
    let app = Application::new(root)
        .theme(&theme)
        .semantics(&semantics)
        .title("Chronobreak — Themes controls");
    for index in 0..10 {
        children.insert(
            format!("node-{index}"),
            Element::secondary_button(format!("Node {index:02} / 知识节点 · 可读标签"))
                .accessible_name(format!("node-{index}")),
        );
    }
    let result = app.run();
    worker.join().expect("finite worker");
    result
}
