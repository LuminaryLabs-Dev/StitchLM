//! One reusable worker engine. Native and browser apps wrap this exact implementation.
use anyhow::{ensure, Result};
use stitch_core::{Assignment, InferenceJob, JobResult, PROTOCOL};
use stitch_model::{lfm2, sha256};

/// Stateful LFM blocks or stateless Bonsai matrix commands, both using the same
/// validated shard kernel. Stateless assignments never claim to own model state.
pub enum WorkerCore {
    Lfm(Box<LfmWorkerCore>),
    Stateless(Box<Assignment>),
}
impl WorkerCore {
    pub fn new(assignment: Assignment, weights: &[u8]) -> Result<Self> {
        if assignment.config.get("backend").and_then(|v| v.as_str()) == Some("bonsai-matrix-shards")
        {
            assignment.model.validate()?;
            ensure!(
                assignment.protocol == PROTOCOL
                    && assignment.model.weights_sha256 == stitch_model::bonsai::WEIGHTS_SHA256
                    && weights.is_empty()
                    && assignment.weights_bytes == 0
                    && assignment.weights_sha256 == sha256(&[])
                    && assignment.range.is_empty()
                    && (256..=8192).contains(&assignment.memory_budget_mb)
                    && (1..=512).contains(&assignment.context_limit),
                "invalid stateless Bonsai assignment"
            );
            return Ok(Self::Stateless(Box::new(assignment)));
        }
        Ok(Self::Lfm(Box::new(LfmWorkerCore::new(
            assignment, weights,
        )?)))
    }
    pub fn assignment(&self) -> &Assignment {
        match self {
            Self::Lfm(w) => w.assignment(),
            Self::Stateless(a) => a,
        }
    }
    pub fn reset(&mut self) {
        if let Self::Lfm(w) = self {
            w.reset();
        }
    }
    pub fn execute(&mut self, job: &InferenceJob) -> Result<JobResult> {
        match self {
            Self::Lfm(w) => w.execute(job),
            Self::Stateless(_) => anyhow::bail!(
                "stateless Bonsai connector accepts matrix commands, not stateful LFM jobs"
            ),
        }
    }
}
pub struct LfmWorkerCore {
    assignment: Assignment,
    cfg: lfm2::Config,
    shard: lfm2::BlockShard,
    cache: lfm2::Cache,
    request: Option<String>,
    next_position: usize,
    last: Option<(String, String, JobResult)>,
}
impl LfmWorkerCore {
    pub fn new(assignment: Assignment, weights: &[u8]) -> Result<Self> {
        ensure!(
            assignment.protocol == PROTOCOL,
            "unsupported assignment protocol"
        );
        assignment.model.validate()?;
        ensure!(
            assignment.weights_bytes == weights.len()
                && sha256(weights) == assignment.weights_sha256,
            "shard integrity failed"
        );
        ensure!(
            (256..=8192).contains(&assignment.memory_budget_mb),
            "invalid memory budget"
        );
        let conservative = weights
            .len()
            .checked_mul(4)
            .and_then(|n| n.checked_add(128 * 1024 * 1024))
            .ok_or_else(|| anyhow::anyhow!("memory estimate overflow"))?;
        ensure!(
            conservative <= assignment.memory_budget_mb * 1024 * 1024,
            "shard exceeds conservative memory admission budget"
        );
        let cfg: lfm2::Config = serde_json::from_value(assignment.config.clone())?;
        stitch_model::validate_config(&cfg)?;
        ensure!(
            cfg.max_position_embeddings == assignment.context_limit,
            "context mismatch"
        );
        assignment.range.validate(cfg.num_hidden_layers)?;
        let shard = stitch_model::load_shard(weights, &cfg, &assignment.range)?;
        let cache = stitch_model::new_cache(&cfg)?;
        Ok(Self {
            assignment,
            cfg,
            shard,
            cache,
            request: None,
            next_position: 0,
            last: None,
        })
    }
    pub fn assignment(&self) -> &Assignment {
        &self.assignment
    }
    pub fn reset(&mut self) {
        self.cache.clear();
        self.request = None;
        self.next_position = 0;
        self.last = None;
    }
    pub fn execute(&mut self, job: &InferenceJob) -> Result<JobResult> {
        job.validate(
            self.cfg.hidden_size,
            self.cfg.num_hidden_layers,
            self.assignment.context_limit,
        )?;
        ensure!(
            job.model_sha256 == self.assignment.model.weights_sha256
                && job.range == self.assignment.range,
            "job model/range does not match assignment"
        );
        let digest = sha256(&serde_json::to_vec(job)?);
        if let Some((id, previous, result)) = &self.last {
            if id == &job.id {
                ensure!(&digest == previous, "job id reused for different input");
                return Ok(result.clone());
            }
        }
        if self.request.as_deref() != Some(&job.request_id) {
            ensure!(
                job.position == 0,
                "new request requires a full prefill from position zero"
            );
            self.reset();
            self.request = Some(job.request_id.clone());
        }
        ensure!(
            job.position == self.next_position,
            "out-of-order token position; replay the request after recovery"
        );
        let result = (|| {
            let input = stitch_model::tensor(
                &job.input,
                self.cfg.hidden_size,
                self.assignment.context_limit,
            )?;
            let output = self.shard.forward(&input, job.position, &mut self.cache)?;
            let frame = stitch_model::frame(&output)?;
            frame.validate(self.cfg.hidden_size, self.assignment.context_limit)?;
            Ok::<_, anyhow::Error>(frame)
        })();
        let output = match result {
            Ok(v) => v,
            Err(e) => {
                self.reset();
                return Err(e);
            }
        };
        self.next_position += job.input.shape[1];
        let result = JobResult {
            protocol: PROTOCOL,
            job_id: job.id.clone(),
            request_id: job.request_id.clone(),
            model_sha256: job.model_sha256.clone(),
            range: job.range.clone(),
            position: job.position,
            output: Some(output),
            error: None,
            elapsed_ms: None,
        };
        self.last = Some((job.id.clone(), digest, result.clone()));
        Ok(result)
    }
}

/// Stateless work uses the same immutable descriptor in native and WASM wrappers.
/// Weight transfer is separate from the command, and checked before kernel entry.
pub fn execute_shard(
    command: &stitch_core::shards::Command,
    definition: &stitch_core::shards::ShardDefinition,
    weights: &[u8],
    input: &[f32],
) -> Result<stitch_core::shards::ResultPacket> {
    use stitch_core::shards::*;
    definition.validate()?;
    ensure!(
        definition.tensor.shape.len() == 2 && definition.computation.output_shape.len() == 1,
        "matrix kernel requires rank-two weights and a vector output"
    );
    ensure!(
        definition.state.is_none()
            && definition.storage.compression == "none"
            && definition.storage.serialization == "raw-tensor-bytes",
        "unsupported state/compression/serialization contract"
    );
    ensure!(
        definition.computation.input_precision == DType::F32
            && definition.computation.accumulation_precision == DType::F32
            && definition.computation.output_precision == DType::F32,
        "kernel does not implement the requested precision"
    );
    ensure!(
        definition.computation.parameters.keys().all(|k| [
            "columns",
            "source_columns",
            "source_shape"
        ]
        .contains(&k.as_str())),
        "unsupported numerical parameter"
    );
    ensure!(
        definition.budget.packed_weights >= weights.len() as u64
            && definition.budget.inputs >= input.len() as u64 * 4
            && definition.budget.outputs >= definition.computation.output_bytes,
        "declared buffers omit actual kernel allocations"
    );
    ensure!(
        command.shard_id == definition.id
            && command.definition_hash == definition.hash()?
            && command.request_id == definition.request_id
            && command.plan_version == definition.plan_version
            && command.assignment_generation > 0,
        "shard command identity mismatch"
    );
    ensure!(
        definition
            .ownership
            .allowed_devices
            .contains(&command.device_id)
            && definition
                .storage
                .approved_destinations
                .contains(&command.device_id),
        "shard device is not authorized"
    );
    ensure!(
        definition.budget.fits()
            && weights.len() as u64 == definition.tensor.byte_length
            && digest(weights) == definition.tensor.checksum,
        "shard memory/integrity contract failed"
    );
    ensure!(
        definition.tensor.transforms.is_empty() && definition.computation.kernel_version == 1,
        "unsupported transform/kernel version"
    );
    let columns = definition
        .computation
        .parameters
        .get("columns")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("columns parameter required"))? as usize;
    ensure!(
        input.len() == columns
            && input.iter().all(|v| v.is_finite())
            && columns > 0
            && columns <= 65536,
        "invalid shard input"
    );
    let input_checksum = digest(&float_bytes(input));
    ensure!(
        definition
            .inputs
            .tensors
            .first()
            .and_then(|r| r.checksum.as_ref())
            == Some(&input_checksum),
        "input reference integrity mismatch"
    );
    let implemented_kernel = match definition.computation.operation {
        Operation::PackedGroup128MatVec => "prism-group128-matvec/1",
        Operation::DenseMatVec => "dense-f32-matvec/1",
        _ => "unsupported",
    };
    ensure!(
        definition
            .computation
            .kernels
            .iter()
            .any(|k| k == implemented_kernel),
        "declared kernel is not implemented by this worker"
    );
    let output=match definition.computation.operation{
  Operation::PackedGroup128MatVec=>{let q=definition.tensor.quantization.as_ref().ok_or_else(||anyhow::anyhow!("packed codec required"))?;ensure!(q.codec=="prism-legacy-group128"&&q.version==1&&q.group_size==128&&q.packed_group_bytes==34,"unsupported packed codec");stitch_model::ternary::matvec_rows(weights,input)?},
  Operation::DenseMatVec=>{ensure!(definition.tensor.dtype==DType::F32&&weights.len().is_multiple_of(columns*4),"invalid dense matrix");let mut output=Vec::new();for row in weights.chunks_exact(columns*4){let mut sum=0f32;for(bytes,x)in row.as_chunks::<4>().0.iter().zip(input){let w=f32::from_le_bytes(*bytes);ensure!(w.is_finite(),"non-finite weight");sum+=w*x;}ensure!(sum.is_finite(),"matrix result overflow");output.push(sum);}output},
  _=>anyhow::bail!("operation has no supported stateless worker kernel; use its dedicated state/attention contract"),
 };
    ensure!(
        output.len() as u64 * 4 == definition.computation.output_bytes,
        "kernel output contract mismatch"
    );
    Ok(ResultPacket {
        command_id: command.id.clone(),
        shard_id: definition.id.clone(),
        definition_hash: command.definition_hash.clone(),
        request_id: command.request_id.clone(),
        plan_version: command.plan_version,
        device_id: command.device_id.clone(),
        assignment_generation: command.assignment_generation,
        checksum: digest(&float_bytes(&output)),
        output,
        execution_ms: None,
        transferred_bytes: weights.len() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stateless_bonsai_assignment_has_no_weight_or_cache_payload() {
        let assignment = Assignment {
            protocol: PROTOCOL,
            connector_id: "test".into(),
            lease_token: "test".into(),
            model: stitch_core::ModelIdentity {
                id: "bonsai".into(),
                revision: "a".repeat(40),
                weights_sha256: stitch_model::bonsai::WEIGHTS_SHA256.into(),
            },
            config: serde_json::json!({"backend":"bonsai-matrix-shards"}),
            range: stitch_core::BlockRange { start: 0, end: 0 },
            weights_path: "/api/connectors/test/weights".into(),
            weights_sha256: sha256(&[]),
            weights_bytes: 0,
            context_limit: 64,
            memory_budget_mb: 512,
        };
        let mut worker = WorkerCore::new(assignment.clone(), &[]).unwrap();
        worker.reset();
        assert!(worker.assignment().range.is_empty());
        assert!(WorkerCore::new(assignment.clone(), &[1]).is_err());
        let mut invalid = assignment;
        invalid.context_limit = 513;
        assert!(WorkerCore::new(invalid, &[]).is_err());
    }
    #[test]
    fn corrupted_shard_rejected_before_allocation() {
        let a = Assignment {
            protocol: 1,
            connector_id: "test".into(),
            lease_token: "test".into(),
            model: stitch_core::ModelIdentity {
                id: "model".into(),
                revision: "a".repeat(40),
                weights_sha256: "b".repeat(64),
            },
            config: serde_json::json!({}),
            range: stitch_core::BlockRange { start: 0, end: 1 },
            weights_path: "/shard".into(),
            weights_sha256: "c".repeat(64),
            weights_bytes: 1,
            context_limit: 16,
            memory_budget_mb: 512,
        };
        assert!(WorkerCore::new(a, &[1]).is_err());
    }
}
