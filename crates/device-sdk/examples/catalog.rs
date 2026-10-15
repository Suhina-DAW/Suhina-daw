fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--markdown") {
        print!("{}", suhina_device_sdk::node_reference_markdown());
    } else {
        println!("{}", serde_json::to_string_pretty(suhina_device_sdk::NODE_CATALOG)?);
    }
    Ok(())
}
