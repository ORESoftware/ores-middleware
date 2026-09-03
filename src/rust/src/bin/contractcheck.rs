fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", serde_json::to_string(&ores_middleware::descriptor())?);
    Ok(())
}
