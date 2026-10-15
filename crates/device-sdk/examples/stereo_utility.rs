//! Build the stereo-utility effect as a self-contained .sd file.
mod common;
use common::{finish, wire};
use suhina_device_sdk::Connection as C;
use suhina_device_sdk::{DeviceGraph, DeviceRole, Node as N, NodeKind as K, Parameter};

pub fn build() -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    let mut g = DeviceGraph::new("org.example.stereo-utility", "stereo-utility", DeviceRole::Effect);
    g.nodes = vec![N::audio_input("in"), N::audio_output("out")];

    g.parameters.push(Parameter::new("width", "Width", (0.0, 2.0), 1.0));
    g.nodes.extend([
        N::new("encode", K::MidSideEncode),
        N::gain("width", "width"),
        N::new("stereo", K::Stereo),
        N::new("decode", K::MidSideDecode),
    ]);
    wire(&mut g, "in", "encode", "in");
    wire(&mut g, "encode", "width", "in");
    g.connections.push(C::ports("encode", "left", "stereo", "left"));
    g.connections.push(C::ports("width", "right", "stereo", "right"));
    wire(&mut g, "stereo", "decode", "in");
    wire(&mut g, "decode", "out", "in");

    finish(g)
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    common::save(build()?, "stereo-utility")
}
