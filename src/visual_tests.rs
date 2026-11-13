use super::*;
use crate::{Border, Shadow, srgb8};

fn rectangle() -> Element {
    Element::column()
        .width(40.0)
        .height(20.0)
        .positioned(20.0, 30.0)
        .background(srgb8(185, 227, 240))
        .on_click(|| {})
}

#[test]
fn decorations_do_not_change_layout_hit_or_child_clip() {
    let mut plain = Scene::new(Element::canvas(100.0, 100.0).child(rectangle()));
    let mut decorated = Scene::new(
        Element::canvas(100.0, 100.0).child(
            rectangle()
                .corner_radius(999.0)
                .border(Border::new(999.0, [1.0; 4]))
                .shadow(Shadow::new([0.0, 0.0, 0.0, 0.5], 8.0).offset(4.0, 5.0)),
        ),
    );
    plain.flush(100.0, 100.0);
    decorated.flush(100.0, 100.0);
    let geometry = |scene: &Scene| {
        scene
            .nodes
            .iter()
            .map(|node| (node.bounds, node.clip))
            .collect::<Vec<_>>()
    };
    assert_eq!(geometry(&plain), geometry(&decorated));
    let child = decorated.nodes[decorated.root.unwrap()].children[0];
    assert_eq!(decorated.hit_test(20.0, 30.0), Some(child));
    assert_ne!(decorated.hit_test(65.0, 40.0), Some(child));
    let Paint::Shape {
        clip, bounds, blur, ..
    } = decorated.display_list()[0]
    else {
        panic!("shadow first")
    };
    assert_eq!(clip, decorated.nodes[decorated.root.unwrap()].bounds);
    assert_ne!(clip, decorated.nodes[child].clip);
    assert_eq!(bounds.x, 24.0);
    assert_eq!(blur, 8.0);
}

#[test]
fn shadow_respects_scroll_ancestors_not_its_own_rectangle() {
    let (viewport, _) = Element::scroll_view(
        30.0,
        Element::canvas(100.0, 80.0)
            .child(rectangle().shadow(Shadow::new([0.0, 0.0, 0.0, 0.5], 8.0).offset(0.0, -10.0))),
    );
    let mut scene = Scene::new(viewport.width(100.0));
    scene.flush(100.0, 100.0);
    let list = scene.display_list();
    let Paint::Shape { clip, bounds, .. } = list[0] else {
        panic!("shadow can enter viewport from clipped owner")
    };
    assert_eq!(clip.height, 30.0);
    assert_eq!(bounds.y, 20.0);
    assert!(!list.iter().any(|paint| matches!(paint, Paint::Text(_))));
}

#[test]
fn rounded_background_does_not_round_child_clip() {
    let mut scene = Scene::new(
        Element::canvas(100.0, 100.0)
            .corner_radius(50.0)
            .background([1.0; 4])
            .child(
                Element::canvas(10.0, 10.0)
                    .background([0.0, 0.0, 0.0, 1.0])
                    .on_click(|| {}),
            ),
    );
    scene.flush(100.0, 100.0);
    let child = scene.nodes[scene.root.unwrap()].children[0];
    assert_eq!(scene.hit_test(0.0, 0.0), Some(child));
    assert_eq!(scene.nodes[child].clip.width, 10.0);
}

#[test]
fn segment_endpoints_are_parent_relative_and_zero_length_is_valid() {
    let mut scene = Scene::new(
        Element::canvas(100.0, 100.0)
            .child(Element::segment([80.0, 20.0], [10.0, 60.0], 4.0, [1.0; 4]))
            .child(Element::segment([50.0, 50.0], [50.0, 50.0], 4.0, [1.0; 4])),
    );
    scene.flush(100.0, 100.0);
    let Paint::Segment {
        origin, segment, ..
    } = scene.display_list()[0]
    else {
        panic!("segment")
    };
    assert_eq!(
        [origin[0] + segment.from[0], origin[1] + segment.from[1]],
        [80.0, 20.0]
    );
    assert_eq!(
        [origin[0] + segment.to[0], origin[1] + segment.to[1]],
        [10.0, 60.0]
    );
    let Paint::Segment { segment, .. } = scene.display_list()[1] else {
        panic!("dot")
    };
    assert_eq!(segment.from, segment.to);
}

#[test]
fn frame_versions_cover_content_constraints_and_native_preedit() {
    use winit::event::Ime;
    let value = Signal::new("start".to_owned());
    let mut scene = Scene::new(Element::text_input(&value));
    scene.flush(200.0, 80.0);
    let id = scene.root.unwrap();
    let first = scene.frame().texts[&id].version;
    value.update(|text| *text = "next".to_owned());
    scene.flush(200.0, 80.0);
    let content = scene.frame().texts[&id].version;
    assert!(content > first);
    scene.flush(180.0, 80.0);
    let constraint = scene.frame().texts[&id].version;
    assert!(constraint > content);
    scene.interaction.focused = Some(id);
    scene.ime_owner = scene.ime_target();
    scene.ime(Ime::Enabled);
    scene.ime(Ime::Preedit("你好".to_owned(), Some((6, 6))));
    scene.flush(180.0, 80.0);
    assert_eq!(value.get(), "next");
    assert!(scene.frame().texts[&id].version > constraint);
    let stable = scene.frame().texts[&id].version;
    scene.flush(180.0, 80.0);
    assert_eq!(scene.frame().texts[&id].version, stable);
}

#[test]
fn hidden_and_removed_frame_resources_never_alias_reused_nodes() {
    let shown = Signal::new(true);
    let (root, children) = Element::keyed_column();
    let mut scene = Scene::new(root.child(Element::text("old").visible(&shown)));
    scene.flush(200.0, 100.0);
    let old = *scene.frame().texts.keys().next().unwrap();
    shown.update(|value| *value = false);
    scene.flush(200.0, 100.0);
    assert!(scene.frame().texts.is_empty());
    children.remove(old);
    scene.flush(200.0, 100.0);
    children.insert("new", Element::text("new"));
    scene.flush(200.0, 100.0);
    let new = children.node("new").unwrap();
    assert_ne!(old, new);
    assert!(scene.nodes.get(old).is_none());
    assert_eq!(
        scene.frame().texts.keys().copied().collect::<Vec<_>>(),
        [new]
    );
}

#[test]
fn fully_clipped_text_input_content_has_no_frame_text_resource() {
    let value = Signal::new("clipped".to_owned());
    let mut scene = Scene::new(Element::text_input(&value));
    scene.flush(8.0, 50.0);
    assert!(scene.nodes[scene.root.unwrap()].clip.width > 0.0);
    assert_eq!(scene.text_geometry(scene.root.unwrap()).1.width, 0.0);
    assert!(scene.frame().texts.is_empty());
}
