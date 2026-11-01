fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("GGUF path required"))?;
    let report = stitch_model::gguf::inspect(&mut std::fs::File::open(path)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
