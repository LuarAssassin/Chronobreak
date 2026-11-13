use chronobreak::{Application, Border, Element, Result, Shadow, srgb8, srgba8};

#[path = "support/visual_scene.rs"]
mod visual_scene;

fn card(label: &str, x: f32, y: f32, color: [f32; 4]) -> Element {
    Element::column()
        .width(150.0)
        .height(52.0)
        .background(color)
        .corner_radius(12.0)
        .border(Border::new(2.0, srgb8(76, 98, 109)))
        .shadow(Shadow::new(srgba8(0, 0, 0, 90), 8.0).offset(0.0, 4.0))
        .positioned(x, y)
        .child(
            Element::text(label)
                .font_size(16.0)
                .width(126.0)
                .height(28.0)
                .color(srgb8(24, 43, 54))
                .positioned(12.0, 12.0),
        )
}

fn tree() -> Element {
    let ink = srgb8(76, 98, 109);
    let mut tree = Element::canvas(650.0, 180.0);
    for (from, to) in [
        ([170.0, 90.0], [235.0, 90.0]),
        ([235.0, 45.0], [235.0, 135.0]),
        ([235.0, 45.0], [300.0, 45.0]),
        ([235.0, 135.0], [300.0, 135.0]),
    ] {
        tree = tree.child(Element::segment(from, to, 2.0, ink));
    }
    tree.child(card("Knowledge", 20.0, 64.0, srgb8(185, 227, 240)))
        .child(card("Evidence", 300.0, 19.0, srgb8(255, 255, 255)))
        .child(card("Questions", 300.0, 109.0, srgb8(244, 211, 94)))
}

fn references() -> Element {
    Element::canvas(650.0, 150.0)
        .background(srgb8(255, 255, 255))
        .child(
            Element::column()
                .width(100.0)
                .height(80.0)
                .positioned(10.0, 10.0)
                .background(srgb8(185, 227, 240)),
        )
        .child(
            Element::text("HH")
                .font_size(70.0)
                .color(srgb8(185, 227, 240))
                .positioned(120.0, 0.0),
        )
        .child(
            Element::text("HH")
                .font_size(70.0)
                .color(srgb8(205, 65, 69))
                .positioned(280.0, 0.0),
        )
        .child(
            Element::column()
                .width(150.0)
                .height(56.0)
                .positioned(290.0, 25.0)
                .background(srgba8(0, 115, 138, 128)),
        )
        .child(
            Element::text("HH")
                .font_size(70.0)
                .color(srgb8(24, 43, 54))
                .positioned(390.0, 0.0),
        )
        .child(
            Element::text("sRGB swatch / glyph · ordered alpha overlap")
                .font_size(16.0)
                .width(630.0)
                .color(srgb8(24, 43, 54))
                .positioned(10.0, 110.0),
        )
}

fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("--verification") => {
            return Application::new(visual_scene::reference())
                .title("Chronobreak visual reference")
                .run();
        }
        Some("--contiguous") => {
            return Application::new(visual_scene::contiguous())
                .title("Chronobreak contiguous primitives")
                .run();
        }
        Some("--resources") | Some("--resources-visible") => {
            return Application::new(visual_scene::resources(
                std::env::args().nth(1).as_deref() == Some("--resources"),
            ))
            .title("Chronobreak visual resources")
            .run();
        }
        _ => {}
    }
    let (viewport, _) = Element::scroll_view(
        370.0,
        Element::column()
            .child(tree())
            .child(references())
            .child(tree()),
    );
    let root = Element::column()
        .padding(16.0)
        .gap(12.0)
        .background(srgb8(247, 250, 252))
        .child(
            Element::text("Finite visual primitives / application-placed tree")
                .color(srgb8(24, 43, 54)),
        )
        .child(viewport)
        .child(
            Element::text(
                "Connectors first; no graph engine, rounded child clips or theme system.",
            )
            .font_size(14.0)
            .color(srgb8(76, 98, 109)),
        );
    Application::new(root)
        .title("Chronobreak visual primitives")
        .run()
}
