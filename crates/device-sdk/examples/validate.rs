fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).ok_or("usage: validate <device.sd>")?;
    let graph = suhina_device_sdk::DeviceGraph::read_file(std::path::Path::new(&path))?;
    match graph.role {
        suhina_device_sdk::DeviceRole::Effect => {
            let _prepared = graph.compile()?;
        }
        suhina_device_sdk::DeviceRole::MidiEffect => {
            let _prepared = graph.compile_midi()?;
        }
        suhina_device_sdk::DeviceRole::Instrument => {
            let _prepared = graph.compile_instrument_at(48_000.0)?;
        }
    }
    println!(
        "Valid {:?}: {} ({} nodes, {} parameters)",
        graph.role,
        graph.name,
        graph.nodes.len(),
        graph.parameters.len()
    );
    Ok(())
}
