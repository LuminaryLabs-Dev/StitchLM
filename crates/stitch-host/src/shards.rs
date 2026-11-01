//! Live shard command buffer shared by native and web connector endpoints.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use stitch_core::shards::*;
use stitch_core::ModelIdentity;
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
#[derive(Clone)]
pub struct MatrixSource {
    pub model: ModelIdentity,
    pub tensor_id: String,
    pub rows: u64,
    pub columns: u64,
    pub packed: bool,
    pub bytes: Arc<Vec<u8>>,
}
impl MatrixSource {
    pub fn bonsai(path: &std::path::Path) -> Result<Self> {
        ensure!(
            std::fs::metadata(path)?.len() == 7165121600,
            "Bonsai file size differs from pinned artifact"
        );
        let mut file = std::fs::File::open(path)?;
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        let hash = hex::encode(hash.finalize());
        ensure!(
            hash == "527f276ddf047b3494de964985b5529b9fc3ecf739ea64d8fab3a6ef8403e81d",
            "Bonsai file is not the pinned verified artifact"
        );
        let report = stitch_model::gguf::inspect(&mut file)?;
        ensure!(
            report.architecture.as_deref() == Some("qwen35"),
            "unexpected architecture"
        );
        let tensor = report
            .tensors
            .iter()
            .find(|t| t.name == "blk.0.ffn_gate.weight")
            .context("projection not found")?;
        ensure!(
            tensor.quant_type == 42
                && tensor.dimensions == [5120, 17408]
                && tensor.storage_span_bytes == 23674880,
            "legacy projection layout mismatch"
        );
        file.seek(SeekFrom::Start(report.data_offset + tensor.offset))?;
        let mut bytes = vec![0; tensor.storage_span_bytes as usize];
        file.read_exact(&mut bytes)?;
        Ok(Self {
            model: ModelIdentity {
                id: "Hikari07jp/Ternary-Bonsai-27B-Abliterated-LowDeg-GGUF".into(),
                revision: "60b1880c4c2e30711e1c90867a057f71ab45d587".into(),
                weights_sha256: hash,
            },
            tensor_id: tensor.name.clone(),
            rows: 17408,
            columns: 5120,
            packed: true,
            bytes: Arc::new(bytes),
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectionTarget {
    Registered {
        resource_id: String,
    },
    Dense {
        rows: u64,
        columns: u64,
        weights: Vec<f32>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionRequest {
    pub target: ProjectionTarget,
    pub input: Vec<f32>,
    pub connector_count: usize,
    pub memory_limit_bytes: u64,
    #[serde(default)]
    pub max_working_bytes: Option<u64>,
    #[serde(default, alias = "preferred_rows")]
    pub preferred_chunk: Option<u64>,
    #[serde(default = "default_axis")]
    pub split_axis: String,
}
fn default_axis() -> String {
    "rows".into()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionReport {
    pub host_reference_ms: f64,
    pub scheduler_objective: String,
    pub request_id: String,
    pub operation: String,
    pub output: Vec<f32>,
    pub maximum_reference_error: f32,
    pub commands: Vec<ExecutionRecord>,
    pub definitions: Vec<ShardDefinition>,
    pub device_measurements: Vec<DeviceCapabilities>,
    pub checkpoint: Checkpoint,
    pub elapsed_ms: f64,
    pub full_model_inference: bool,
}
#[derive(Clone)]
pub struct ShardService {
    pub buffer: CommandBuffer,
    pub resources: ResourceStore,
    pub envelopes: BTreeMap<String, WorkEnvelope>,
    pub pending: BTreeMap<String, String>,
    pub weight_slices: BTreeMap<String, Vec<u8>>,
    pub devices: BTreeMap<String, DeviceCapabilities>,
    pub active_request: Option<String>,
}
impl ShardService {
    pub fn new() -> Result<Self> {
        Ok(Self {
            buffer: CommandBuffer::new(256, 4)?,
            resources: ResourceStore::new(256 * 1024 * 1024),
            envelopes: BTreeMap::new(),
            pending: BTreeMap::new(),
            weight_slices: BTreeMap::new(),
            devices: BTreeMap::new(),
            active_request: None,
        })
    }
    pub fn poll(&mut self, device: &str) -> Result<Option<WorkEnvelope>> {
        if let Some(id) = self.pending.get(device) {
            let record = self
                .buffer
                .records
                .get(id)
                .context("pending record missing")?;
            if !matches!(record.status, Status::Dispatched | Status::Running)
                || now_ms() > record.command.lease_expires_ms
            {
                return Ok(None);
            }
            if record.status == Status::Dispatched {
                let generation = record.command.assignment_generation;
                self.buffer.start(id, device, generation, now_ms())?;
            }
            if let Some(profile) = self.devices.get_mut(device) {
                profile.heartbeat_ms = now_ms();
            }
            return Ok(self.envelopes.get(id).cloned());
        }
        Ok(None)
    }
    pub fn weights(&self, device: &str, command: &str) -> Result<Vec<u8>> {
        let envelope = self.envelopes.get(command).context("unknown command")?;
        ensure!(
            envelope.command.device_id == device
                && self.pending.get(device).is_some_and(|x| x == command)
                && now_ms() <= envelope.command.lease_expires_ms,
            "resource lease denied"
        );
        self.weight_slices
            .get(command)
            .cloned()
            .context("weight slice unavailable")
    }
    pub fn accept(&mut self, result: ResultPacket) -> Result<bool> {
        let device = result.device_id.clone();
        let command = result.command_id.clone();
        let elapsed = result.execution_ms;
        let _transferred = result.transferred_bytes;
        let now = now_ms();
        let accepted = self.buffer.accept(result, now, &mut self.resources)?;
        if accepted {
            self.pending.remove(&device);
            self.weight_slices.remove(&command);
            if let Some(profile) = self.devices.get_mut(&device) {
                profile.current_workload = profile.current_workload.saturating_sub(1);
                profile.heartbeat_ms = now;
                let record = &self.buffer.records[&command];
                let def = &self.buffer.definitions[&record.command.shard_id];
                if let Some(ms) = elapsed {
                    let end_to_end = now.saturating_sub(record.dispatched_ms.unwrap_or(now)) as f64;
                    profile.measurements.push(Measurement {
                        operation: def.computation.operation.clone(),
                        kernel: def.computation.kernels[0].clone(),
                        units: def.computation.output_shape[0] * def.tensor.shape[1],
                        execution_ms: ms,
                        loading_ms: 0.,
                        conversion_ms: 0.,
                        result_return_ms: 0.,
                        measured_at_ms: now,
                        samples: 1,
                        uncertainty_ms: end_to_end * 0.1,
                        end_to_end_ms: Some(end_to_end),
                    });
                    profile.measured_at_ms = now;
                    let discard = profile.measurements.len().saturating_sub(256);
                    profile.measurements.drain(..discard);
                }
            }
        }
        Ok(accepted)
    }
}
pub fn run_projection(
    request: ProjectionRequest,
    source: MatrixSource,
    participants: Vec<(String, String, usize)>,
    service: Arc<Mutex<ShardService>>,
) -> Result<ProjectionReport> {
    run_projection_checked(request, source, participants, service, || Ok(()))
}
pub fn run_projection_checked(
    request: ProjectionRequest,
    source: MatrixSource,
    participants: Vec<(String, String, usize)>,
    service: Arc<Mutex<ShardService>>,
    check_cancel: impl Fn() -> Result<()>,
) -> Result<ProjectionReport> {
    check_cancel()?;

    ensure!(
        (1..=4).contains(&request.connector_count) && participants.len() >= request.connector_count,
        "requested ready connector count unavailable"
    );
    ensure!(
        (16 * 1024 * 1024..=1_000_000_000).contains(&request.memory_limit_bytes),
        "shard memory limit must be16MiB..1GB"
    );
    ensure!(
        request.input.len() as u64 == source.columns && request.input.iter().all(|v| v.is_finite()),
        "input dimensions invalid"
    );
    let started = Instant::now();
    let reference_start = Instant::now();
    let reference = if source.packed {
        stitch_model::ternary::matvec_rows(&source.bytes, &request.input)?
    } else {
        source
            .bytes
            .chunks_exact(source.columns as usize * 4)
            .map(|row| {
                row.as_chunks::<4>()
                    .0
                    .iter()
                    .zip(&request.input)
                    .map(|(b, x)| f32::from_le_bytes(*b) * x)
                    .sum()
            })
            .collect::<Vec<f32>>()
    };
    check_cancel()?;
    let host_reference_ms = reference_start.elapsed().as_secs_f64() * 1000.;
    let request_id = super::secret();
    let chosen: Vec<_> = participants
        .into_iter()
        .take(request.connector_count)
        .collect();
    let devices: Vec<String> = chosen.iter().map(|(id, _, _)| id.clone()).collect();
    let spec = ProjectionSpec {
        id: format!("projection:{request_id}"),
        request_id: request_id.clone(),
        model: source.model.clone(),
        tensor_id: source.tensor_id.clone(),
        rows: source.rows,
        columns: source.columns,
        packed: source.packed,
        owner: "host".into(),
        devices: devices.clone(),
        memory_limit: request
            .max_working_bytes
            .map(|n| n.saturating_add(256 * 1024 * 1024))
            .unwrap_or(request.memory_limit_bytes)
            .min(request.memory_limit_bytes),
        runtime_overhead: 256 * 1024 * 1024,
    };
    let parent = projection_definition(&spec, &source.bytes, &request.input)?;
    ensure!(
        ["rows", "columns"].contains(&request.split_axis.as_str()),
        "unsupported split axis"
    );
    let prepared: Vec<PreparedShard> = if request.split_axis == "columns" {
        let group = if source.packed { 128 } else { 1 };
        let preferred = request.preferred_chunk.unwrap_or_else(|| {
            source
                .columns
                .div_ceil(request.connector_count as u64)
                .div_ceil(group)
                * group
        });
        split_columns(&parent, &source.bytes, &request.input, preferred)?
    } else {
        let preferred = request
            .preferred_chunk
            .unwrap_or_else(|| source.rows.div_ceil(request.connector_count as u64));
        split_rows(&parent, &source.bytes, preferred)?
            .into_iter()
            .map(|d| PreparedShard {
                weights: source.bytes
                    [d.storage.byte_range.start as usize..d.storage.byte_range.end as usize]
                    .to_vec(),
                input: request.input.clone(),
                source_range: d.assembly.output_slice.clone(),
                definition: d,
            })
            .collect()
    };
    let definitions: Vec<_> = prepared.iter().map(|p| p.definition.clone()).collect();
    let data: BTreeMap<_, _> = prepared
        .into_iter()
        .map(|p| (p.definition.id.clone(), p))
        .collect();
    ensure!(
        definitions.len() <= 256,
        "shard plan exceeds bounded queue; increase chunk size"
    );
    {
        let previous = {
            let old = service
                .lock()
                .map_err(|_| anyhow::anyhow!("shard service poisoned"))?;
            ensure!(old.active_request.is_none(), "shard request already active");
            old.devices.clone()
        };
        let mut s = ShardService::new()?;
        s.active_request = Some(request_id.clone());
        for (id, kind, memory) in chosen {
            let mut profile = DeviceCapabilities {
                id: id.clone(),
                version: 1,
                backend: if kind == "web" { "wasm" } else { "cpu" }.into(),
                capabilities: vec!["f32".into(), "little_endian".into()],
                kernels: vec![
                    "dense-f32-matvec/1".into(),
                    "prism-group128-matvec/1".into(),
                ],
                available_memory: (memory as u64 * 1024 * 1024).min(request.memory_limit_bytes),
                runtime_overhead: 256 * 1024 * 1024,
                current_workload: 0,
                maximum_in_flight: 1,
                network_latency_ms: None,
                bandwidth_bytes_per_ms: None,
                queue_delay_ms: None,
                measured_at_ms: 0,
                measurements: vec![],
                thermal_state: None,
                battery_percent: None,
                background_execution: None,
                online: true,
                heartbeat_ms: now_ms(),
                allowed_sensitivities: vec![Sensitivity::Internal],
            };
            if let Some(old) = previous.get(&id) {
                profile.measurements = old.measurements.clone();
                profile.measured_at_ms = old.measured_at_ms;
            }
            s.devices.insert(id, profile);
        }
        s.buffer.define(parent.clone())?;
        for def in &definitions {
            s.buffer.define(def.clone())?;
            let id = format!("command:{}", def.id);
            let now = now_ms();
            s.buffer
                .enqueue(&id, &def.id, 0, now, 60_000, Some(now + 120_000))?;
        }
        s.buffer.validate_graph()?;
        *service.lock().unwrap() = s;
    }
    let result = (|| -> Result<ProjectionReport> {
        loop {
            check_cancel()?;
            let mut s = service.lock().unwrap();
            ensure!(
                s.active_request.as_deref() == Some(&request_id),
                "projection cancelled"
            );
            let expired = s.buffer.expire(now_ms());
            ensure!(
                expired.is_empty(),
                "shard command timed out; explicit retry required"
            );
            let ready = s.buffer.ready(now_ms());
            for id in ready {
                if s.pending.len() >= request.connector_count {
                    break;
                }
                let def = s.buffer.definitions[&s.buffer.records[&id].command.shard_id].clone();
                let profiles: Vec<_> = s.devices.values().cloned().collect();
                let Some(placement) = choose_device(
                    &def,
                    &profiles,
                    def.computation.output_shape[0] * def.tensor.shape[1],
                    Some(
                        host_reference_ms
                            * (def.computation.output_shape[0] * def.tensor.shape[1]) as f64
                            / (source.rows * source.columns) as f64,
                    ),
                    now_ms(),
                    120_000,
                    Objective::Capacity,
                )?
                else {
                    continue;
                };
                let command = s.buffer.dispatch(&id, &placement.device_id, now_ms())?;
                s.buffer.records.get_mut(&id).unwrap().placement = Some(placement.clone());
                let prepared = &data[&def.id];
                let envelope = WorkEnvelope {
                    weights_path: format!(
                        "/api/connectors/{}/shard-weights/{}",
                        placement.device_id,
                        hex::encode(id.as_bytes())
                    ),
                    command,
                    definition: def,
                    input: prepared.input.clone(),
                };
                s.weight_slices.insert(id.clone(), prepared.weights.clone());
                s.pending.insert(placement.device_id.clone(), id.clone());
                s.devices
                    .get_mut(&placement.device_id)
                    .unwrap()
                    .current_workload += 1;
                s.envelopes.insert(id, envelope);
            }
            if s.buffer
                .records
                .values()
                .all(|r| r.status == Status::Completed)
            {
                break;
            }
            ensure!(
                started.elapsed() < Duration::from_secs(120),
                "projection deadline exceeded"
            );
            drop(s);
            std::thread::sleep(Duration::from_millis(10));
        }
        let s = service.lock().unwrap();
        let mut parts = Vec::new();
        for record in s.buffer.records.values() {
            let def = &s.buffer.definitions[&record.command.shard_id];
            let reference = record
                .accepted_output
                .as_ref()
                .context("missing accepted output")?;
            let bytes = s.resources.get(reference, "host")?;
            let values = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect();
            parts.push(SliceResult {
                contributor: def.id.clone(),
                range: data[&def.id].source_range.clone(),
                values,
                checksum: reference
                    .checksum
                    .clone()
                    .context("missing output checksum")?,
            });
        }
        let required: Vec<_> = definitions.iter().map(|d| d.id.clone()).collect();
        let output = if request.split_axis == "columns" {
            sum_reduction(
                &parts,
                &required,
                Range {
                    start: 0,
                    end: source.columns,
                },
                source.rows as usize,
            )?
        } else {
            concatenate(
                &parts,
                &required,
                Range {
                    start: 0,
                    end: source.rows,
                },
            )?
        };
        let error = output
            .iter()
            .zip(&reference)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        ensure!(
            output
                .iter()
                .zip(&reference)
                .all(|(a, b)| (a - b).abs() <= 0.002 + 0.0001 * b.abs()),
            "distributed projection failed absolute/relative reference comparison"
        );
        let checkpoint = s.buffer.checkpoint(
            &request_id,
            &source.model.weights_sha256,
            1,
            0,
            &s.resources,
        )?;
        Ok(ProjectionReport {
            host_reference_ms,
            scheduler_objective:
                "capacity calibration with measured cost ranking; negative savings remain visible"
                    .into(),
            request_id: request_id.clone(),
            operation: "matrix-vector projection, not complete model generation".into(),
            output,
            maximum_reference_error: error,
            commands: s.buffer.records.values().cloned().collect(),
            definitions,
            device_measurements: s.devices.values().cloned().collect(),
            checkpoint,
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.,
            full_model_inference: false,
        })
    })();
    let mut s = service.lock().unwrap();
    s.active_request = None;
    if result.is_err() {
        let ids: Vec<_> = s
            .buffer
            .records
            .iter()
            .filter(|(_, r)| r.status != Status::Completed)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            let _ = s.buffer.cancel(&id, "projection failed");
        }
        s.pending.clear();
    }
    result
}
