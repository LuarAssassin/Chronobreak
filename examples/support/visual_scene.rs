use chronobreak::{Border, Element, Shadow, Signal, srgb8, srgba8};
use std::{cell::Cell, rc::Rc};

fn rect(x: f32, y: f32, width: f32, height: f32, color: [f32; 4]) -> Element {
    Element::canvas(width, height)
        .positioned(x, y)
        .background(color)
}

pub fn reference() -> Element {
    let background = srgb8(247, 250, 252);
    let cyan = srgb8(185, 227, 240);
    let red = srgb8(205, 65, 69);
    let dark = srgb8(24, 43, 54);
    let mut root = Element::canvas(700.0, 480.0).background(background);
    for index in 0..24 {
        root = root.child(rect(10.0 + index as f32 * 12.0, 12.0, 10.0, 14.0, cyan));
    }
    root.child(rect(10.0, 50.0, 70.0, 50.0, cyan))
        .child(rect(100.0, 50.0, 70.0, 50.0, red))
        .child(rect(190.0, 50.0, 70.0, 50.0, dark))
        .child(rect(280.0, 50.0, 70.0, 50.0, srgba8(0, 115, 138, 128)))
        .child(
            Element::text("HH")
                .font_size(70.0)
                .color(cyan)
                .positioned(10.0, 105.0),
        )
        .child(
            Element::text("HH")
                .font_size(70.0)
                .color(red)
                .positioned(170.0, 105.0),
        )
        .child(
            rect(400.0, 40.0, 120.0, 80.0, cyan)
                .corner_radius(20.0)
                .border(Border::new(4.0, dark))
                .shadow(
                    Shadow::new(srgba8(0, 0, 0, 128), 8.0)
                        .offset(6.0, 8.0)
                        .spread(2.0),
                ),
        )
        .child(rect(10.0, 200.0, 100.0, 50.0, red).corner_radius(17.25))
        .child(
            rect(200.0, 200.0, 100.0, 50.0, cyan)
                .corner_radius(20.0)
                .child(rect(0.0, 0.0, 12.0, 12.0, red)),
        )
        .child(
            Element::canvas(150.0, 100.0)
                .positioned(400.0, 160.0)
                .child(
                    rect(120.0, 20.0, 20.0, 50.0, cyan)
                        .shadow(Shadow::new(srgba8(0, 0, 0, 128), 8.0).offset(20.0, 0.0)),
                ),
        )
        .child(rect(10.0, 280.0, 250.0, 100.0, srgb8(255, 255, 255)))
        .child(
            Element::text("HH")
                .font_size(80.0)
                .color(dark)
                .positioned(20.0, 270.0),
        )
        .child(rect(20.0, 295.0, 130.0, 55.0, srgba8(0, 115, 138, 128)))
        .child(
            Element::text("HH")
                .font_size(80.0)
                .color(red)
                .positioned(90.0, 270.0),
        )
        .child(Element::segment([320.0, 350.0], [440.0, 400.0], 4.0, dark))
        .child(Element::segment([470.0, 400.0], [470.0, 400.0], 6.0, red))
        .child(
            rect(560.0, 40.0, 80.0, 80.0, cyan)
                .corner_radius(900.0)
                .border(Border::new(900.0, dark)),
        )
        .child(
            rect(560.0, 160.0, 80.0, 60.0, [0.0; 4])
                .corner_radius(12.0)
                .border(Border::new(3.0, red)),
        )
        .child(
            Element::text("sRGB / linear alpha / finite SDF / insertion order")
                .font_size(16.0)
                .color(dark)
                .positioned(10.0, 440.0),
        )
}

pub fn contiguous() -> Element {
    let mut root = Element::canvas(760.0, 520.0).background(srgb8(247, 250, 252));
    for index in 0..32 {
        root = root.child(
            rect(
                10.0 + index as f32 * 20.0,
                100.0,
                16.0,
                40.0,
                srgb8(185, 227, 240),
            )
            .corner_radius(4.0)
            .border(Border::new(1.0, srgb8(24, 43, 54))),
        );
    }
    root
}

fn resource_content(sequence: usize, visible: &Signal<bool>, extra_text: bool) -> Element {
    let hidden = Signal::new(false);
    let mut content = Element::canvas(600.0, 400.0).visible(visible);
    for index in 0..16 {
        content = content.child(
            Element::text(format!("replacement {sequence:03} row {index:02}"))
                .positioned(10.0, 10.0 + index as f32 * 22.0)
                .color(srgb8(24, 43, 54)),
        );
    }
    for index in 0..if extra_text { 64 } else { 0 } {
        content = content
            .child(Element::text(format!("hidden {sequence} {index}")).visible(&hidden))
            .child(
                Element::text(format!("clipped {sequence} {index}"))
                    .positioned(0.0, 600.0 + index as f32 * 24.0),
            );
    }
    content
}

pub fn resources(extra_text: bool) -> Element {
    let (container, children) = Element::keyed_column();
    let step = Rc::new(Cell::new(0));
    let visible = Signal::new(true);
    Element::canvas(700.0, 480.0)
        .background(srgb8(247, 250, 252))
        .child(container)
        .on_click(move || {
            let current = step.get();
            let phase = current % 5;
            eprintln!("resource_step={current} phase={phase}");
            match phase {
                0 => {
                    visible.update(|value| *value = true);
                    children.insert("content", resource_content(current, &visible, extra_text));
                }
                1 => visible.update(|value| *value = false),
                2 => visible.update(|value| *value = true),
                3 => {
                    children.remove(children.node("content").unwrap());
                    children.insert("content", resource_content(current, &visible, extra_text));
                }
                4 => {
                    children.remove(children.node("content").unwrap());
                }
                _ => unreachable!(),
            }
            step.set(current + 1);
        })
}
