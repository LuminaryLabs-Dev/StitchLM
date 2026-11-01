//! Whole-model Bonsai host. KV and recurrent state stay local; weight matrix
//! operations may use the same authenticated native/WASM shard command buffer.
use crate::{
    secret,
    shards::{self, MatrixSource, ProjectionRequest, ProjectionTarget, ShardService},
    Fleet,
};
use anyhow::{ensure, Result};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use stitch_core::*;
use stitch_model::bonsai::{self, Weights};
use tokio::sync::mpsc;

pub const TOKENIZER_SHA256: &str =
    "5f9e4d4901a92b997e463c1f46055088b6cca5ca61a6522d1b9f64c4bb81cb42";
pub struct BonsaiRuntime {
    pub identity: ModelIdentity,
    pub context: usize,
    weights: Mutex<bonsai::FileWeights>,
    tokenizer: tokenizers::Tokenizer,
}
impl BonsaiRuntime {
    pub fn load(path: &Path, tokenizer_path: &Path, context: usize) -> Result<Self> {
        ensure!(
            (1..=512).contains(&context),
            "Bonsai context must be 1..512"
        );
        let bytes = std::fs::read(tokenizer_path)?;
        ensure!(
            stitch_model::sha256(&bytes) == TOKENIZER_SHA256,
            "Bonsai tokenizer checksum mismatch"
        );
        let tokenizer = tokenizers::Tokenizer::from_bytes(bytes).map_err(anyhow::Error::msg)?;
        let weights = bonsai::FileWeights::open(path, 4_000_000)?;
        Ok(Self {
            identity: ModelIdentity {
                id: "Hikari07jp/Ternary-Bonsai-27B-Abliterated-LowDeg-GGUF".into(),
                revision: "60b1880c4c2e30711e1c90867a057f71ab45d587".into(),
                weights_sha256: bonsai::WEIGHTS_SHA256.into(),
            },
            context,
            weights: Mutex::new(weights),
            tokenizer,
        })
    }
    pub fn assignment(
        &self,
        r: &Registration,
        id: String,
        lease: String,
    ) -> Result<(Assignment, Vec<u8>)> {
        r.validate()?;
        ensure!(
            r.kernels.iter().any(|k| k == "prism-group128-matvec/1"),
            "Bonsai matrix kernel required"
        );
        Ok((
            Assignment {
                protocol: PROTOCOL,
                connector_id: id.clone(),
                lease_token: lease,
                model: self.identity.clone(),
                config: serde_json::json!({"backend":"bonsai-matrix-shards"}),
                range: BlockRange { start: 0, end: 0 },
                weights_path: format!("/api/connectors/{id}/weights"),
                weights_sha256: stitch_model::sha256(&[]),
                weights_bytes: 0,
                context_limit: self.context,
                memory_budget_mb: r.memory_budget_mb,
            },
            vec![],
        ))
    }
    pub fn generate(
        &self,
        request: GenerateRequest,
        fleet: Arc<Mutex<Fleet>>,
        events: mpsc::Sender<GenerationEvent>,
        service: Arc<Mutex<ShardService>>,
    ) -> Result<()> {
        let request_id = secret();
        let started = Instant::now();
        let mut generated = Vec::new();
        let mut remote_shards = 0;
        let participants = {
            let mut f = fleet
                .lock()
                .map_err(|_| anyhow::anyhow!("fleet lock poisoned"))?;
            ensure!(f.active.is_none(), "another generation is active");
            f.active = Some(request_id.clone());
            f.cancel = false;
            let mut items = f
                .connectors
                .iter()
                .filter(|(_, c)| {
                    !request.local_only
                        && c.ready
                        && c.seen.elapsed() < Duration::from_secs(30)
                        && c.kernels.iter().any(|k| k == "prism-group128-matvec/1")
                })
                .map(|(id, c)| (id.clone(), c.kind.clone(), c.assignment.memory_budget_mb))
                .collect::<Vec<_>>();
            items.sort_by(|a, b| a.0.cmp(&b.0));
            items.truncate(4);
            items
        };
        let outcome = (|| -> Result<()> {
            ensure!(
                !request.prompt.trim().is_empty()
                    && request.prompt.len() <= 8192
                    && (1..=128).contains(&request.max_tokens),
                "invalid generation request"
            );
            let count = request.connector_count.unwrap_or(participants.len());
            ensure!(
                count <= 4 && count <= participants.len(),
                "requested connector count unavailable"
            );
            ensure!(
                request.local_only || request.connector_count != Some(0),
                "use local_only for host-only inference"
            );
            let axis = request.split_axis.as_deref().unwrap_or("rows");
            ensure!(
                ["rows", "columns"].contains(&axis),
                "unsupported Bonsai split axis"
            );
            let preferred =
                request
                    .preferred_chunk
                    .unwrap_or(if axis == "rows" { 1024 } else { 1280 });
            ensure!(preferred > 0, "chunk size must be positive");
            let working = request.working_memory_bytes.unwrap_or(32_000_000);
            ensure!(
                (1_000_000..=128_000_000).contains(&working),
                "working budget must be 1..128 MB"
            );
            let prompt = if request.raw_prompt {
                request.prompt.clone()
            } else {
                format!("<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n",request.prompt)
            };
            let input = self
                .tokenizer
                .encode(prompt, false)
                .map_err(anyhow::Error::msg)?
                .get_ids()
                .to_vec();
            ensure!(
                !input.is_empty() && input.len() + request.max_tokens <= self.context,
                "prompt and output exceed context budget"
            );
            let mut provider = self
                .weights
                .lock()
                .map_err(|_| anyhow::anyhow!("weight provider poisoned"))?;
            let mut w = DistributedWeights {
                inner: &mut provider,
                identity: &self.identity,
                participants: participants[..count].to_vec(),
                fleet: fleet.clone(),
                service: service.clone(),
                axis: axis.into(),
                preferred,
                working,
                remote_shards: 0,
            };
            let mut state = bonsai::State::new(self.context)?;
            let mut ids = input.clone();
            for pos in 0..input.len() + request.max_tokens - 1 {
                w.check_cancel()?;
                let logits = bonsai::forward(&mut w, ids[pos], &mut state)?;
                remote_shards = w.remote_shards;
                w.check_cancel()?;
                if pos < input.len() - 1 {
                    events
                        .blocking_send(GenerationEvent {
                            request_id: request_id.clone(),
                            kind: format!("prefill {}/{}", pos + 1, input.len()),
                            text: String::new(),
                            token: None,
                            tokens: 0,
                            elapsed_ms: started.elapsed().as_secs_f64() * 1000.,
                            remote_blocks: 0,
                            remote_shards,
                        })
                        .map_err(|_| anyhow::anyhow!("client disconnected"))?;
                    continue;
                }
                let next = logits
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.total_cmp(b.1))
                    .ok_or_else(|| anyhow::anyhow!("empty logits"))?
                    .0 as u32;
                generated.push(next);
                ids.push(next);
                let text = self
                    .tokenizer
                    .decode(&generated, true)
                    .map_err(anyhow::Error::msg)?;
                events
                    .blocking_send(GenerationEvent {
                        request_id: request_id.clone(),
                        kind: "token".into(),
                        text,
                        token: Some(next),
                        tokens: generated.len(),
                        elapsed_ms: started.elapsed().as_secs_f64() * 1000.,
                        remote_blocks: 0,
                        remote_shards,
                    })
                    .map_err(|_| anyhow::anyhow!("client disconnected"))?;
                if next == 248046 {
                    break;
                }
            }
            Ok(())
        })();
        {
            let mut f = fleet.lock().unwrap();
            f.active = None;
            f.cancel = false;
        }
        {
            let mut s = service.lock().unwrap();
            s.active_request = None;
            let ids = s.pending.values().cloned().collect::<Vec<_>>();
            for id in ids {
                let _ = s.buffer.cancel(&id, "generation ended");
            }
            s.pending.clear();
            s.weight_slices.clear();
        }
        let text = match &outcome {
            Ok(_) => self
                .tokenizer
                .decode(&generated, true)
                .map_err(anyhow::Error::msg)?,
            Err(e) => e.to_string(),
        };
        let _ = events.blocking_send(GenerationEvent {
            request_id,
            kind: if outcome.is_ok() { "done" } else { "error" }.into(),
            text,
            token: None,
            tokens: generated.len(),
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.,
            remote_blocks: 0,
            remote_shards,
        });
        outcome
    }
}
struct DistributedWeights<'a> {
    inner: &'a mut bonsai::FileWeights,
    identity: &'a ModelIdentity,
    participants: Vec<(String, String, usize)>,
    fleet: Arc<Mutex<Fleet>>,
    service: Arc<Mutex<ShardService>>,
    axis: String,
    preferred: u64,
    working: u64,
    remote_shards: usize,
}
impl DistributedWeights<'_> {
    fn check_cancel(&self) -> Result<()> {
        ensure!(
            !self
                .fleet
                .lock()
                .map_err(|_| anyhow::anyhow!("fleet poisoned"))?
                .cancel,
            "generation cancelled"
        );
        Ok(())
    }
}
impl Weights for DistributedWeights<'_> {
    fn vector(&mut self, name: &str) -> Result<Vec<f32>> {
        self.check_cancel()?;
        self.inner.vector(name)
    }
    fn embedding(&mut self, token: u32) -> Result<Vec<f32>> {
        self.check_cancel()?;
        self.inner.embedding(token)
    }
    fn matvec(&mut self, name: &str, input: &[f32]) -> Result<Vec<f32>> {
        self.check_cancel()?;
        if self.participants.is_empty() {
            return self.inner.matvec(name, input);
        }
        let t = self.inner.tensor(name)?;
        ensure!(
            t.quant_type == 42 && t.dimensions.len() == 2,
            "unsupported distributed tensor"
        );
        let (columns, rows) = (t.dimensions[0], t.dimensions[1]);
        let bytes = (columns / 128 * 34 * rows) as usize;
        let data = self.inner.read_range(name, 0, bytes)?;
        self.check_cancel()?;
        let source = MatrixSource {
            model: self.identity.clone(),
            tensor_id: name.into(),
            rows,
            columns,
            packed: true,
            bytes: Arc::new(data),
        };
        let request = ProjectionRequest {
            target: ProjectionTarget::Registered {
                resource_id: name.into(),
            },
            input: input.to_vec(),
            connector_count: self.participants.len(),
            memory_limit_bytes: 1_000_000_000,
            max_working_bytes: Some(self.working),
            preferred_chunk: Some(self.preferred),
            split_axis: self.axis.clone(),
        };
        let fleet = self.fleet.clone();
        let report = shards::run_projection_checked(
            request,
            source,
            self.participants.clone(),
            self.service.clone(),
            move || {
                ensure!(
                    !fleet
                        .lock()
                        .map_err(|_| anyhow::anyhow!("fleet poisoned"))?
                        .cancel,
                    "generation cancelled"
                );
                Ok(())
            },
        )?;
        self.check_cancel()?;
        self.remote_shards += report.commands.len();
        Ok(report.output)
    }
}
