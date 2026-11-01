# Data-driven shard execution

The implementation separates three lifetimes:

1. **ShardDefinition** is an immutable, checksummed description of a piece of work.
2. **ExecutionRecord** tracks assignments, leases, progress and accepted results.
3. **DeviceCapabilities** records supported kernels, admitted memory and observations.

The Rust definitions live in `crates/stitch-core/src/shards/`. Native and WASM
connectors execute the same `stitch_worker::execute_shard` implementation. They
fetch only the weight slice authorized for their current command and lease.

## Field groups and behavior

- Identity/ownership: model revision and checksum, parent/shard/request/operation
  IDs, owner, sensitivity and allowed destinations. Redefining an existing shard
  ID with different bytes fails. Transfer endpoints bind device authentication to
  the current command; a descriptor does not grant arbitrary URL/file access.
- Tensor structure: shape, axes, type, precision, quantization groups, transforms,
  layout, strides, alignment, ranges, byte spans and digest. Unsupported layouts,
  transforms or precision combinations fail rather than being guessed.
- Split rules: permitted axes, bounds, group alignment, independence, overlap and
  recursion. Matrix row splits preserve complete input vectors; column splits
  repack complete quantization groups and retain their source column ranges.
- Inputs/dependencies: versioned, checksummed tensor/state references and operation
  prerequisites. Dependency-ready commands are selected before dispatch. Workers
  verify the actual input payload against its declared digest.
- Computation: operation/kernel version, capabilities, parameters, precision,
  output contract and tolerances. A registry entry is not proof of an implemented
  kernel; unsupported operation types remain explicit errors.
- Budgets: weights, expanded representation, input/output, temporary, cache,
  state, transfer, decompression, prefetch and runtime reservations. Oversized
  parent definitions may be split; executable children must fit. Preflight runs
  before fetching weight payloads. These estimates supplement, not replace, OS
  resource limits and measured process/heap memory.
- Storage: references, byte ranges, residency, encoding, checksums, replicas,
  retention and approved destinations. The current transport serves bounded raw
  tensor bytes. Compression and arbitrary storage providers are not implemented.
- Devices: supported kernels, workload, admitted capacity, timestamps, samples,
  observed execution/round-trip costs and uncertainty. Unavailable thermal,
  battery, background and network details remain unknown. The current cost model
  is a workload-scaled estimate with a margin, not a guaranteed speed prediction.
- Commands: priority, sequence, dependencies, assignment generation, timestamps,
  lease expiry, timeout, heartbeat, cancellation and backpressure. The live HTTP
  path allows four in-flight commands and bounded multi-round queues.
- Assembly: exact contributor coverage, checksums, destination ranges and merge
  rules. Rows concatenate; column partial products sum in a stable range order
  using F64 accumulation. Missing, overlapping or corrupt results cannot pass.
- State: owner/version compare-and-swap, committed positions and retained-state
  references. Moving ownership requires retained bytes; actual backend state
  restoration remains a separate requirement. A pointer cannot recreate a cache.
- Recovery: sealed checkpoints contain definitions, execution records, resource
  references and state ownership. In-flight leases are invalidated on restore.
  Missing outputs become ReplayRequired. Missing mutable state requires replay.

## Scheduling policy

The HTTP projection route performs **capacity calibration**: it deliberately uses
eligible connectors so their real costs can be measured. Later chunks rank idle
compatible devices by observed cost. Every placement retains the estimated cost
and expected saving versus a measured host reference when evidence is available.
A negative saving is not hidden or presented as a speedup.

The core Latency objective excludes measured remote placements slower than the
host and rejects unmeasured speed claims. It is unit-tested; the projection UI
uses capacity calibration, not an automatic globally optimal inference scheduler.
Admission capacity is an operator budget with a runtime reservation, not a claim
that the OS/browser reported that much free physical RAM.

## Supported execution and coverage boundaries

- Dense F32 matrix-vector row and column partitioning: native/WASM execution.
- Exact pinned legacy Prism group128 matrix-vector projection: native/WASM
  execution using real Bonsai GGUF weights, including multi-round command buffers.
- Stable attention-segment merge: mathematical implementation and reference unit
  tests. It is not enabled as a complete model-attention sharding backend.
- Mutable-state ownership, version conflicts and migration fences: control-layer
  implementation and tests. This does not serialize/restore an arbitrary model's
  KV, convolution or recurrent cache.
- Complete LFM generation retains its validated contiguous-block protocol and
  cache-aware worker. The generic operation descriptor path is not a replacement
  for every LFM internal kernel in this version.
- The pinned Bonsai text graph now calls these matrix kernels throughout full
  generation, with all nonlinear, KV and recurrent state owned by the host.
  Arbitrary GGUF models and distributed attention/state execution remain unsupported.

## Inspect and recover

Host-authorized APIs:

- POST /api/shards/project: execute a bounded row/column plan.
- GET /api/shards/status: live command states, placements and device observations.
- GET /api/shards/checkpoint: capture the current sealed execution checkpoint.
- POST /api/shards/restore: validate and restore retained records with fresh leases.

Restore does not silently run cancelled work. The response distinguishes retained
completed outputs, queued work needing a fresh plan, and replay requirements.
Checkpoints contain references rather than copied model tensors. If the host
restarts without the referenced data, replay is required; no durable cache store
or transparent crash migration is claimed.
