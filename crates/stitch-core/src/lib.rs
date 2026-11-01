//! Shared protocol for one model execution, used by native and WASM connectors.
use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};

pub const PROTOCOL: u32 = 1;
pub const MAX_CONTEXT: usize = 512;
pub const MAX_FRAME_VALUES: usize = 512 * 4096;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockRange {
    pub start: usize,
    pub end: usize,
}
impl BlockRange {
    pub fn validate(&self, layers: usize) -> Result<()> {
        ensure!(
            self.start < self.end && self.end <= layers,
            "invalid model block range"
        );
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelIdentity {
    pub id: String,
    pub revision: String,
    pub weights_sha256: String,
}
impl ModelIdentity {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.id.is_empty() && self.id.len() < 200,
            "invalid model id"
        );
        ensure!(
            self.revision.len() == 40 && self.revision.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid model revision"
        );
        ensure!(
            self.weights_sha256.len() == 64
                && self.weights_sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid weight digest"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TensorFrame {
    pub shape: [usize; 3],
    pub values: Vec<f32>,
}
impl TensorFrame {
    pub fn validate(&self, hidden: usize, context: usize) -> Result<()> {
        ensure!(
            self.shape[0] == 1
                && self.shape[1] > 0
                && self.shape[1] <= context
                && self.shape[2] == hidden,
            "invalid activation shape"
        );
        let size = self
            .shape
            .iter()
            .try_fold(1usize, |a, b| a.checked_mul(*b))
            .ok_or_else(|| anyhow::anyhow!("activation overflow"))?;
        ensure!(
            size <= MAX_FRAME_VALUES && self.values.len() == size,
            "activation length exceeds contract"
        );
        ensure!(
            self.values.iter().all(|x| x.is_finite()),
            "non-finite activation"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceJob {
    pub protocol: u32,
    pub id: String,
    pub request_id: String,
    pub model_sha256: String,
    pub range: BlockRange,
    pub position: usize,
    pub input: TensorFrame,
}
impl InferenceJob {
    pub fn validate(&self, hidden: usize, layers: usize, context: usize) -> Result<()> {
        ensure!(self.protocol == PROTOCOL, "unsupported protocol");
        ensure!(
            !self.id.is_empty()
                && self.id.len() <= 128
                && !self.request_id.is_empty()
                && self.request_id.len() <= 128,
            "invalid job identity"
        );
        self.range.validate(layers)?;
        self.input.validate(hidden, context)?;
        ensure!(
            self.position
                .checked_add(self.input.shape[1])
                .is_some_and(|p| p <= context),
            "context limit exceeded"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobResult {
    pub protocol: u32,
    pub job_id: String,
    pub request_id: String,
    pub model_sha256: String,
    pub range: BlockRange,
    pub position: usize,
    pub output: Option<TensorFrame>,
    pub error: Option<String>,
    pub elapsed_ms: Option<f64>,
}
impl JobResult {
    pub fn matches(&self, j: &InferenceJob) -> Result<()> {
        ensure!(
            self.protocol == j.protocol
                && self.job_id == j.id
                && self.request_id == j.request_id
                && self.model_sha256 == j.model_sha256
                && self.range == j.range
                && self.position == j.position,
            "stale or mismatched result"
        );
        if let Some(e) = &self.error {
            bail!("connector execution failed: {e}");
        }
        ensure!(self.output.is_some(), "missing result tensor");
        ensure!(
            self.elapsed_ms.is_none_or(|x| x.is_finite() && x >= 0.0),
            "invalid timing"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    #[serde(default)]
    pub kernels: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub protocol: u32,
    pub name: String,
    pub kind: String,
    pub memory_budget_mb: usize,
}
impl Registration {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.kernels.len() <= 32
                && self.capabilities.len() <= 32
                && self
                    .kernels
                    .iter()
                    .chain(&self.capabilities)
                    .all(|s| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control)),
            "invalid worker capability declaration"
        );
        ensure!(self.protocol == PROTOCOL, "unsupported protocol");
        ensure!(
            !self.name.trim().is_empty()
                && self.name.len() <= 64
                && !self.name.chars().any(char::is_control),
            "invalid connector name"
        );
        ensure!(
            ["native", "web"].contains(&self.kind.as_str()),
            "unsupported connector kind"
        );
        ensure!(
            (256..=8192).contains(&self.memory_budget_mb),
            "memory budget must be256..8192MiB"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub protocol: u32,
    pub connector_id: String,
    pub lease_token: String,
    pub model: ModelIdentity,
    pub config: serde_json::Value,
    pub range: BlockRange,
    pub weights_path: String,
    pub weights_sha256: String,
    pub weights_bytes: usize,
    pub context_limit: usize,
    pub memory_budget_mb: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateRequest {
    pub prompt: String,
    #[serde(default = "default_tokens")]
    pub max_tokens: usize,
    #[serde(default)]
    pub local_only: bool,
    #[serde(default)]
    pub raw_prompt: bool,
    #[serde(default)]
    pub split_axis: Option<String>,
    #[serde(default)]
    pub preferred_chunk: Option<u64>,
    #[serde(default)]
    pub connector_count: Option<usize>,
    #[serde(default)]
    pub working_memory_bytes: Option<u64>,
}
fn default_tokens() -> usize {
    32
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationEvent {
    pub request_id: String,
    pub kind: String,
    pub text: String,
    pub token: Option<u32>,
    pub tokens: usize,
    pub elapsed_ms: f64,
    pub remote_blocks: usize,
    #[serde(default)]
    pub remote_shards: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorStatus {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub range: BlockRange,
    pub ready: bool,
    pub online: bool,
    pub completed_jobs: u64,
    pub last_seen_ms: u64,
    pub memory_budget_mb: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostStatus {
    pub protocol: u32,
    pub version: String,
    pub model: ModelIdentity,
    pub active_request: Option<String>,
    pub connectors: Vec<ConnectorStatus>,
    pub context_limit: usize,
    pub max_tokens: usize,
    pub total_layers: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranges_and_frames_are_bounded() {
        assert!(BlockRange { start: 3, end: 2 }.validate(16).is_err());
        assert!(BlockRange { start: 0, end: 17 }.validate(16).is_err());
        assert!(TensorFrame {
            shape: [1, 1, 2],
            values: vec![0.0, f32::NAN]
        }
        .validate(2, 16)
        .is_err());
        assert!(TensorFrame {
            shape: [1, 1, 2],
            values: vec![1.0, 2.0]
        }
        .validate(2, 16)
        .is_ok());
    }
    #[test]
    fn invalid_connector_is_not_admitted() {
        assert!(Registration {
            kernels: vec![],
            capabilities: vec![],
            protocol: 1,
            name: "x".into(),
            kind: "relay".into(),
            memory_budget_mb: 512
        }
        .validate()
        .is_err());
    }
}

pub mod shards;
