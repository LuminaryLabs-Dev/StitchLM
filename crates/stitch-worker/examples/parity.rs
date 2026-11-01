//! Real-weight, multi-step reference vs contiguous distributed block execution.
use anyhow::{ensure, Context, Result};
use candle::{Device, Tensor};
use stitch_core::*;
use stitch_worker::WorkerCore;
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let dir = args.get(1).context("model directory required")?;
    let out = args.get(2).context("evidence directory required")?;
    std::fs::create_dir_all(out)?;
    let weights = std::fs::read(format!("{dir}/model.safetensors"))?;
    let cfg = stitch_model::resolve_config(
        &std::fs::read_to_string(format!("{dir}/config.json"))?,
        &weights,
        256,
    )?;
    let model = stitch_model::load_model(&weights, &cfg)?;
    let mut reference_cache = stitch_model::new_cache(&cfg)?;
    let mut local_cache = stitch_model::new_cache(&cfg)?;
    let identity = ModelIdentity {
        id: "LiquidAI/LFM2.5-350M".into(),
        revision: "9e6c6ccf47cd318696e137d381a7ded8fe4df09f".into(),
        weights_sha256: stitch_model::sha256(&weights),
    };
    let mut workers = Vec::new();
    for (start, end) in [(0, 2), (2, 4)] {
        let range = BlockRange { start, end };
        let bytes = stitch_model::shard_bytes(&weights, &range, 16)?;
        let assignment = Assignment {
            protocol: 1,
            connector_id: format!("worker-{start}"),
            lease_token: "test-only".into(),
            model: identity.clone(),
            config: serde_json::to_value(&cfg)?,
            range,
            weights_path: "unused".into(),
            weights_sha256: stitch_model::sha256(&bytes),
            weights_bytes: bytes.len(),
            context_limit: 256,
            memory_budget_mb: 512,
        };
        std::fs::write(
            format!("{out}/assignment-{start}.json"),
            serde_json::to_vec_pretty(&assignment)?,
        )?;
        std::fs::write(format!("{out}/shard-{start}.safetensors"), &bytes)?;
        workers.push(WorkerCore::new(assignment, &bytes)?);
    }
    let tok = tokenizers::Tokenizer::from_file(format!("{dir}/tokenizer.json"))
        .map_err(anyhow::Error::msg)?;
    let mut ids=tok.encode("<|startoftext|><|im_start|>user\nCount from one to five.<|im_end|>\n<|im_start|>assistant\n",false).map_err(anyhow::Error::msg)?.get_ids().to_vec();
    let mut pos = 0;
    let mut tokens = Vec::new();
    let mut max_error = 0f32;
    for step in 0..8 {
        let input = Tensor::new(ids.as_slice(), &Device::Cpu)?.unsqueeze(0)?;
        let reference = model
            .forward(&input, pos, &mut reference_cache)?
            .flatten_all()?
            .to_vec1::<f32>()?;
        let mut hidden = model.embed(&input)?;
        for (n, worker) in workers.iter_mut().enumerate() {
            let job = InferenceJob {
                protocol: 1,
                id: format!("parity:{step}:{n}"),
                request_id: "parity".into(),
                model_sha256: identity.weights_sha256.clone(),
                range: worker.assignment().range.clone(),
                position: pos,
                input: stitch_model::frame(&hidden)?,
            };
            let result = worker.execute(&job)?;
            let replay = worker.execute(&job)?;
            ensure!(
                serde_json::to_vec(&result)? == serde_json::to_vec(&replay)?,
                "duplicate job advanced cache"
            );
            std::fs::write(
                format!("{out}/job-{step}-{n}.json"),
                serde_json::to_vec(&job)?,
            )?;
            std::fs::write(
                format!("{out}/result-{step}-{n}.json"),
                serde_json::to_vec(&result)?,
            )?;
            hidden = stitch_model::tensor(result.output.as_ref().unwrap(), cfg.hidden_size, 256)?;
        }
        hidden = model.blocks(&hidden, pos, 4, 16, &mut local_cache)?;
        let distributed = model.finish(&hidden)?.flatten_all()?.to_vec1::<f32>()?;
        let error = reference
            .iter()
            .zip(&distributed)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        max_error = max_error.max(error);
        ensure!(error < 1e-5, "logits differ by{error}");
        let token = distributed
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0 as u32;
        tokens.push(token);
        pos += ids.len();
        ids = vec![token];
        if Some(token) == cfg.eos_token_id {
            break;
        }
    }
    let mut invalid: InferenceJob =
        serde_json::from_slice(&std::fs::read(format!("{out}/job-0-0.json"))?)?;
    invalid.id = "new-job".into();
    invalid.request_id = "new-request".into();
    invalid.position = 1;
    ensure!(
        workers[0].execute(&invalid).is_err(),
        "accepted missing prefill"
    );
    let report = serde_json::json!({"synthetic":false,"model":identity,"remote_ranges":[[0,2],[2,4]],"host_range":[4,16],"tokens":tokens,"text":tok.decode(&tokens,true).map_err(anyhow::Error::msg)?,"maximum_logit_error":max_error,"duplicate_idempotency":true,"missing_prefill_rejected":true,"transport":"in-process shared worker core; network separately tested"});
    std::fs::write(
        format!("{out}/native-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
