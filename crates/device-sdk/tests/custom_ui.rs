use suhina_device_sdk::{DeviceGraph, ui::*};
#[path = "../examples/custom_panel.rs"]
mod panel;

#[test]
fn custom_panel_roundtrips_with_embedded_artwork() {
    let graph = panel::build().unwrap();
    let restored = DeviceGraph::from_sd(&graph.to_sd().unwrap()).unwrap();
    assert_eq!(restored, graph);
    let canvas = restored.ui.as_ref().unwrap();
    assert_eq!(canvas.assets[0].decode_rgba().unwrap().1, [16, 16]);
    let expression = UiValue::from(10.0) + UiValue::normalized("level", false) * 80.0.into();
    assert_eq!(expression.evaluate(&|_, normalized, _| if normalized { 0.5 } else { 1.0 }), 50.0);
    let mut effect = restored.compile().unwrap();
    effect.process(&mut [0.1, 0.2]);
}
#[test]
fn custom_panel_rejects_invalid_resources_and_bindings() {
    let graph = panel::build().unwrap();
    let mut bad = graph.clone();
    bad.ui.as_mut().unwrap().assets[0].data = "not-base64".into();
    assert!(bad.validate().is_err());
    let mut bad = graph.clone();
    bad.ui.as_mut().unwrap().controls[0].x.as_mut().unwrap().parameter = "missing".into();
    assert!(bad.validate().is_err());
    let mut bad = graph.clone();
    bad.ui.as_mut().unwrap().controls[0].bounds[2] = f32::INFINITY;
    assert!(bad.validate().is_err());
    let mut bad = graph.clone();
    bad.ui.as_mut().unwrap().controls[0].x.as_mut().unwrap().parameter = "balance".into();
    assert!(bad.validate().is_err());
    let mut bad = graph;
    let mut value = UiValue::from(1.0);
    for _ in 0..20 {
        value = value.sin();
    }
    bad.ui.as_mut().unwrap().layers.push(Draw::Circle {
        center: [value, 0.0.into()],
        radius: 1.0.into(),
        color: [255; 4],
    });
    assert!(bad.validate().is_err());
}
#[test]
fn jpeg_assets_decode_without_external_paths() {
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut bytes)
        .encode(&[10, 20, 30], 1, 1, image::ExtendedColorType::Rgb8)
        .unwrap();
    let asset = ImageAsset::from_bytes("photo", &bytes).unwrap();
    assert_eq!(asset.decode_rgba().unwrap().1, [1, 1]);
    assert!(ImageAsset::from_bytes("bad", b"not an image").is_err());
    assert!(ImageAsset::from_bytes("huge", &vec![0; MAX_ASSET_BYTES + 1]).is_err());
}

#[test]
fn canvas_and_viewport_sizes_roundtrip_and_validate() {
    let mut graph = panel::build().unwrap();
    let defaults: DeviceUi = serde_json::from_str("{}").unwrap();
    assert_eq!(defaults.size, [640.0, 240.0]);
    assert_eq!(defaults.viewport_size, None);
    let ui = graph.ui.as_mut().unwrap();
    ui.size = [800.0, 480.0];
    ui.viewport_size = Some([320.0, 180.0]);
    assert_eq!(DeviceGraph::from_sd(&graph.to_sd().unwrap()).unwrap(), graph);
    for size in [[0.0, 240.0], [320.0, 8193.0], [f32::NAN, 240.0], [320.0, f32::INFINITY]] {
        graph.ui.as_mut().unwrap().viewport_size = Some(size);
        assert!(graph.validate().is_err());
    }
}
