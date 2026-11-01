fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("checkpoint/report JSON path required"))?;
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let cp: stitch_core::shards::Checkpoint =
        serde_json::from_value(value.get("checkpoint").cloned().unwrap_or(value))?;
    let mut sealed = cp.clone();
    sealed.seal()?;
    println!("expected={} recomputed={}", cp.checksum, sealed.checksum);
    cp.verify()?;
    println!("checkpoint integrity and structure passed");
    Ok(())
}
