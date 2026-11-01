use anyhow::{Context, Result};
use candle::{Device, Tensor};
fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let dir = args.get(1).context("model directory required")?;
    let prompt = args
        .get(2)
        .map(String::as_str)
        .unwrap_or("Reply with the word hello.");
    let count: usize = args.get(3).map(|s| s.parse()).transpose()?.unwrap_or(12);
    let weights = std::fs::read(format!("{dir}/model.safetensors"))?;
    let cfg = stitch_model::resolve_config(
        &std::fs::read_to_string(format!("{dir}/config.json"))?,
        &weights,
        256,
    )?;
    let model = stitch_model::load_model(&weights, &cfg)?;
    let mut cache = stitch_model::new_cache(&cfg)?;
    let tok = tokenizers::Tokenizer::from_file(format!("{dir}/tokenizer.json"))
        .map_err(anyhow::Error::msg)?;
    let formatted =
        format!("<|startoftext|><|im_start|>user\n{prompt}<|im_end|>\n<|im_start|>assistant\n");
    let mut ids = tok
        .encode(formatted, false)
        .map_err(anyhow::Error::msg)?
        .get_ids()
        .to_vec();
    let mut generated = Vec::new();
    let mut pos = 0;
    let start = std::time::Instant::now();
    for _ in 0..count {
        let input = Tensor::new(ids.as_slice(), &Device::Cpu)?.unsqueeze(0)?;
        let logits = model
            .forward(&input, pos, &mut cache)?
            .squeeze(0)?
            .to_vec1::<f32>()?;
        let next = logits
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0 as u32;
        pos += ids.len();
        generated.push(next);
        if Some(next) == cfg.eos_token_id {
            break;
        }
        ids = vec![next];
    }
    println!(
        "{}",
        serde_json::json!({"model":"LiquidAI/LFM2.5-350M","tokens":generated,"text":tok.decode(&generated,true).map_err(anyhow::Error::msg)?,"seconds":start.elapsed().as_secs_f64(),"resolved_intermediate_size":cfg.intermediate_size,"backend":"Candle Rust CPU f32","synthetic":false})
    );
    Ok(())
}
