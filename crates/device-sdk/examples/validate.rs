fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).ok_or("usage: validate <effect.sd>")?;
    let graph = suhina_device_sdk::DeviceGraph::read_file(std::path::Path::new(&path))?;
    let _effect = graph.compile()?;
    println!("Valid effect: {} ({} nodes, {} parameters)", graph.name, graph.nodes.len(), graph.parameters.len());
    Ok(())
}
