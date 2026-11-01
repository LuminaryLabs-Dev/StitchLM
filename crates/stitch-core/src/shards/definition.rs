use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
pub const SHARD_SCHEMA: u32 = 1;
pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub fn identity(s: &str) -> bool {
    !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
}
pub fn checksum(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Range {
    pub start: u64,
    pub end: u64,
}
impl Range {
    pub fn length(&self) -> Result<u64> {
        ensure!(self.start < self.end, "empty or inverted slice");
        Ok(self.end - self.start)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DType {
    F32,
    F64,
    F16,
    Bf16,
    I8,
    U8,
    Packed,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    ReadOnly,
    Mutable,
    Exclusive,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    Public,
    Internal,
    Private,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Residency {
    Resident,
    Cached,
    Streamed,
    Missing,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    DenseMatVec,
    PackedGroup128MatVec,
    AttentionSegment,
    LfmBlocks,
    StateUpdate,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Merge {
    Concatenate,
    Sum,
    StableAttention,
    OrderedState,
    Identity,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ownership {
    pub owner: String,
    pub allowed_devices: Vec<String>,
    pub access: Access,
    pub sensitivity: Sensitivity,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quantization {
    pub codec: String,
    pub version: u32,
    pub group_size: u64,
    pub packed_group_bytes: u64,
    pub scales_ref: Option<String>,
    pub zero_points_ref: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TensorLayout {
    pub shape: Vec<u64>,
    pub axes: Vec<String>,
    pub dtype: DType,
    pub precision_bits: u32,
    pub quantization: Option<Quantization>,
    pub transforms: Vec<String>,
    pub layout: String,
    pub strides: Vec<u64>,
    pub alignment: u64,
    pub byte_order: String,
    pub slices: BTreeMap<String, Range>,
    pub byte_offset: u64,
    pub byte_length: u64,
    pub checksum: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplitRules {
    pub axes: Vec<String>,
    pub minimum: u64,
    pub preferred: u64,
    pub maximum: u64,
    pub alignment: u64,
    pub quantization_boundary: u64,
    pub overlap: u64,
    pub neighbor_refs: Vec<String>,
    pub independent: bool,
    pub recursive: bool,
    pub child_rule: String,
    pub colocated_refs: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceRef {
    pub id: String,
    pub version: u64,
    pub checksum: Option<String>,
    pub access: Access,
    pub location: String,
    pub available: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inputs {
    pub tensors: Vec<ResourceRef>,
    pub states: Vec<ResourceRef>,
    pub prerequisites: Vec<String>,
    pub completion_rule: String,
    pub token_position: u64,
    pub valid_sequence_length: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub operation: Operation,
    pub parameters: BTreeMap<String, serde_json::Value>,
    pub kernels: Vec<String>,
    pub kernel_version: u32,
    pub capabilities: Vec<String>,
    pub input_precision: DType,
    pub accumulation_precision: DType,
    pub output_precision: DType,
    pub absolute_tolerance: f64,
    pub relative_tolerance: f64,
    pub deterministic: bool,
    pub output_shape: Vec<u64>,
    pub output_bytes: u64,
    pub unsupported: Vec<String>,
    pub fallback: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub packed_weights: u64,
    pub expanded_weights: u64,
    pub inputs: u64,
    pub outputs: u64,
    pub temporary: u64,
    pub cache: u64,
    pub persistent_state: u64,
    pub transfer: u64,
    pub decompression: u64,
    pub prefetch: u64,
    pub estimated_peak: u64,
    pub runtime_overhead: u64,
    pub memory_limit: u64,
    pub cpu_percent: u32,
    pub threads: u32,
    pub accelerators: Vec<String>,
}
impl Budget {
    pub fn fits(&self) -> bool {
        self.estimated_peak
            .checked_add(self.runtime_overhead)
            .is_some_and(|n| n <= self.memory_limit)
    }
    pub fn validate(&self) -> Result<()> {
        let sum = [
            self.packed_weights,
            self.expanded_weights,
            self.inputs,
            self.outputs,
            self.temporary,
            self.cache,
            self.persistent_state,
            self.transfer,
            self.decompression,
            self.prefetch,
        ]
        .into_iter()
        .try_fold(0u64, |a, b| a.checked_add(b))
        .ok_or_else(|| anyhow::anyhow!("memory sum overflow"))?;
        ensure!(
            self.estimated_peak >= sum,
            "peak budget omits overlapping buffers"
        );
        ensure!(
            self.estimated_peak
                .checked_add(self.runtime_overhead)
                .is_some()
                && self.memory_limit > self.runtime_overhead,
            "invalid memory accounting/limit"
        );
        ensure!(
            (1..=100).contains(&self.cpu_percent) && self.threads > 0 && self.threads <= 256,
            "invalid CPU budget"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Storage {
    pub resource: ResourceRef,
    pub byte_range: Range,
    pub residency: Residency,
    pub compression: String,
    pub serialization: String,
    pub transfer_bytes: u64,
    pub measured_transfer_ms: Option<f64>,
    pub replicas: Vec<ResourceRef>,
    pub prefetch_priority: i32,
    pub retain_until_ms: Option<u64>,
    pub eviction_rule: String,
    pub integrity: String,
    pub approved_destinations: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assembly {
    pub destination: ResourceRef,
    pub output_slice: Range,
    pub operation: Merge,
    pub contributors: Vec<String>,
    pub missing_policy: String,
    pub order: Vec<String>,
    pub accumulation_precision: DType,
    pub validations: Vec<String>,
    pub completion_rule: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateContract {
    pub state: ResourceRef,
    pub kind: String,
    pub layer_range: Option<Range>,
    pub head_range: Option<Range>,
    pub token_range: Option<Range>,
    pub history_refs: Vec<ResourceRef>,
    pub update_sequence: u64,
    pub exclusive_writer: String,
    pub migration_rule: String,
    pub replication_rule: String,
    pub committed_position: u64,
    pub replay_required: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShardDefinition {
    pub schema: u32,
    pub id: String,
    pub parent_id: Option<String>,
    pub model_id: String,
    pub model_revision: String,
    pub weight_checksum: String,
    pub request_id: String,
    pub plan_version: u64,
    pub layer_id: Option<String>,
    pub operation_id: String,
    pub tensor_id: String,
    pub ownership: Ownership,
    pub tensor: TensorLayout,
    pub splitting: SplitRules,
    pub inputs: Inputs,
    pub computation: Contract,
    pub budget: Budget,
    pub storage: Storage,
    pub assembly: Assembly,
    pub state: Option<StateContract>,
}
impl ShardDefinition {
    pub fn hash(&self) -> Result<String> {
        self.validate()?;
        Ok(digest(&serde_json::to_vec(self)?))
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == SHARD_SCHEMA
                && identity(&self.id)
                && identity(&self.request_id)
                && identity(&self.operation_id)
                && identity(&self.tensor_id)
                && identity(&self.model_id)
                && identity(&self.ownership.owner)
                && self.plan_version > 0,
            "invalid shard identity"
        );
        ensure!(
            self.model_revision.len() == 40
                && self.model_revision.bytes().all(|b| b.is_ascii_hexdigit())
                && checksum(&self.weight_checksum),
            "model is not pinned"
        );
        ensure!(
            self.parent_id
                .as_ref()
                .is_none_or(|s| identity(s) && s != &self.id),
            "invalid parent"
        );
        let t = &self.tensor;
        ensure!(
            !t.shape.is_empty()
                && t.shape.len() <= 8
                && t.shape.len() == t.axes.len()
                && t.strides.len() == t.shape.len()
                && t.shape.iter().all(|n| *n > 0),
            "invalid tensor layout"
        );
        ensure!(
            t.alignment.is_power_of_two()
                && t.alignment <= 4096
                && t.byte_order == "little"
                && t.layout == "row_major"
                && checksum(&t.checksum),
            "unsupported tensor encoding"
        );
        ensure!(
            t.byte_offset.is_multiple_of(t.alignment)
                && t.byte_offset.checked_add(t.byte_length).is_some()
                && t.byte_length > 0,
            "invalid tensor byte span"
        );
        ensure!(
            t.shape
                .iter()
                .try_fold(1u64, |a, b| a.checked_mul(*b))
                .is_some(),
            "tensor shape overflows addressable elements"
        );
        ensure!(
            self.inputs.tensors.len() <= 128
                && self.inputs.states.len() <= 128
                && self.inputs.prerequisites.len() <= 4096
                && self.computation.output_shape.len() <= 8,
            "descriptor collection limits exceeded"
        );
        let mut seen = std::collections::BTreeSet::new();
        for (axis, n) in t.axes.iter().zip(&t.shape) {
            ensure!(
                identity(axis) && seen.insert(axis),
                "duplicate/invalid axis"
            );
            if let Some(r) = t.slices.get(axis) {
                r.length()?;
                ensure!(r.end <= *n, "slice outside tensor");
            }
        }
        ensure!(
            t.slices.keys().all(|a| seen.contains(a)),
            "unknown slice axis"
        );
        let s = &self.splitting;
        ensure!(
            s.minimum > 0
                && s.minimum <= s.preferred
                && s.preferred <= s.maximum
                && s.alignment > 0
                && s.quantization_boundary > 0,
            "invalid split limits"
        );
        ensure!(
            s.axes.iter().all(|a| t.axes.contains(a)),
            "split axis not present"
        );
        if let Some(q) = &t.quantization {
            ensure!(
                q.version > 0 && q.group_size > 0 && q.packed_group_bytes > 0 && identity(&q.codec),
                "invalid quantization contract"
            );
        }
        let c = &self.computation;
        ensure!(
            c.kernel_version > 0
                && !c.kernels.is_empty()
                && c.absolute_tolerance.is_finite()
                && c.absolute_tolerance >= 0.
                && c.relative_tolerance.is_finite()
                && c.relative_tolerance >= 0.,
            "invalid kernel/tolerance"
        );
        ensure!(
            !c.output_shape.is_empty()
                && c.output_shape.iter().all(|n| *n > 0)
                && c.output_bytes > 0,
            "invalid output contract"
        );
        self.budget.validate()?;
        self.storage.byte_range.length()?;
        ensure!(
            self.storage.byte_range.length()? == t.byte_length
                && self.storage.transfer_bytes >= t.byte_length,
            "storage span mismatch"
        );
        ensure!(
            self.storage
                .measured_transfer_ms
                .is_none_or(|v| v.is_finite() && v >= 0.),
            "invalid transfer measurement"
        );
        self.assembly.output_slice.length()?;
        ensure!(
            self.inputs
                .prerequisites
                .iter()
                .all(|id| identity(id) && id != &self.operation_id),
            "invalid self-dependency"
        );
        for r in self
            .inputs
            .tensors
            .iter()
            .chain(self.inputs.states.iter())
            .chain(std::iter::once(&self.storage.resource))
            .chain(std::iter::once(&self.assembly.destination))
        {
            ensure!(
                identity(&r.id) && r.version > 0 && r.checksum.as_ref().is_none_or(|s| checksum(s)),
                "invalid resource reference"
            );
        }
        if let Some(state) = &self.state {
            ensure!(
                identity(&state.exclusive_writer) && state.state.access == Access::Exclusive,
                "mutable state requires an exclusive writer"
            );
        }
        Ok(())
    }
}
