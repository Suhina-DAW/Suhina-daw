//! Original artwork and custom controls, all packaged in one .sd.
mod common;
use common::{parameter, wire};
use image::ImageEncoder as _;
use suhina_device_sdk::{DeviceGraph, DeviceRole, FilterMode, Node as N, NodeKind as K, ui::*};

fn text(position: [f32; 2], label: &str, size: f32) -> Draw {
    Draw::Text {
        align: Default::default(),
        format: Default::default(),
        position: position.map(Into::into),
        text: label.into(),
        color: [204, 220, 233, 255],
        size,
        parameter: None,
        decimals: 0,
    }
}

pub fn build() -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    let mut g = DeviceGraph::new("org.example.custom-panel", "Orbit Filter", DeviceRole::Effect);
    g.nodes = vec![N::audio_input("in"), N::audio_output("out")];
    parameter(&mut g, "cutoff", (20.0, 20_000.0), 2400.0, "Hz", true);
    parameter(&mut g, "balance", (-1.0, 1.0), 0.0, "", false);
    parameter(&mut g, "level", (0.0, 2.0), 1.0, "", false);
    g.nodes.extend([
        N::new("filter", K::Filter { mode: FilterMode::Lowpass }),
        N::new("pan", K::Pan),
        N::gain("gain", "level"),
    ]);
    wire(&mut g, "in", "filter", "in");
    wire(&mut g, "cutoff", "filter", "frequency");
    wire(&mut g, "filter", "pan", "in");
    wire(&mut g, "balance", "pan", "balance");
    wire(&mut g, "pan", "gain", "in");
    wire(&mut g, "gain", "out", "in");
    for n in &mut g.nodes {
        n.visible = false;
    }

    // Self-authored transparent raster pattern; replace with ImageAsset::from_file for photos/icons.
    let mut png = Vec::new();
    let mut pixels = vec![0u8; 16 * 16 * 4];
    for y in 0..16 {
        for x in 0..16 {
            let i = (y * 16 + x) * 4;
            pixels[i..i + 4].copy_from_slice(&[129, 177, 210, if (x + y) % 16 == 0 { 22 } else { 0 }]);
        }
    }
    image::codecs::png::PngEncoder::new(&mut png).write_image(&pixels, 16, 16, image::ExtendedColorType::Rgba8)?;
    let mut canvas = DeviceUi {
        size: [600.0, 260.0],
        assets: vec![ImageAsset::from_bytes("grain", &png)?],
        hidden_parameters: vec!["cutoff".into(), "balance".into(), "level".into()],
        ..DeviceUi::default()
    };
    let accent = [103, 227, 189, 255];
    canvas.layers = vec![
        Draw::Gradient {
            rect: [0.0, 0.0, 600.0, 260.0].map(Into::into),
            colors: [[23, 38, 57, 255], [37, 29, 53, 255], [15, 24, 34, 255], [15, 24, 34, 255]],
        },
        Draw::Image {
            asset: "grain".into(),
            rect: [0.0, 0.0, 600.0, 260.0].map(Into::into),
            uv: [0.0, 0.0, 1.0, 1.0],
            tint: [255; 4],
            tile_size: Some([16.0, 16.0]),
            fit: ImageFit::Stretch,
        },
        text([24.0, 16.0], "ORBIT / FILTER", 20.0),
        text([24.0, 44.0], "Frequency x stereo position", 12.0),
        text([24.0, 232.0], "Drag the orbit. Shift for precision. Double click to reset.", 12.0),
        text([454.0, 184.0], "OUTPUT", 13.0),
    ];
    let px = UiValue::from(12.0) + UiValue::normalized("cutoff", true) * 366.0.into();
    let py = UiValue::from(136.0) + UiValue::normalized("balance", false) * (-124.0).into();
    canvas.controls.push(CustomControl {
        widget: None,
        visible_when: None,
        id: "orbit".into(),
        name: "Frequency and stereo position".into(),
        tooltip: "X: logarithmic cutoff. Y: stereo balance. Arrow keys adjust each axis.".into(),
        bounds: [24.0, 72.0, 390.0, 148.0],
        gesture: UiGesture::Position,
        x: Some(UiBinding { parameter: "cutoff".into(), logarithmic: true }),
        y: Some(UiBinding::new("balance")),
        drawing: vec![
            Draw::Rect { rect: [0.0, 0.0, 390.0, 148.0].map(Into::into), color: [9, 17, 27, 240], radius: 8.0 },
            Draw::Line {
                from: [0.0.into(), py.clone()],
                to: [390.0.into(), py.clone()],
                color: [54, 68, 82, 255],
                width: 1.0,
            },
            Draw::Line {
                from: [px.clone(), 0.0.into()],
                to: [px.clone(), 148.0.into()],
                color: [103, 227, 189, 60],
                width: 1.0,
            },
            Draw::Circle { center: [px, py], radius: 7.0.into(), color: accent },
        ],
    });
    let angle = UiValue::from(-2.35) + UiValue::normalized("level", false) * 4.7.into();
    canvas.controls.push(CustomControl {
        widget: None,
        visible_when: None,
        id: "output".into(),
        name: "Output level".into(),
        tooltip: "Drag vertically or press Up/Down. Double click restores unity.".into(),
        bounds: [448.0, 88.0, 96.0, 88.0],
        gesture: UiGesture::Drag,
        x: None,
        y: Some(UiBinding::new("level")),
        drawing: vec![
            Draw::Circle { center: [48.0.into(), 44.0.into()], radius: 33.0.into(), color: [12, 20, 31, 255] },
            Draw::Arc {
                center: [48.0.into(), 44.0.into()],
                radius: 36.0.into(),
                start: (-3.92).into(),
                end: (UiValue::from(-1.57) + angle.clone()),
                color: accent,
                width: 3.0,
            },
            Draw::Line {
                from: [48.0.into(), 44.0.into()],
                to: [
                    UiValue::from(48.0) + angle.clone().sin() * 27.0.into(),
                    UiValue::from(44.0) + angle.cos() * (-27.0).into(),
                ],
                color: accent,
                width: 3.0,
            },
            Draw::Text {
                align: Default::default(),
                format: Default::default(),
                position: [30.0.into(), 72.0.into()],
                text: "".into(),
                color: [204, 220, 233, 255],
                size: 12.0,
                parameter: Some("level".into()),
                decimals: 2,
            },
        ],
    });
    g.nodes.retain(|node| node.id != "level");
    g.ui = Some(canvas);
    let mut g = common::finish(g)?;
    for node in &mut g.nodes {
        node.visible = false;
    }
    Ok(g)
}
#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    common::save(build()?, "custom-panel")
}
