use super::*;
use anyhow::{ensure, Result};
use std::collections::BTreeMap;
#[derive(Debug, Clone)]
pub struct ProjectionSpec {
    pub id: String,
    pub request_id: String,
    pub model: crate::ModelIdentity,
    pub tensor_id: String,
    pub rows: u64,
    pub columns: u64,
    pub packed: bool,
    pub owner: String,
    pub devices: Vec<String>,
    pub memory_limit: u64,
    pub runtime_overhead: u64,
}
pub fn projection_definition(
    spec: &ProjectionSpec,
    weights: &[u8],
    input: &[f32],
) -> Result<ShardDefinition> {
    ensure!(
        spec.rows > 0
            && spec.rows <= 262144
            && spec.columns > 0
            && spec.columns <= 65536
            && spec.columns == input.len() as u64
            && input.iter().all(|v| v.is_finite()),
        "projection input dimensions mismatch"
    );
    spec.model.validate()?;
    let stride = if spec.packed {
        ensure!(
            spec.columns.is_multiple_of(128),
            "packed columns must align to128"
        );
        spec.columns / 128 * 34
    } else {
        spec.columns
            .checked_mul(4)
            .ok_or_else(|| anyhow::anyhow!("stride overflow"))?
    };
    ensure!(
        spec.rows.checked_mul(stride) == Some(weights.len() as u64),
        "weight byte size mismatch"
    );
    let resource = ResourceRef {
        id: format!("weights:{}", spec.id),
        version: 1,
        checksum: Some(digest(weights)),
        access: Access::ReadOnly,
        location: "coordinator-memory".into(),
        available: true,
    };
    let input_ref = ResourceRef {
        id: format!("input:{}", spec.id),
        version: 1,
        checksum: Some(digest(&float_bytes(input))),
        access: Access::ReadOnly,
        location: "coordinator-memory".into(),
        available: true,
    };
    let output = ResourceRef {
        id: format!("output:{}", spec.id),
        version: 1,
        checksum: None,
        access: Access::Exclusive,
        location: "coordinator-memory".into(),
        available: false,
    };
    let weight_bytes = weights.len() as u64;
    let input_bytes = spec.columns * 4;
    let output_bytes = spec.rows * 4;
    let temporary = 131072u64;
    let peak = weight_bytes
        .checked_mul(2)
        .and_then(|x| x.checked_add(input_bytes + output_bytes + temporary))
        .ok_or_else(|| anyhow::anyhow!("budget overflow"))?;
    let d = ShardDefinition {
        schema: 1,
        id: spec.id.clone(),
        parent_id: None,
        model_id: spec.model.id.clone(),
        model_revision: spec.model.revision.clone(),
        weight_checksum: spec.model.weights_sha256.clone(),
        request_id: spec.request_id.clone(),
        plan_version: 1,
        layer_id: Some("projection".into()),
        operation_id: format!("operation:{}", spec.id),
        tensor_id: spec.tensor_id.clone(),
        ownership: Ownership {
            owner: spec.owner.clone(),
            allowed_devices: spec.devices.clone(),
            access: Access::ReadOnly,
            sensitivity: Sensitivity::Internal,
        },
        tensor: TensorLayout {
            shape: vec![spec.rows, spec.columns],
            axes: vec!["rows".into(), "columns".into()],
            dtype: if spec.packed {
                DType::Packed
            } else {
                DType::F32
            },
            precision_bits: if spec.packed { 2 } else { 32 },
            quantization: spec.packed.then(|| Quantization {
                codec: "prism-legacy-group128".into(),
                version: 1,
                group_size: 128,
                packed_group_bytes: 34,
                scales_ref: Some("inline-f16-per-group".into()),
                zero_points_ref: None,
            }),
            transforms: vec![],
            layout: "row_major".into(),
            strides: vec![spec.columns, 1],
            alignment: 2,
            byte_order: "little".into(),
            slices: BTreeMap::from([
                (
                    "rows".into(),
                    Range {
                        start: 0,
                        end: spec.rows,
                    },
                ),
                (
                    "columns".into(),
                    Range {
                        start: 0,
                        end: spec.columns,
                    },
                ),
            ]),
            byte_offset: 0,
            byte_length: weight_bytes,
            checksum: digest(weights),
        },
        splitting: SplitRules {
            axes: vec!["rows".into(), "columns".into()],
            minimum: 1,
            preferred: spec.rows.min(1024),
            maximum: spec.rows.max(spec.columns),
            alignment: 1,
            quantization_boundary: if spec.packed { 128 } else { 1 },
            overlap: 0,
            neighbor_refs: vec![],
            independent: true,
            recursive: true,
            child_rule: "contiguous output rows; preserve complete input and packed column groups"
                .into(),
            colocated_refs: vec![],
        },
        inputs: Inputs {
            tensors: vec![input_ref],
            states: vec![],
            prerequisites: vec![],
            completion_rule: "all input versions available".into(),
            token_position: 0,
            valid_sequence_length: 1,
        },
        computation: Contract {
            operation: if spec.packed {
                Operation::PackedGroup128MatVec
            } else {
                Operation::DenseMatVec
            },
            parameters: BTreeMap::from([("columns".into(), serde_json::json!(spec.columns))]),
            kernels: vec![if spec.packed {
                "prism-group128-matvec/1"
            } else {
                "dense-f32-matvec/1"
            }
            .into()],
            kernel_version: 1,
            capabilities: vec!["f32".into(), "little_endian".into()],
            input_precision: DType::F32,
            accumulation_precision: DType::F32,
            output_precision: DType::F32,
            absolute_tolerance: 0.002,
            relative_tolerance: 0.0001,
            deterministic: true,
            output_shape: vec![spec.rows],
            output_bytes,
            unsupported: vec![
                "unrecognized transforms".into(),
                "unvalidated quantization layouts".into(),
            ],
            fallback: "host reference kernel".into(),
        },
        budget: Budget {
            packed_weights: weight_bytes,
            expanded_weights: 0,
            inputs: input_bytes,
            outputs: output_bytes,
            temporary,
            cache: 0,
            persistent_state: 0,
            transfer: weight_bytes,
            decompression: 0,
            prefetch: 0,
            estimated_peak: peak,
            runtime_overhead: spec.runtime_overhead,
            memory_limit: spec.memory_limit,
            cpu_percent: 10,
            threads: 1,
            accelerators: vec!["cpu".into(), "wasm".into()],
        },
        storage: Storage {
            resource,
            byte_range: Range {
                start: 0,
                end: weight_bytes,
            },
            residency: Residency::Streamed,
            compression: "none".into(),
            serialization: "raw-tensor-bytes".into(),
            transfer_bytes: weight_bytes,
            measured_transfer_ms: None,
            replicas: vec![],
            prefetch_priority: 0,
            retain_until_ms: None,
            eviction_rule: "after request completion or explicit cancellation".into(),
            integrity: "sha256".into(),
            approved_destinations: spec.devices.clone(),
        },
        assembly: Assembly {
            destination: output,
            output_slice: Range {
                start: 0,
                end: spec.rows,
            },
            operation: Merge::Concatenate,
            contributors: vec![],
            missing_policy: "wait or fail; never silently omit".into(),
            order: vec![],
            accumulation_precision: DType::F32,
            validations: vec![
                "exact coverage".into(),
                "finite values".into(),
                "checksums".into(),
            ],
            completion_rule: "all declared contributors accepted".into(),
        },
        state: None,
    };
    d.validate()?;
    Ok(d)
}
/// Child digests are computed from actual bytes, never inherited as invented hashes.
pub fn split_rows(
    parent: &ShardDefinition,
    weights: &[u8],
    preferred_rows: u64,
) -> Result<Vec<ShardDefinition>> {
    parent.validate()?;
    ensure!(
        parent.tensor.shape.len() == 2 && parent.inputs.tensors.len() == 1,
        "projection splitting requires rank-two weights and one input"
    );
    ensure!(
        parent.parent_id.is_none() || parent.splitting.recursive,
        "recursive splitting forbidden"
    );
    if parent.computation.operation == Operation::PackedGroup128MatVec {
        let q = parent
            .tensor
            .quantization
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("quantization contract missing"))?;
        ensure!(
            q.codec == "prism-legacy-group128"
                && q.version == 1
                && q.group_size == 128
                && q.packed_group_bytes == 34,
            "unsupported packed layout"
        );
    }
    ensure!(
        parent.splitting.axes.iter().any(|a| a == "rows")
            && parent.splitting.independent
            && parent.splitting.overlap == 0
            && parent.state.is_none(),
        "row splitting is not legal for this operation"
    );
    ensure!(
        matches!(
            parent.computation.operation,
            Operation::DenseMatVec | Operation::PackedGroup128MatVec
        ) && parent.tensor.transforms.is_empty(),
        "unsupported split kernel/transform"
    );
    ensure!(
        digest(weights) == parent.tensor.checksum
            && weights.len() as u64 == parent.tensor.byte_length,
        "parent tensor integrity mismatch"
    );
    let range = parent
        .tensor
        .slices
        .get("rows")
        .ok_or_else(|| anyhow::anyhow!("rows slice required"))?;
    let rows = range.length()?;
    ensure!(
        parent.tensor.byte_length.is_multiple_of(rows),
        "nonuniform row storage"
    );
    let stride = parent.tensor.byte_length / rows;
    let fixed = parent
        .budget
        .inputs
        .checked_add(parent.budget.temporary)
        .and_then(|n| n.checked_add(parent.budget.runtime_overhead))
        .ok_or_else(|| anyhow::anyhow!("fixed memory overflow"))?;
    let available = parent
        .budget
        .memory_limit
        .checked_sub(fixed)
        .ok_or_else(|| anyhow::anyhow!("no memory available for weights"))?;
    let per_row = stride
        .checked_mul(2)
        .and_then(|n| n.checked_add(4))
        .ok_or_else(|| anyhow::anyhow!("row memory overflow"))?;
    let fitting = available / per_row;
    let count = preferred_rows
        .min(parent.splitting.maximum)
        .min(fitting)
        .min(rows);
    ensure!(
        count >= parent.splitting.minimum
            && count > 0
            && count.is_multiple_of(parent.splitting.alignment),
        "no aligned shard fits budget"
    );
    let mut children = Vec::new();
    let mut start = 0;
    while start < rows {
        let mut end = start.saturating_add(count).min(rows);
        if rows - end > 0
            && rows - end < parent.splitting.minimum
            && rows - start <= parent.splitting.maximum.min(fitting)
        {
            end = rows;
        }
        ensure!(
            end - start >= parent.splitting.minimum,
            "tail shard violates minimum chunk size"
        );
        let a = (start * stride) as usize;
        let b = (end * stride) as usize;
        let mut child = parent.clone();
        child.id = format!(
            "{}/rows-{}-{}",
            parent.id,
            range.start + start,
            range.start + end
        );
        child.parent_id = Some(parent.id.clone());
        child.operation_id = child.id.clone();
        child.tensor.slices.insert(
            "rows".into(),
            Range {
                start: range.start + start,
                end: range.start + end,
            },
        );
        child.tensor.byte_offset = parent.tensor.byte_offset + start * stride;
        child.tensor.byte_length = (end - start) * stride;
        child.tensor.checksum = digest(&weights[a..b]);
        child.storage.byte_range = Range {
            start: parent.storage.byte_range.start + start * stride,
            end: parent.storage.byte_range.start + end * stride,
        };
        child.storage.transfer_bytes = child.tensor.byte_length;
        child.budget.packed_weights = child.tensor.byte_length;
        child.budget.transfer = child.tensor.byte_length;
        child.budget.outputs = (end - start) * 4;
        child.budget.estimated_peak = child.budget.packed_weights
            + child.budget.transfer
            + child.budget.inputs
            + child.budget.outputs
            + child.budget.temporary;
        child.computation.output_shape = vec![end - start];
        child.computation.output_bytes = (end - start) * 4;
        child.assembly.output_slice = Range {
            start: range.start + start,
            end: range.start + end,
        };
        child.assembly.contributors = vec![child.id.clone()];
        child.assembly.order = vec![child.id.clone()];
        child.validate()?;
        children.push(child);
        start = end;
    }
    Ok(children)
}

#[derive(Debug, Clone)]
pub struct PreparedShard {
    pub definition: ShardDefinition,
    pub weights: Vec<u8>,
    pub input: Vec<f32>,
    pub source_range: Range,
}
/// Reduction-axis splitting repacks complete quantization groups and records the
/// source column range separately from the child tensor's local shape.
pub fn split_columns(
    parent: &ShardDefinition,
    weights: &[u8],
    input: &[f32],
    preferred_columns: u64,
) -> Result<Vec<PreparedShard>> {
    parent.validate()?;
    ensure!(
        parent.tensor.shape.len() == 2 && parent.inputs.tensors.len() == 1,
        "projection splitting requires rank-two weights and one input"
    );
    ensure!(
        parent.parent_id.is_none() || parent.splitting.recursive,
        "recursive splitting forbidden"
    );
    if parent.computation.operation == Operation::PackedGroup128MatVec {
        let q = parent
            .tensor
            .quantization
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("quantization contract missing"))?;
        ensure!(
            q.codec == "prism-legacy-group128"
                && q.version == 1
                && q.group_size == 128
                && q.packed_group_bytes == 34,
            "unsupported packed layout"
        );
    }
    ensure!(
        parent.splitting.axes.iter().any(|a| a == "columns")
            && parent.splitting.independent
            && parent.splitting.overlap == 0,
        "column split is not permitted by the descriptor"
    );
    ensure!(
        matches!(
            parent.computation.operation,
            Operation::DenseMatVec | Operation::PackedGroup128MatVec
        ) && parent.state.is_none()
            && parent.tensor.transforms.is_empty(),
        "column split unsupported"
    );
    ensure!(
        digest(weights) == parent.tensor.checksum,
        "parent weights changed"
    );
    let rows = parent.tensor.shape[0];
    let columns = parent.tensor.shape[1];
    ensure!(
        input.len() as u64 == columns
            && digest(&float_bytes(input))
                == parent.inputs.tensors[0]
                    .checksum
                    .clone()
                    .unwrap_or_default(),
        "input changed"
    );
    let packed = parent.computation.operation == Operation::PackedGroup128MatVec;
    let group = if packed { 128 } else { 1 };
    let group_bytes = if packed { 34 } else { 4 };
    ensure!(
        columns.is_multiple_of(group) && preferred_columns >= group,
        "column split violates quantization grouping"
    );
    let fixed = parent.budget.runtime_overhead + parent.budget.temporary + rows * 4;
    let available = parent
        .budget
        .memory_limit
        .checked_sub(fixed)
        .ok_or_else(|| anyhow::anyhow!("column split cannot fit fixed buffers"))?;
    let per_group = rows
        .checked_mul(group_bytes * 2)
        .and_then(|n| n.checked_add(group * 4))
        .ok_or_else(|| anyhow::anyhow!("column budget overflow"))?;
    let groups = (preferred_columns / group)
        .min(available / per_group)
        .min(columns / group);
    ensure!(groups > 0, "one quantization group cannot fit");
    let chunk = groups * group;
    let row_stride = columns / group * group_bytes;
    ensure!(
        weights.len() as u64 == rows * row_stride,
        "parent matrix layout invalid"
    );
    let mut result = Vec::new();
    let mut start = 0;
    while start < columns {
        let end = (start + chunk).min(columns);
        let mut child_weights =
            Vec::with_capacity((rows * (end - start) / group * group_bytes) as usize);
        for row in 0..rows {
            let a = (row * row_stride + start / group * group_bytes) as usize;
            let b = (row * row_stride + end / group * group_bytes) as usize;
            child_weights.extend_from_slice(&weights[a..b]);
        }
        let child_input = input[start as usize..end as usize].to_vec();
        let spec = ProjectionSpec {
            id: format!("{}/columns-{start}-{end}", parent.id),
            request_id: parent.request_id.clone(),
            model: crate::ModelIdentity {
                id: parent.model_id.clone(),
                revision: parent.model_revision.clone(),
                weights_sha256: parent.weight_checksum.clone(),
            },
            tensor_id: parent.tensor_id.clone(),
            rows,
            columns: end - start,
            packed,
            owner: parent.ownership.owner.clone(),
            devices: parent.ownership.allowed_devices.clone(),
            memory_limit: parent.budget.memory_limit,
            runtime_overhead: parent.budget.runtime_overhead,
        };
        let mut d = projection_definition(&spec, &child_weights, &child_input)?;
        d.parent_id = Some(parent.id.clone());
        d.computation.parameters.insert(
            "source_columns".into(),
            serde_json::json!({"start":start,"end":end}),
        );
        d.computation.parameters.insert(
            "source_shape".into(),
            serde_json::json!(parent.tensor.shape),
        );
        d.assembly.destination = parent.assembly.destination.clone();
        d.assembly.operation = Merge::Sum;
        d.assembly.accumulation_precision = DType::F64;
        d.assembly.output_slice = Range {
            start: 0,
            end: rows,
        };
        d.assembly.contributors = vec![d.id.clone()];
        d.assembly.order = vec![d.id.clone()];
        d.validate()?;
        ensure!(d.budget.fits(), "column child exceeds budget");
        result.push(PreparedShard {
            definition: d,
            weights: child_weights,
            input: child_input,
            source_range: Range { start, end },
        });
        start = end;
    }
    Ok(result)
}
