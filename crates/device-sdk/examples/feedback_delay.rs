//! Build the feedback-delay effect as a self-contained .sd file.
mod common;
use common::{finish, parameter, wire};
use suhina_device_sdk::{DeviceGraph, DeviceRole, Node as N, NodeKind as K, Parameter};

pub fn build() -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    let mut g = DeviceGraph::new("org.example.feedback-delay", "feedback-delay", DeviceRole::Effect);
    g.nodes = vec![N::audio_input("in"), N::audio_output("out")];

    g.parameters.push(Parameter::new("feedback", "Feedback", (0.0, 0.95), 0.5));
    parameter(&mut g, "time", (0.001, 1.0), 0.25, "seconds", true);
    parameter(&mut g, "wet", (0.0, 1.0), 0.5, "", false);
    g.nodes.extend([
        N::new("sum", K::Add),
        N::new("delay", K::Delay { max_seconds: 1.0 }),
        N::gain("feedback", "feedback"),
        N::new("mix", K::Mix),
    ]);
    wire(&mut g, "in", "sum", "a");
    wire(&mut g, "feedback", "sum", "b");
    wire(&mut g, "delay", "feedback", "in");
    wire(&mut g, "sum", "delay", "in");
    wire(&mut g, "time", "delay", "time");
    wire(&mut g, "in", "mix", "a");
    wire(&mut g, "delay", "mix", "b");
    wire(&mut g, "wet", "mix", "amount");
    wire(&mut g, "mix", "out", "in");

    finish(g)
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    common::save(build()?, "feedback-delay")
}
