use anyhow::{ensure, Result};
use std::{path::PathBuf, time::Instant};
use stitch_model::bonsai::{self, Weights};
fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    ensure!(
        args.len() == 5,
        "usage: bonsai_generate MODEL TOKENIZER PROMPT STEPS OUTPUT"
    );
    let out = PathBuf::from(&args[4]);
    std::fs::create_dir_all(&out)?;
    let tokenizer = tokenizers::Tokenizer::from_file(&args[1]).map_err(anyhow::Error::msg)?;
    let input = tokenizer
        .encode(args[2].as_str(), false)
        .map_err(anyhow::Error::msg)?
        .get_ids()
        .to_vec();
    let steps: usize = args[3].parse()?;
    ensure!(
        !input.is_empty() && steps > 0 && steps <= 32 && input.len() + steps <= 128,
        "token budget exceeded"
    );
    let mut weights = bonsai::FileWeights::open(std::path::Path::new(&args[0]), 4_000_000)?;
    let mut state = bonsai::State::new(128)?;
    let mut ids = input.clone();
    let mut generated = Vec::new();
    let start = Instant::now();
    let mut report = Vec::new();
    for position in 0..input.len() + steps - 1 {
        let token = ids[position];
        let trace = out.join(format!("trace-{position}"));
        if std::env::var_os("STITCH_TRACE").is_some() {
            std::fs::create_dir_all(&trace)?;
            weights.trace_directory = Some(trace);
        }
        let before = Instant::now();
        let logits = bonsai::forward(&mut weights, token, &mut state)?;
        let next = logits
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0 as u32;
        let mut top = (0..logits.len()).collect::<Vec<_>>();
        top.sort_by(|a, b| logits[*b].total_cmp(&logits[*a]));
        std::fs::write(
            out.join(format!("logits-{position}.f32")),
            logits
                .iter()
                .flat_map(|x| x.to_le_bytes())
                .collect::<Vec<_>>(),
        )?;
        report.push(serde_json::json!({"position":position,"input_token":token,"next":next,"seconds":before.elapsed().as_secs_f64(),"top":top[..10].iter().map(|i|serde_json::json!([i,logits[*i]])).collect::<Vec<_>>()}));
        eprintln!(
            "position {position} -> {next}, {:.2}s",
            before.elapsed().as_secs_f64()
        );
        if position >= input.len() - 1 {
            generated.push(next);
            ids.push(next);
        }
        weights.observe("last_logits", &logits)?;
    }
    let text = tokenizer
        .decode(&generated, true)
        .map_err(anyhow::Error::msg)?;
    let result = serde_json::json!({"input":input,"generated":generated,"text":text,"seconds":start.elapsed().as_secs_f64(),"bytes_read":weights.bytes_read,"steps":report,"state_position":state.position});
    std::fs::write(out.join("report.json"), serde_json::to_vec_pretty(&result)?)?;
    println!("{}", result);
    Ok(())
}
