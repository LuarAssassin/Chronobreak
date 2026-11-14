#[path = "support/knowledge_document.rs"]
mod knowledge_document;

use chronobreak::{
    Application, Children, Element, EventKind, EventPhase, MessageSender, Result, Semantics,
    Signal, ThemeColor, ThemeController, ThemeMode, message_channel,
};
use knowledge_document::{Document, MAX_NODES, MAX_SOURCE_BYTES, Node, SourceRef, paragraphs};
use std::{cell::RefCell, collections::HashMap, num::NonZeroUsize, path::PathBuf, rc::Rc};

enum Reply {
    Loaded(std::io::Result<Document>),
    Pasted(std::result::Result<String, String>),
    Saved(std::io::Result<()>),
}

struct Workbench {
    document: Option<Document>,
    draft: Signal<String>,
    status: Signal<String>,
    busy: Signal<bool>,
    expanded: HashMap<String, Signal<bool>>,
    selected: HashMap<String, Signal<bool>>,
    map: Children,
    sender: MessageSender<Reply>,
    path: PathBuf,
}

type Shared = Rc<RefCell<Workbench>>;

fn offline(source: String) -> std::io::Result<Document> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(std::io::Error::other("source exceeds 64 KiB"));
    }
    let ranges = paragraphs(&source);
    if ranges.len() >= MAX_NODES {
        return Err(std::io::Error::other(
            "offline mode allows at most 127 lines",
        ));
    }
    let mut nodes = vec![Node {
        id: "source".into(),
        parent: None,
        title: "Offline source / 未调用 LLM".into(),
        expanded: true,
        source_refs: vec![],
    }];
    for (index, bytes) in ranges.iter().enumerate() {
        let text = source[bytes.start..bytes.end].trim();
        let title: String = text.chars().take(60).collect();
        nodes.push(Node {
            id: format!("paragraph-{index}"),
            parent: Some("source".into()),
            title: if title.is_empty() {
                "Blank line / 空行".into()
            } else {
                title
            },
            expanded: true,
            source_refs: vec![SourceRef {
                paragraph: index,
                bytes: bytes.clone(),
            }],
        });
    }
    let document = Document {
        schema_version: 1,
        source,
        paragraphs: ranges,
        root: "source".into(),
        nodes,
    };
    document.validate()?;
    Ok(document)
}

fn node_view(
    shared: &Shared,
    document: &Document,
    id: &str,
    depth: usize,
    order: &mut usize,
) -> Element {
    let node = document.nodes.iter().find(|node| node.id == id).unwrap();
    let (expanded, selected) = {
        let mut state = shared.borrow_mut();
        let expanded = Signal::new(node.expanded);
        let selected = Signal::new(false);
        state.expanded.insert(id.into(), expanded.clone());
        state.selected.insert(id.into(), selected.clone());
        (expanded, selected)
    };
    let click_state = shared.clone();
    let click_id = id.to_owned();
    let child_nodes: Vec<_> = document
        .nodes
        .iter()
        .filter(|child| child.parent.as_deref() == Some(id))
        .collect();
    let top = *order as f32 * 80.0;
    *order += 1;
    let left = depth as f32 * 8.0 + 12.0;
    let height = document.nodes.len() as f32 * 80.0;
    let header = Element::tree_header(&node.title, &expanded)
        .positioned(left, top)
        .selected_when(&selected)
        .accessible_name(format!("knowledge-{id}"))
        .width((400.0 - depth as f32 * 8.0).max(140.0))
        .height(64.0)
        .on_click(move || {
            let state = click_state.borrow();
            for (id, selected) in &state.selected {
                selected.update(|value| *value = id == &click_id);
            }
        });
    let mut children = Element::canvas(428.0, height)
        .positioned(0.0, 0.0)
        .visible(&expanded);
    for child in child_nodes {
        let child_top = *order as f32 * 80.0;
        children = children
            .child(Element::connector(
                [left, top + 32.0],
                [left - 8.0, top + 32.0],
                2.0,
            ))
            .child(Element::connector(
                [left - 8.0, top + 32.0],
                [left - 8.0, child_top + 32.0],
                2.0,
            ))
            .child(Element::connector(
                [left - 8.0, child_top + 32.0],
                [left + 8.0, child_top + 32.0],
                2.0,
            ))
            .child(node_view(shared, document, &child.id, depth + 1, order).positioned(0.0, 0.0));
    }
    Element::canvas(428.0, height).child(children).child(header)
}

fn install(shared: &Shared, document: Document) {
    let old = shared.borrow().map.node("document");
    if let Some(old) = old {
        shared.borrow().map.remove(old);
    }
    {
        let mut state = shared.borrow_mut();
        state.expanded.clear();
        state.selected.clear();
        state.document = Some(document.clone());
    }
    let view = node_view(shared, &document, &document.root, 0, &mut 0);
    shared.borrow().map.insert("document", view);
}

fn snapshot(state: &Workbench) -> Option<Document> {
    let mut document = state.document.clone()?;
    for node in &mut document.nodes {
        if let Some(expanded) = state.expanded.get(&node.id) {
            node.expanded = expanded.get();
        }
    }
    Some(document)
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("--path") {
        return Err("usage: knowledge_workbench --path TRUSTED_LOCAL_FILE".into());
    }
    let path = PathBuf::from(args.next().ok_or("missing --path value")?);
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    let theme = ThemeController::default();
    let semantics = Semantics::default();
    let (sender, receiver) = message_channel(NonZeroUsize::new(4).unwrap());
    let (map, children) = Element::keyed_column();
    let shared = Rc::new(RefCell::new(Workbench {
        document: None,
        draft: Signal::new(String::new()),
        status: Signal::new("Loading trusted local file… / 正在恢复".into()),
        busy: Signal::new(true),
        expanded: HashMap::new(),
        selected: HashMap::new(),
        map: children,
        sender: sender.clone(),
        path: path.clone(),
    }));
    let draft = shared.borrow().draft.clone();
    let status = shared.borrow().status.clone();
    let busy = shared.borrow().busy.clone();
    let paste = shared.clone();
    let fixture = shared.clone();
    let organize = shared.clone();
    let save = shared.clone();
    let receive = shared.clone();
    let switch = theme.clone();
    let probe_semantics = semantics.clone();
    let probe_state = shared.clone();
    let probe_theme = theme.clone();
    let verify = std::env::var_os("CHRONOBREAK_WORKBENCH_VERIFY").is_some();
    let (source_scroll, _) = Element::scroll_view(
        260.0,
        Element::bound_text(&draft, |text| {
            if text.is_empty() {
                "No pending source / 请主动粘贴".into()
            } else {
                text
            }
        })
        .width(276.0)
        .theme_color(ThemeColor::Text),
    );
    let (tree_scroll, _) = Element::scroll_view(260.0, map);
    let probe = Rc::new(
        move |event: &chronobreak::UiEvent, context: &mut chronobreak::EventContext| {
            if verify
                && let EventKind::Key(input) = &event.kind
                && input.state.is_pressed()
                && !input.repeat
                && input.logical_key
                    == chronobreak::keyboard::Key::Named(chronobreak::keyboard::NamedKey::F9)
            {
                let state = probe_state.borrow();
                let nodes: Vec<_> = probe_semantics.snapshot().nodes.iter().filter(|node| node.name.starts_with("knowledge-") || ["paste-source", "load-fixture", "offline-organize", "save-document", "theme-switch"].contains(&node.name.as_str())).map(|node| serde_json::json!({
                    "name": node.name, "id": format!("{:?}", node.id), "expanded": node.expanded,
                    "selected": node.selected, "focused": node.focused,
                    "bounds": [node.bounds.x, node.bounds.y, node.bounds.width, node.bounds.height],
                })).collect();
                eprintln!(
                    "workbench-probe {}",
                    serde_json::json!({
                        "theme": format!("{:?}", probe_theme.mode()), "busy": state.busy.get(),
                        "status": state.status.get(), "source_bytes": state.draft.get().len(), "nodes": nodes,
                    })
                );
                context.prevent_default();
            }
        },
    );
    let capture_probe = probe.clone();
    let root = Element::canvas(760.0, 520.0).theme_background(ThemeColor::Background)
        .on_event(EventPhase::Capture, move |event, context| capture_probe(event, context))
        .on_event(EventPhase::Target, move |event, context| probe(event, context))
        .on_message(receiver, move |reply| {
            receive.borrow().busy.update(|value| *value = false);
            match reply {
                Reply::Loaded(Ok(document)) => {
                    receive.borrow().draft.update(|value| *value = document.source.clone());
                    install(&receive, document);
                    receive.borrow().status.update(|value| *value = "Restored valid document / 已恢复；来源未联网核实".into());
                }
                Reply::Loaded(Err(error)) => receive.borrow().status.update(|value| *value =
                    if error.kind() == std::io::ErrorKind::NotFound { "Empty workbench / 本地文件尚不存在".into() }
                    else { format!("Restore failed ({:?}); file untouched / 恢复失败", error.kind()) }),
                Reply::Pasted(Ok(text)) => {
                    receive.borrow().draft.update(|value| *value = text);
                    receive.borrow().status.update(|value| *value = "Pending source only; previous tree retained / 待整理，旧树保留".into());
                }
                Reply::Pasted(Err(_)) => receive.borrow().status.update(|value| *value = "Clipboard unavailable or exceeds 64 KiB / 粘贴失败，旧文档保留".into()),
                Reply::Saved(Ok(())) => receive.borrow().status.update(|value| *value = "Saved snapshot / 已保存；关闭前等待此提示".into()),
                Reply::Saved(Err(error)) => receive.borrow().status.update(|value| *value = format!("Save failed ({:?}); memory retained / 保存失败", error.kind())),
            }
        })
        .child(Element::label("Knowledge workbench / 离线知识工作台").positioned(16.0, 8.0).font_size(24.0).width(720.0).height(38.0))
        .child(Element::label("Offline only · no network · tree titles are not verified facts / 不联网、不自动上传").positioned(16.0, 48.0).width(720.0).height(44.0))
        .child(Element::button("Paste / 粘贴").positioned(16.0, 96.0).width(170.0).height(38.0).disabled_when(&busy).accessible_name("paste-source").on_click(move || {
            let state = paste.borrow(); state.busy.update(|value| *value = true); state.status.update(|value| *value = "Reading clipboard… / 正在读取".into());
            let sender = state.sender.clone();
            std::thread::spawn(move || {
                let result = arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()).map_err(|_| "clipboard unavailable".to_owned()).and_then(|text| if text.len() <= MAX_SOURCE_BYTES { Ok(text) } else { Err("too large".into()) });
                let _ = sender.try_send(Reply::Pasted(result));
            });
        }))
        .child(Element::secondary_button("Synthetic fixture").positioned(194.0, 96.0).width(170.0).height(38.0).disabled_when(&busy).accessible_name("load-fixture").on_click(move || {
            let document = knowledge_document::fixture();
            fixture.borrow().draft.update(|value| *value = document.source.clone());
            install(&fixture, document);
            fixture.borrow().status.update(|value| *value = "Offline public synthetic fixture / 合成演示，未调用 LLM".into());
        }))
        .child(Element::button("Offline line tree").positioned(372.0, 96.0).width(180.0).height(38.0).disabled_when(&busy).accessible_name("offline-organize").on_click(move || {
            let source = organize.borrow().draft.get();
            match offline(source) {
                Ok(document) => { install(&organize, document); organize.borrow().status.update(|value| *value = "Offline line partition, not AI organization / 逐行离线树".into()); }
                Err(_) => organize.borrow().status.update(|value| *value = "Source rejected (64 KiB / 127 lines); previous tree retained".into()),
            }
        }))
        .child(Element::button("Save / 保存").positioned(560.0, 96.0).width(184.0).height(38.0).disabled_when(&busy).accessible_name("save-document").on_click(move || {
            let state = save.borrow();
            let Some(document) = snapshot(&state) else { state.status.update(|value| *value = "No valid tree to save / 尚无有效树".into()); return; };
            state.busy.update(|value| *value = true); state.status.update(|value| *value = "Saving snapshot… / 正在保存".into());
            let (sender, path) = (state.sender.clone(), state.path.clone());
            std::thread::spawn(move || { let _ = sender.try_send(Reply::Saved(document.save_atomic(&path))); });
        }))
        .child(Element::secondary_button("Light / Dark").positioned(560.0, 144.0).width(184.0).height(38.0).accessible_name("theme-switch").on_click(move || switch.set(match switch.mode() { ThemeMode::Light => ThemeMode::Dark, ThemeMode::Dark => ThemeMode::Light })))
        .child(Element::label("Pending source / 待整理原文").positioned(16.0, 152.0).width(280.0).height(28.0))
        .child(Element::label("Valid tree / 有效树").positioned(316.0, 152.0).width(230.0).height(28.0))
        .child(source_scroll.positioned(16.0, 192.0).width(280.0))
        .child(tree_scroll.positioned(316.0, 192.0).width(428.0))
        .child(Element::bound_text(&status, |text| text).positioned(16.0, 464.0).width(728.0).height(52.0).theme_color(ThemeColor::Text));
    let app = Application::new(root)
        .title("Chronobreak — Offline knowledge workbench")
        .theme(&theme)
        .semantics(&semantics);
    std::thread::spawn(move || {
        let _ = sender.try_send(Reply::Loaded(Document::load(&path)));
    });
    app.run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_partition_preserves_unicode_and_exact_source() {
        let source = "中文\r\n\n🌱 e\u{301}\0".to_owned();
        let document = offline(source.clone()).unwrap();
        assert_eq!(document.source, source);
        assert_eq!(document.nodes.len(), 4);
        assert_eq!(
            Document::parse(&document.serialize().unwrap()).unwrap(),
            document
        );
        assert!(
            document.nodes[1..]
                .iter()
                .all(|node| node.source_refs.len() == 1)
        );
    }

    #[test]
    fn offline_limits_and_empty_source_are_explicit() {
        assert!(offline("x\n".repeat(128)).is_err());
        assert!(offline("x".repeat(MAX_SOURCE_BYTES + 1)).is_err());
        assert_eq!(offline(String::new()).unwrap().nodes.len(), 1);
    }
}
