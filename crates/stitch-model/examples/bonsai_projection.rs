//! Bounded actual-weight projection probe. Not full Qwen35 model inference.
use anyhow::{ensure, Context, Result};
use std::io::{Read, Seek, SeekFrom};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let path = args.get(1).context("GGUF path")?;
    let output = args.get(2).context("output JSON")?;
    let start: usize = args.get(3).context("row start")?.parse()?;
    let end: usize = args.get(4).context("row end")?.parse()?;
    let mut file = std::fs::File::open(path)?;
    let report = stitch_model::gguf::inspect(&mut file)?;
    ensure!(
        report.architecture.as_deref() == Some("qwen35"),
        "expected exact Bonsai architecture"
    );
    let tensor = report
        .tensors
        .iter()
        .find(|t| t.name == "blk.0.ffn_gate.weight")
        .context("projection missing")?;
    ensure!(
        tensor.quant_type == 42
            && tensor.dimensions == [5120, 17408]
            && tensor.storage_span_bytes == 23674880,
        "not the known legacy group128 tensor layout"
    );
    ensure!(start < end && end <= 17408, "row range invalid");
    let columns = 5120usize;
    let stride = columns / 128 * 34;
    let input: Vec<f32> = (0..columns)
        .map(|i| ((i % 31) as f32 - 15.) / 32.)
        .collect();
    let mut result = Vec::with_capacity(end - start);
    let mut buffer = vec![0; stride];
    file.seek(SeekFrom::Start(
        report.data_offset + tensor.offset + (start * stride) as u64,
    ))?;
    let now = std::time::Instant::now();
    for _ in start..end {
        file.read_exact(&mut buffer)?;
        result.push(stitch_model::ternary::matvec_rows(&buffer, &input)?[0]);
    }
    let data = serde_json::json!({"test":"actual GGUF projection rows, not full-model inference","tensor":tensor.name,"row_start":start,"row_end":end,"columns":columns,"codec":"legacy Prism group128; not modern upstream type42","seconds":now.elapsed().as_secs_f64(),"max_weight_buffer_bytes":stride,"output":result});
    std::fs::write(output, serde_json::to_vec(&data)?)?;
    println!(
        "rows {start}..{end} completed in{:.3}s",
        now.elapsed().as_secs_f64()
    );
    Ok(())
}
