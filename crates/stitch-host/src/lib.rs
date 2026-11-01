//! Native coordinator. One active request owns every participating block cache.
use anyhow::{ensure, Context, Result};
use candle::{Device, Tensor};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use stitch_core::*;
use stitch_model::lfm2;
use tokio::sync::{mpsc, oneshot};

pub fn secret() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}
pub struct Connector {
    pub kernels: Vec<String>,
    pub capabilities: Vec<String>,
    pub assignment: Assignment,
    pub name: String,
    pub kind: String,
    pub ready: bool,
    pub seen: Instant,
    pub completed: u64,
    pub pending: Option<InferenceJob>,
    pub result: Option<oneshot::Sender<JobResult>>,
}
pub struct Fleet {
    pub connectors: HashMap<String, Connector>,
    pub active: Option<String>,
    pub cancel: bool,
}
pub struct Runtime {
    pub cfg: lfm2::Config,
    pub identity: ModelIdentity,
    pub weights: Arc<Vec<u8>>,
    pub model: lfm2::Model,
    pub tokenizer: tokenizers::Tokenizer,
}
impl Runtime {
    pub fn load(dir: &Path, context: usize) -> Result<Self> {
        for (name, size) in [
            ("model.safetensors", 708984464u64),
            ("config.json", 1281),
            ("tokenizer.json", 4733389),
        ] {
            ensure!(
                std::fs::metadata(dir.join(name))?.len() == size,
                "{name} size does not match the pinned model"
            );
        }
        let weights = std::fs::read(dir.join("model.safetensors"))?;
        let digest = stitch_model::sha256(&weights);
        // Trust the repository-pinned model identity, never an arbitrary local manifest digest.
        ensure!(
            digest == "1c9c77a4471a7f590f85240f74ed1fc26df7fbde88c3006724e2f93ca993ea4e",
            "model weights differ from pinned v0.0.1 model"
        );
        let identity = ModelIdentity {
            id: "LiquidAI/LFM2.5-350M".into(),
            revision: "9e6c6ccf47cd318696e137d381a7ded8fe4df09f".into(),
            weights_sha256: digest,
        };
        let config = std::fs::read_to_string(dir.join("config.json"))?;
        ensure!(
            stitch_model::sha256(config.as_bytes())
                == "720b43d6ddc2ed25be23eed355aefcf342434a176dedad23dbe0a5e3ac24bbb8",
            "config digest mismatch"
        );
        let tokenizer_bytes = std::fs::read(dir.join("tokenizer.json"))?;
        ensure!(
            stitch_model::sha256(&tokenizer_bytes)
                == "df1d8d5ec5d091b460562ffd545e4a5e91d17d4a0db7ebe733be34ed374377bd",
            "tokenizer digest mismatch"
        );
        let cfg = stitch_model::resolve_config(&config, &weights, context)?;
        let model = stitch_model::load_model(&weights, &cfg)?;
        let tokenizer =
            tokenizers::Tokenizer::from_bytes(tokenizer_bytes).map_err(anyhow::Error::msg)?;
        Ok(Self {
            cfg,
            identity,
            weights: Arc::new(weights),
            model,
            tokenizer,
        })
    }
    pub fn assignment(
        &self,
        registration: &Registration,
        id: String,
        lease: String,
        range: BlockRange,
    ) -> Result<(Assignment, Vec<u8>)> {
        registration.validate()?;
        let bytes = stitch_model::shard_bytes(&self.weights, &range, self.cfg.num_hidden_layers)?;
        ensure!(
            bytes.len() * 4 + 128 * 1024 * 1024 <= registration.memory_budget_mb * 1024 * 1024,
            "requested memory budget is too small"
        );
        Ok((
            Assignment {
                protocol: PROTOCOL,
                connector_id: id.clone(),
                lease_token: lease,
                model: self.identity.clone(),
                config: serde_json::to_value(&self.cfg)?,
                range,
                weights_path: format!("/api/connectors/{id}/weights"),
                weights_sha256: stitch_model::sha256(&bytes),
                weights_bytes: bytes.len(),
                context_limit: self.cfg.max_position_embeddings,
                memory_budget_mb: registration.memory_budget_mb,
            },
            bytes,
        ))
    }
    pub fn generate(
        &self,
        request: GenerateRequest,
        fleet: Arc<Mutex<Fleet>>,
        events: mpsc::Sender<GenerationEvent>,
    ) -> Result<()> {
        ensure!(
            !request.prompt.trim().is_empty() && request.prompt.len() <= 8192,
            "prompt must contain1..8192bytes"
        );
        ensure!(
            (1..=128).contains(&request.max_tokens),
            "token limit must be1..128"
        );
        let request_id = secret();
        let participants = {
            let mut f = fleet
                .lock()
                .map_err(|_| anyhow::anyhow!("fleet lock poisoned"))?;
            ensure!(f.active.is_none(), "another generation is active");
            f.active = Some(request_id.clone());
            f.cancel = false;
            let mut items: Vec<_> = f
                .connectors
                .iter()
                .filter(|(_, c)| {
                    !request.local_only && c.ready && c.seen.elapsed() < Duration::from_secs(30)
                })
                .map(|(id, c)| (id.clone(), c.assignment.range.clone()))
                .collect();
            items.sort_by(|(aid, a), (bid, b)| a.start.cmp(&b.start).then(aid.cmp(bid)));
            items.dedup_by_key(|(_, r)| r.start);
            items
        };
        let started = Instant::now();
        let mut generated = Vec::new();
        let mut remote_blocks = 0usize;
        let outcome = (|| -> Result<()> {
            let text = format!(
                "<|startoftext|><|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
                request.prompt
            );
            let mut ids = self
                .tokenizer
                .encode(text, false)
                .map_err(anyhow::Error::msg)?
                .get_ids()
                .to_vec();
            ensure!(
                ids.len() + request.max_tokens <= self.cfg.max_position_embeddings,
                "prompt plus output exceeds context budget"
            );
            let mut cache = stitch_model::new_cache(&self.cfg)?;
            let mut position = 0;
            for step in 0..request.max_tokens {
                ensure!(!fleet.lock().unwrap().cancel, "generation cancelled");
                let input = Tensor::new(ids.as_slice(), &Device::Cpu)?.unsqueeze(0)?;
                let mut hidden = self.model.embed(&input)?;
                let mut cursor = 0;
                for (connector_id, range) in &participants {
                    if cursor < range.start {
                        hidden = self.model.blocks(
                            &hidden,
                            position,
                            cursor,
                            range.start,
                            &mut cache,
                        )?;
                    }
                    let job = InferenceJob {
                        protocol: PROTOCOL,
                        id: format!("{request_id}:{step}:{}", range.start),
                        request_id: request_id.clone(),
                        model_sha256: self.identity.weights_sha256.clone(),
                        range: range.clone(),
                        position,
                        input: stitch_model::frame(&hidden)?,
                    };
                    let (tx, mut rx) = oneshot::channel();
                    {
                        let mut f = fleet.lock().unwrap();
                        let c = f
                            .connectors
                            .get_mut(connector_id)
                            .context("connector disappeared")?;
                        ensure!(c.ready, "connector is not ready");
                        c.pending = Some(job.clone());
                        c.result = Some(tx);
                    }
                    let deadline = Instant::now() + Duration::from_secs(60);
                    let result = loop {
                        ensure!(!fleet.lock().unwrap().cancel, "generation cancelled");
                        match rx.try_recv() {
                            Ok(v) => break v,
                            Err(oneshot::error::TryRecvError::Closed) => {
                                anyhow::bail!("connector disconnected; restart request")
                            }
                            Err(oneshot::error::TryRecvError::Empty) => {
                                ensure!(
                                    Instant::now() < deadline,
                                    "connector timed out; restart request"
                                );
                                std::thread::sleep(Duration::from_millis(10));
                            }
                        }
                    };
                    result.matches(&job)?;
                    let output = result.output.context("missing output")?;
                    ensure!(
                        output.shape == job.input.shape,
                        "connector output shape mismatch"
                    );
                    hidden = stitch_model::tensor(
                        &output,
                        self.cfg.hidden_size,
                        self.cfg.max_position_embeddings,
                    )?;
                    cursor = range.end;
                    remote_blocks += range.len();
                }
                if cursor < self.cfg.num_hidden_layers {
                    hidden = self.model.blocks(
                        &hidden,
                        position,
                        cursor,
                        self.cfg.num_hidden_layers,
                        &mut cache,
                    )?;
                }
                let logits = self.model.finish(&hidden)?.squeeze(0)?.to_vec1::<f32>()?;
                ensure!(
                    logits.iter().all(|v| v.is_finite()),
                    "non-finite model logits"
                );
                let token = logits
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.total_cmp(b.1))
                    .context("empty logits")?
                    .0 as u32;
                position += ids.len();
                generated.push(token);
                ids = vec![token];
                events
                    .blocking_send(GenerationEvent {
                        request_id: request_id.clone(),
                        kind: "token".into(),
                        text: self
                            .tokenizer
                            .decode(&generated, true)
                            .map_err(anyhow::Error::msg)?,
                        token: Some(token),
                        tokens: generated.len(),
                        elapsed_ms: started.elapsed().as_secs_f64() * 1000.,
                        remote_blocks,
                    })
                    .map_err(|_| anyhow::anyhow!("client disconnected"))?;
                if Some(token) == self.cfg.eos_token_id {
                    break;
                }
            }
            Ok(())
        })();
        {
            let mut f = fleet.lock().unwrap();
            f.active = None;
            f.cancel = false;
            for (id, _) in &participants {
                if let Some(c) = f.connectors.get_mut(id) {
                    c.pending = None;
                    c.result = None;
                    if outcome.is_err() {
                        c.ready = false;
                    }
                }
            }
        }
        let (kind, text) = match &outcome {
            Ok(_) => (
                "done",
                self.tokenizer
                    .decode(&generated, true)
                    .map_err(anyhow::Error::msg)?,
            ),
            Err(e) => ("error", e.to_string()),
        };
        let _ = events.blocking_send(GenerationEvent {
            request_id,
            kind: kind.into(),
            text,
            token: None,
            tokens: generated.len(),
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.,
            remote_blocks,
        });
        outcome
    }
}

pub mod shards;
