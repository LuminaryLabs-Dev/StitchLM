use super::definition::*;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Queued,
    Dispatched,
    Running,
    Completed,
    Cancelled,
    Failed,
    ReplayRequired,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub id: String,
    pub shard_id: String,
    pub definition_hash: String,
    pub request_id: String,
    pub plan_version: u64,
    pub device_id: String,
    pub assignment_generation: u64,
    pub sequence: u64,
    pub prerequisites: Vec<String>,
    pub input_refs: Vec<ResourceRef>,
    pub lease_expires_ms: u64,
    pub deadline_ms: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRecord {
    pub placement: Option<super::scheduler::Placement>,
    pub command: Command,
    pub status: Status,
    pub priority: i32,
    pub queued_ms: u64,
    pub dispatched_ms: Option<u64>,
    pub started_ms: Option<u64>,
    pub completed_ms: Option<u64>,
    pub heartbeat_ms: Option<u64>,
    pub timeout_ms: u64,
    pub retry_count: u32,
    pub failure_reason: Option<String>,
    pub accepted_output: Option<ResourceRef>,
    pub accepted_result_hash: Option<String>,
    pub cancellation_requested: bool,
    pub committed_position: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultPacket {
    pub command_id: String,
    pub shard_id: String,
    pub definition_hash: String,
    pub request_id: String,
    pub plan_version: u64,
    pub device_id: String,
    pub assignment_generation: u64,
    pub output: Vec<f32>,
    pub checksum: String,
    pub execution_ms: Option<f64>,
    pub transferred_bytes: u64,
}
pub fn float_bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceEntry {
    pub reference: ResourceRef,
    pub owner: String,
    pub allowed_devices: Vec<String>,
    pub sensitivity: Sensitivity,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone, Default)]
pub struct ResourceStore {
    entries: BTreeMap<String, ResourceEntry>,
    pub maximum_bytes: u64,
    used: u64,
}
impl ResourceStore {
    pub fn new(maximum_bytes: u64) -> Self {
        Self {
            maximum_bytes,
            ..Self::default()
        }
    }
    pub fn insert(&mut self, entry: ResourceEntry) -> Result<()> {
        ensure!(
            identity(&entry.reference.id)
                && entry.reference.version > 0
                && entry.reference.checksum.as_deref() == Some(digest(&entry.bytes).as_str()),
            "invalid resource integrity"
        );
        ensure!(identity(&entry.owner), "resource owner required");
        if let Some(old) = self.entries.get(&entry.reference.id) {
            ensure!(
                old.reference.version == entry.reference.version
                    && old.reference.checksum == entry.reference.checksum
                    && old.bytes == entry.bytes
                    && old.owner == entry.owner
                    && old.allowed_devices == entry.allowed_devices
                    && old.sensitivity == entry.sensitivity
                    && old.reference.access == entry.reference.access,
                "immutable resource conflict"
            );
            return Ok(());
        }
        let total = self
            .used
            .checked_add(entry.bytes.len() as u64)
            .context("resource accounting overflow")?;
        ensure!(
            total <= self.maximum_bytes,
            "resource store capacity exceeded"
        );
        self.used = total;
        self.entries.insert(entry.reference.id.clone(), entry);
        Ok(())
    }
    pub fn get(&self, reference: &ResourceRef, device: &str) -> Result<&[u8]> {
        let entry = self
            .entries
            .get(&reference.id)
            .context("resource missing")?;
        ensure!(
            entry.reference.version == reference.version
                && entry.reference.checksum == reference.checksum,
            "stale resource reference"
        );
        ensure!(
            device == entry.owner || entry.allowed_devices.iter().any(|d| d == device),
            "resource access denied"
        );
        Ok(&entry.bytes)
    }
    pub fn contains_exact(&self, r: &ResourceRef) -> bool {
        self.entries
            .get(&r.id)
            .is_some_and(|e| e.reference.version == r.version && e.reference.checksum == r.checksum)
    }
    pub fn references(&self) -> Vec<ResourceRef> {
        self.entries.values().map(|e| e.reference.clone()).collect()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateVersion {
    pub id: String,
    pub request_id: String,
    pub model_checksum: String,
    pub version: u64,
    pub owner: String,
    pub committed_position: u64,
    pub checksum: String,
    pub location: String,
    pub kind: String,
    pub replay_required: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub schema: u32,
    pub version: u64,
    pub request_id: String,
    pub model_checksum: String,
    pub plan_version: u64,
    pub token_position: u64,
    pub execution_position: u64,
    pub next_command_sequence: u64,
    pub definitions: BTreeMap<String, ShardDefinition>,
    pub records: BTreeMap<String, ExecutionRecord>,
    pub retained_resources: Vec<ResourceRef>,
    pub states: BTreeMap<String, StateVersion>,
    pub recovery_procedure: String,
    pub prompt_replay_required: bool,
    pub checksum: String,
}
impl Checkpoint {
    pub fn seal(&mut self) -> Result<()> {
        self.checksum.clear();
        self.checksum = digest(&serde_json::to_vec(self)?);
        Ok(())
    }
    pub fn verify(&self) -> Result<()> {
        let mut copy = self.clone();
        let expected = copy.checksum.clone();
        copy.seal()?;
        ensure!(
            checksum(&expected) && copy.checksum == expected,
            "checkpoint integrity mismatch"
        );
        ensure!(
            self.schema == 1
                && self.version > 0
                && identity(&self.request_id)
                && checksum(&self.model_checksum),
            "invalid checkpoint identity"
        );
        ensure!(
            self.next_command_sequence > 0 && self.execution_position < self.next_command_sequence,
            "invalid checkpoint execution cursor"
        );
        for (id, d) in &self.definitions {
            d.validate()?;
            ensure!(
                id == &d.id
                    && d.request_id == self.request_id
                    && d.weight_checksum == self.model_checksum
                    && d.plan_version == self.plan_version,
                "checkpoint mixes incompatible definitions"
            );
        }
        let mut sequences = BTreeSet::new();
        for (id, r) in &self.records {
            ensure!(
                id == &r.command.id
                    && r.command.request_id == self.request_id
                    && r.command.plan_version == self.plan_version
                    && r.command.sequence > 0
                    && r.command.sequence < self.next_command_sequence
                    && sequences.insert(r.command.sequence),
                "checkpoint command identity/sequence mismatch"
            );
        }
        for (id, state) in &self.states {
            ensure!(
                id == &state.id
                    && identity(&state.owner)
                    && state.version > 0
                    && state.request_id == self.request_id
                    && state.model_checksum == self.model_checksum
                    && checksum(&state.checksum),
                "checkpoint state ownership/model mismatch"
            );
        }
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct CommandBuffer {
    pub definitions: BTreeMap<String, ShardDefinition>,
    pub records: BTreeMap<String, ExecutionRecord>,
    pub states: BTreeMap<String, StateVersion>,
    queue: VecDeque<String>,
    maximum_queued: usize,
    maximum_in_flight: usize,
    next_sequence: u64,
}
impl CommandBuffer {
    pub fn new(maximum_queued: usize, maximum_in_flight: usize) -> Result<Self> {
        ensure!(
            maximum_queued > 0
                && maximum_queued <= 4096
                && maximum_in_flight > 0
                && maximum_in_flight <= 64,
            "invalid command-buffer bounds"
        );
        Ok(Self {
            definitions: BTreeMap::new(),
            records: BTreeMap::new(),
            states: BTreeMap::new(),
            queue: VecDeque::new(),
            maximum_queued,
            maximum_in_flight,
            next_sequence: 1,
        })
    }
    pub fn define(&mut self, definition: ShardDefinition) -> Result<String> {
        let hash = definition.hash()?;
        if let Some(old) = self.definitions.get(&definition.id) {
            ensure!(old.hash()? == hash, "immutable shard ID redefined");
        } else {
            ensure!(
                self.definitions.len() < self.maximum_queued * 4,
                "definition registry full"
            );
            self.definitions.insert(definition.id.clone(), definition);
        }
        Ok(hash)
    }
    pub fn enqueue(
        &mut self,
        id: &str,
        shard: &str,
        priority: i32,
        now: u64,
        timeout_ms: u64,
        deadline: Option<u64>,
    ) -> Result<()> {
        ensure!(
            identity(id) && !self.records.contains_key(id),
            "duplicate command ID"
        );
        ensure!(
            self.queue.len() < self.maximum_queued && self.records.len() < self.maximum_queued * 4,
            "command buffer backpressure"
        );
        ensure!(
            timeout_ms > 0 && timeout_ms <= 3_600_000 && deadline.is_none_or(|d| d > now),
            "invalid deadline/timeout"
        );
        let d = self.definitions.get(shard).context("undefined shard")?;
        ensure!(
            d.budget.fits(),
            "shard must be split to fit memory budget before enqueue"
        );
        let command = Command {
            id: id.into(),
            shard_id: shard.into(),
            definition_hash: d.hash()?,
            request_id: d.request_id.clone(),
            plan_version: d.plan_version,
            device_id: String::new(),
            assignment_generation: 0,
            sequence: self.next_sequence,
            prerequisites: d.inputs.prerequisites.clone(),
            input_refs: d
                .inputs
                .tensors
                .iter()
                .chain(d.inputs.states.iter())
                .cloned()
                .collect(),
            lease_expires_ms: 0,
            deadline_ms: deadline,
        };
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .context("command sequence overflow")?;
        self.records.insert(
            id.into(),
            ExecutionRecord {
                placement: None,
                command,
                status: Status::Queued,
                priority,
                queued_ms: now,
                dispatched_ms: None,
                started_ms: None,
                completed_ms: None,
                heartbeat_ms: None,
                timeout_ms,
                retry_count: 0,
                failure_reason: None,
                accepted_output: None,
                accepted_result_hash: None,
                cancellation_requested: false,
                committed_position: d.inputs.token_position,
            },
        );
        self.queue.push_back(id.into());
        Ok(())
    }
    pub fn ready(&self, now: u64) -> Vec<String> {
        let mut groups: BTreeMap<(String, u64, String), bool> = BTreeMap::new();
        for record in self.records.values() {
            if let Some(d) = self.definitions.get(&record.command.shard_id) {
                let key = (d.request_id.clone(), d.plan_version, d.operation_id.clone());
                let done = groups.entry(key).or_insert(true);
                *done &= record.status == Status::Completed;
            }
        }
        let complete: BTreeSet<_> = groups
            .into_iter()
            .filter(|(_, done)| *done)
            .map(|(key, _)| key)
            .collect();
        let mut ids: Vec<_> = self
            .queue
            .iter()
            .filter(|id| {
                self.records.get(*id).is_some_and(|r| {
                    r.status == Status::Queued
                        && !r.cancellation_requested
                        && r.command.deadline_ms.is_none_or(|d| d > now)
                        && r.command.prerequisites.iter().all(|p| {
                            complete.contains(&(
                                r.command.request_id.clone(),
                                r.command.plan_version,
                                p.clone(),
                            ))
                        })
                })
            })
            .cloned()
            .collect();
        ids.sort_by_key(|id| {
            (
                -i64::from(self.records[id].priority),
                self.records[id].command.sequence,
            )
        });
        ids
    }
    pub fn dispatch(&mut self, id: &str, device: &str, now: u64) -> Result<Command> {
        ensure!(
            identity(device) && self.ready(now).iter().any(|x| x == id),
            "command not dependency-ready"
        );
        ensure!(
            self.records
                .values()
                .filter(|r| matches!(r.status, Status::Dispatched | Status::Running))
                .count()
                < self.maximum_in_flight,
            "in-flight backpressure"
        );
        let r = self.records.get_mut(id).context("unknown command")?;
        let d = &self.definitions[&r.command.shard_id];
        ensure!(
            d.ownership.allowed_devices.iter().any(|x| x == device)
                && d.storage.approved_destinations.iter().any(|x| x == device),
            "device not authorized"
        );
        r.command.device_id = device.into();
        r.command.assignment_generation = r
            .command
            .assignment_generation
            .checked_add(1)
            .context("generation overflow")?;
        r.command.lease_expires_ms = now.checked_add(r.timeout_ms).context("lease overflow")?;
        r.status = Status::Dispatched;
        r.dispatched_ms = Some(now);
        r.heartbeat_ms = Some(now);
        self.queue.retain(|x| x != id);
        Ok(r.command.clone())
    }
    pub fn start(&mut self, id: &str, device: &str, generation: u64, now: u64) -> Result<()> {
        let r = self.records.get_mut(id).context("unknown command")?;
        ensure!(
            r.command.device_id == device
                && r.command.assignment_generation == generation
                && r.status == Status::Dispatched
                && now <= r.command.lease_expires_ms,
            "stale start"
        );
        r.status = Status::Running;
        r.started_ms = Some(now);
        r.heartbeat_ms = Some(now);
        Ok(())
    }
    pub fn accept(
        &mut self,
        result: ResultPacket,
        now: u64,
        resources: &mut ResourceStore,
    ) -> Result<bool> {
        let r = self
            .records
            .get_mut(&result.command_id)
            .context("unknown result command")?;
        let c = &r.command;
        ensure!(
            c.shard_id == result.shard_id
                && c.definition_hash == result.definition_hash
                && c.request_id == result.request_id
                && c.plan_version == result.plan_version
                && c.device_id == result.device_id
                && c.assignment_generation == result.assignment_generation,
            "stale/mismatched result identity"
        );
        let bytes = float_bytes(&result.output);
        ensure!(
            result.output.iter().all(|v| v.is_finite())
                && digest(&bytes) == result.checksum
                && result.execution_ms.is_none_or(|v| v.is_finite() && v >= 0.),
            "invalid result integrity/metrics"
        );
        let d = &self.definitions[&c.shard_id];
        let count = d
            .computation
            .output_shape
            .iter()
            .try_fold(1u64, |a, b| a.checked_mul(*b))
            .context("output size overflow")?;
        ensure!(
            result.output.len() as u64 == count && bytes.len() as u64 == d.computation.output_bytes,
            "output shape/length mismatch"
        );
        let fingerprint = digest(&serde_json::to_vec(&(
            &result.command_id,
            &result.definition_hash,
            result.assignment_generation,
            &result.checksum,
        ))?);
        if r.status == Status::Completed {
            ensure!(
                r.accepted_result_hash.as_ref() == Some(&fingerprint),
                "conflicting duplicate result"
            );
            return Ok(false);
        }
        ensure!(
            matches!(r.status, Status::Dispatched | Status::Running)
                && !r.cancellation_requested
                && now <= c.lease_expires_ms
                && c.deadline_ms.is_none_or(|t| now <= t),
            "result arrived outside active lease"
        );
        let reference = ResourceRef {
            id: format!("result:{}:{}", c.id, c.assignment_generation),
            version: 1,
            checksum: Some(result.checksum),
            access: Access::ReadOnly,
            location: "coordinator-memory".into(),
            available: true,
        };
        resources.insert(ResourceEntry {
            reference: reference.clone(),
            owner: d.ownership.owner.clone(),
            allowed_devices: d.ownership.allowed_devices.clone(),
            sensitivity: d.ownership.sensitivity.clone(),
            bytes,
        })?;
        r.status = Status::Completed;
        r.completed_ms = Some(now);
        r.accepted_output = Some(reference);
        r.accepted_result_hash = Some(fingerprint);
        Ok(true)
    }
    pub fn cancel(&mut self, id: &str, reason: &str) -> Result<()> {
        let r = self.records.get_mut(id).context("unknown command")?;
        ensure!(
            r.status != Status::Completed,
            "completed command cannot be cancelled"
        );
        r.status = Status::Cancelled;
        r.cancellation_requested = true;
        r.failure_reason = Some(reason.into());
        self.queue.retain(|x| x != id);
        Ok(())
    }
    pub fn retry(&mut self, id: &str, now: u64, reason: &str) -> Result<()> {
        ensure!(self.queue.len() < self.maximum_queued, "retry backpressure");
        let r = self.records.get_mut(id).context("unknown command")?;
        ensure!(
            !matches!(r.status, Status::Completed | Status::Cancelled)
                && r.retry_count < 3
                && !r.cancellation_requested,
            "retry forbidden"
        );
        ensure!(
            r.command.deadline_ms.is_none_or(|d| d > now),
            "deadline expired"
        );
        r.retry_count += 1;
        r.status = Status::Queued;
        r.failure_reason = Some(reason.into());
        r.command.assignment_generation = r
            .command
            .assignment_generation
            .checked_add(1)
            .context("assignment generation overflow")?;
        r.command.device_id.clear();
        r.command.lease_expires_ms = 0;
        if !self.queue.iter().any(|x| x == id) {
            self.queue.push_back(id.into());
        }
        Ok(())
    }
    pub fn expire(&mut self, now: u64) -> Vec<String> {
        let mut expired = Vec::new();
        for (id, r) in &mut self.records {
            if matches!(r.status, Status::Running | Status::Dispatched)
                && now > r.command.lease_expires_ms
            {
                r.status = Status::Failed;
                r.failure_reason = Some("assignment timeout".into());
                expired.push(id.clone());
            }
        }
        expired
    }
    pub fn commit_state(
        &mut self,
        next: StateVersion,
        expected_version: u64,
        writer: &str,
    ) -> Result<()> {
        ensure!(
            identity(&next.id)
                && identity(&next.request_id)
                && checksum(&next.model_checksum)
                && checksum(&next.checksum)
                && next.owner == writer,
            "invalid state ownership/integrity"
        );
        if let Some(old) = self.states.get(&next.id) {
            ensure!(
                old.owner == writer
                    && old.request_id == next.request_id
                    && old.model_checksum == next.model_checksum
                    && old.version == expected_version
                    && next.version == expected_version + 1
                    && next.committed_position >= old.committed_position,
                "state ownership/version conflict"
            );
        } else {
            ensure!(
                expected_version == 0 && next.version == 1,
                "initial state version must be1"
            );
        }
        self.states.insert(next.id.clone(), next);
        Ok(())
    }
    pub fn checkpoint(
        &self,
        request: &str,
        model: &str,
        plan: u64,
        token: u64,
        resources: &ResourceStore,
    ) -> Result<Checkpoint> {
        let mut ordered: Vec<_> = self
            .records
            .values()
            .filter(|r| r.command.request_id == request)
            .collect();
        ordered.sort_by_key(|r| r.command.sequence);
        let execution_position = ordered
            .into_iter()
            .take_while(|r| r.status == Status::Completed)
            .last()
            .map_or(0, |r| r.command.sequence);
        let mut cp=Checkpoint{
            schema:1,version:1,request_id:request.into(),model_checksum:model.into(),
            plan_version:plan,token_position:token,execution_position,next_command_sequence:self.next_sequence,
            definitions:self.definitions.iter().filter(|(_,d)|d.request_id==request).map(|(k,v)|(k.clone(),v.clone())).collect(),
            records:self.records.iter().filter(|(_,r)|r.command.request_id==request).map(|(k,v)|(k.clone(),v.clone())).collect(),
            retained_resources:resources.references(),
            states:self.states.iter().filter(|(_,state)|state.request_id==request).map(|(k,v)|(k.clone(),v.clone())).collect(),
            recovery_procedure:"restore exact retained resources; invalidate in-flight leases; replay missing mutable state from prompt".into(),
            prompt_replay_required:self.states.values().any(|state|state.request_id==request&&state.replay_required),
            checksum:String::new(),
        };
        cp.seal()?;
        cp.verify()?;
        Ok(cp)
    }
    pub fn restore(
        cp: &Checkpoint,
        resources: &ResourceStore,
        maximum_queued: usize,
        maximum_in_flight: usize,
    ) -> Result<Self> {
        cp.verify()?;
        let mut b = Self::new(maximum_queued, maximum_in_flight)?;
        for d in cp.definitions.values() {
            b.define(d.clone())?;
        }
        ensure!(
            cp.records.len() <= maximum_queued * 4,
            "checkpoint exceeds record capacity"
        );
        b.records = cp.records.clone();
        b.states = cp.states.clone();
        b.next_sequence = cp.next_command_sequence;
        for (id, r) in &mut b.records {
            ensure!(
                b.definitions
                    .get(&r.command.shard_id)
                    .is_some_and(|d| d.hash().ok().as_ref() == Some(&r.command.definition_hash)),
                "checkpoint definition mismatch"
            );
            if r.status == Status::Completed
                && r.accepted_output
                    .as_ref()
                    .is_none_or(|v| !resources.contains_exact(v))
            {
                r.status = Status::ReplayRequired;
                r.failure_reason = Some("completed output not retained".into());
            } else if matches!(
                r.status,
                Status::Dispatched | Status::Running | Status::Queued
            ) {
                r.command.assignment_generation = r
                    .command
                    .assignment_generation
                    .checked_add(1)
                    .context("assignment generation overflow")?;
                r.command.device_id.clear();
                r.command.lease_expires_ms = 0;
                r.status = Status::Queued;
                ensure!(
                    b.queue.len() < maximum_queued,
                    "restore queue exceeds bounds"
                );
                b.queue.push_back(id.clone());
            }
        }
        for state in b.states.values_mut() {
            state.replay_required = !resources.contains_exact(&ResourceRef {
                id: state.id.clone(),
                version: state.version,
                checksum: Some(state.checksum.clone()),
                access: Access::Exclusive,
                location: state.location.clone(),
                available: true,
            });
        }
        Ok(b)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkEnvelope {
    pub command: Command,
    pub definition: ShardDefinition,
    pub weights_path: String,
    pub input: Vec<f32>,
}

impl CommandBuffer {
    pub fn heartbeat(&mut self, id: &str, device: &str, generation: u64, now: u64) -> Result<()> {
        let r = self.records.get_mut(id).context("unknown command")?;
        ensure!(
            matches!(r.status, Status::Running | Status::Dispatched)
                && r.command.device_id == device
                && r.command.assignment_generation == generation
                && now <= r.command.lease_expires_ms
                && r.heartbeat_ms.is_none_or(|old| now >= old),
            "stale heartbeat"
        );
        r.heartbeat_ms = Some(now);
        let next = now
            .checked_add(r.timeout_ms)
            .context("heartbeat overflow")?;
        r.command.lease_expires_ms = r.command.deadline_ms.map_or(next, |d| d.min(next));
        Ok(())
    }
    pub fn migrate_state(
        &mut self,
        id: &str,
        expected: u64,
        writer: &str,
        new_owner: &str,
        allowed: &[String],
        resources: &ResourceStore,
    ) -> Result<()> {
        let s = self.states.get_mut(id).context("unknown state")?;
        ensure!(
            s.owner == writer && s.version == expected && allowed.iter().any(|d| d == new_owner),
            "state migration authority/version conflict"
        );
        let r = ResourceRef {
            id: s.id.clone(),
            version: s.version,
            checksum: Some(s.checksum.clone()),
            access: Access::Exclusive,
            location: s.location.clone(),
            available: true,
        };
        ensure!(
            resources.contains_exact(&r),
            "state bytes must be retained before migration"
        );
        s.owner = new_owner.into();
        s.version = s.version.checked_add(1).context("state version overflow")?;
        s.replay_required = true;
        Ok(())
    }
}

impl WorkEnvelope {
    /// Validate admission before allocating or fetching the weight payload.
    pub fn validate_transfer(&self, device: &str, memory_limit: u64) -> Result<()> {
        self.definition.validate()?;
        ensure!(
            self.command.device_id == device
                && self.command.shard_id == self.definition.id
                && self.command.definition_hash == self.definition.hash()?
                && self.command.request_id == self.definition.request_id
                && self.command.plan_version == self.definition.plan_version
                && self.command.assignment_generation > 0,
            "unbound shard transfer"
        );
        let b = &self.definition.budget;
        ensure!(
            b.fits()
                && b.estimated_peak
                    .checked_add(b.runtime_overhead)
                    .is_some_and(|n| n <= memory_limit),
            "shard exceeds device memory admission"
        );
        ensure!(
            self.definition.tensor.byte_length <= 256 * 1024 * 1024
                && self.definition.tensor.byte_length <= b.packed_weights
                && self.input.len() <= 65536
                && self.input.len() as u64 * 4 <= b.inputs,
            "payload exceeds declared buffers"
        );
        ensure!(
            self.definition
                .ownership
                .allowed_devices
                .iter()
                .any(|id| id == device)
                && self
                    .definition
                    .storage
                    .approved_destinations
                    .iter()
                    .any(|id| id == device),
            "transfer destination not authorized"
        );
        ensure!(
            self.definition.inputs.tensors.len() == 1
                && self.definition.inputs.states.is_empty()
                && self.definition.state.is_none(),
            "stateless transfer has unsupported state/inputs"
        );
        ensure!(
            self.input.iter().all(|x| x.is_finite())
                && self.definition.inputs.tensors[0].checksum.as_deref()
                    == Some(digest(&float_bytes(&self.input)).as_str()),
            "input integrity failed before transfer"
        );
        Ok(())
    }
}

impl CommandBuffer {
    pub fn validate_graph(&self) -> Result<()> {
        type Key = (String, u64, String);
        let mut graph: BTreeMap<Key, BTreeSet<Key>> = BTreeMap::new();
        for r in self.records.values() {
            let d = self
                .definitions
                .get(&r.command.shard_id)
                .context("undefined graph operation")?;
            let key = (d.request_id.clone(), d.plan_version, d.operation_id.clone());
            let edges = graph.entry(key).or_default();
            for dependency in &r.command.prerequisites {
                edges.insert((d.request_id.clone(), d.plan_version, dependency.clone()));
            }
        }
        ensure!(
            graph.values().flatten().all(|key| graph.contains_key(key)),
            "dependency has no producer in this request/plan"
        );
        let mut complete = BTreeSet::new();
        loop {
            let ready: Vec<_> = graph
                .iter()
                .filter(|(key, deps)| !complete.contains(*key) && deps.is_subset(&complete))
                .map(|(key, _)| key.clone())
                .collect();
            if ready.is_empty() {
                break;
            }
            complete.extend(ready);
        }
        ensure!(complete.len() == graph.len(), "dependency cycle detected");
        Ok(())
    }
}
