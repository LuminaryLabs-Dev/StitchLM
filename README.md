# StitchLM

**StitchLM** is a planned heterogeneous distributed LLM inference system: phones, laptops, desktops, and servers should be able to advertise their available capacity and contribute appropriate parts of a **single model's inference**. Contributions may include model-weight placement, transformer-layer execution, attention-head groups, MLP partitions, KV-cache hosting with local attention, and partial tensor reductions.

> **Status (2026-10-31):** architecture and documentation only. This repository does not yet contain an inference runtime, benchmarks, an integrated vLLM backend, distributed workers, tests, or validated throughput measurements.

## Intent

Build a capability-aware inference fabric: rather than requiring each worker to host an entire model or perform an identical role, partition a model's executable dependency graph and schedule useful operations on compatible workers. The central coordinator must preserve exact mathematical dependencies, numerical compatibility and cache consistency. Devices are not guaranteed work when their network/compute overhead would slow inference.

## Proposed architecture

1. **Control plane / vLLM integration** — use vLLM's request scheduling, batching and KV-management ideas as a foundation; adding arbitrary cross-device tensor-operation scheduling requires new executor, planner and transport code. Integration is not yet implemented.
2. **Capacity agent** — measure usable RAM/VRAM, available storage, supported arithmetic and kernels, compute throughput, bandwidth, latency, thermal and battery constraints; periodically re-advertise capacity.
3. **Partition planner** — select complete layer shards, Q/K/V and output projections, attention-head or KV-head groups, prefill and decode partitions, MLP gate/up/down slices, expert partitions (MoE only), and reduction/merge points.
4. **Worker runtime** — execute assigned operations on CPU/GPU where supported, cache relevant weights locally, maintain assigned KV blocks, and return versioned typed outputs.
5. **Transport and collectives** — authenticated device discovery, secure model and activation transfer, reduction, gather, synchronization, cancellation, backpressure and failure recovery.
6. **Placement policy** — optimize realized latency and throughput rather than raw FLOPS; tolerate device churn, never assume a phone's contribution is beneficial, and allow idle or cache-only roles.
7. **Validation and benchmarking** — deterministic operation tests, single-host numerical comparisons, distributed output equivalence, token correctness, per-device contribution and speedups/regressions.

## Transformer inference decomposition

For every layer, the current hidden state feeds normalization, Q/K/V projections, position encoding, attention against the relevant key/value cache, output projection and a residual update; then normalization, feed-forward projections, gating/nonlinearity, down projection and a second residual update. All workers must respect this data dependency order.

Attention may distribute query heads and/or context/KV partitions. Context-partitioned attention outputs require a mathematically correct global softmax merge (running maximum, normalization sums and weighted values), **not** averaging worker outputs. MLP neuron slices can execute in parallel and sum their down-projection contributions. Each device should keep weights near its compute kernels; migrating full weights on every generated token is not viable.

## Goals and limits

- **Goal:** allow ~50 heterogeneous devices to participate in one model's execution, each in a useful, measurable capacity.
- **Not a claim:** 50 workers imply neither 50x speed nor 50 simultaneously generated tokens for one conversation. Sequential token dependencies, network transport, collectives and KV ownership constrain throughput.
- **Measure separately:** single-stream tokens/s, aggregate tokens/s across requests, first-token latency, peak/working memory, model quality/numerical drift, device energy, and network bytes/token.
- **Initial target:** a reproducible single-host baseline, then a two-worker correct forward pass, followed by adaptive scheduling and broader device support.
- **Backends:** vLLM is the preferred orchestration starting point; llama.cpp/GGML and Ollama are possible later adapters, not proven interchangeable tensor executors.

## Proposed first milestones

1. Record model architecture, supported hardware and a reference single-device inference run.
2. Implement capacity probe plus authenticated device registration.
3. Implement one remote operation with typed tensors and deterministic merge against baseline.
4. Execute a complete forward pass across two workers with correctly owned KV caches.
5. Add latency-aware placements, recovery and heterogeneous worker types.
6. Expand to a multi-device benchmark; only then evaluate scaling toward 50 participants.

Each milestone requires measurable acceptance evidence, not only source-code presence.

## Repository organization (proposed, not yet created)

- `docs/`: architecture, protocols, benchmarking and decisions
- `coordinator/`: resource manager and scheduling extensions
- `workers/`: portable native and device-specific executors
- `protocol/`: tensor and control-message contracts
- `tests/`: reference correctness and integration coverage
- `.agent/`: agent context, bounded task tracking and handoff notes

## Governance

Follow the [Luminary Framework](https://docs.google.com/document/d/1lZLgr2A99EBzMwoLl4iEY7NiFVFG6LjU8snI2eP3ZWI/edit), [Upkeep Guide](https://docs.google.com/document/d/1nIacfvsNnmFg0kxZxs-tU1XF7w7d97HcfN3JEBOCGpw/edit), and the [live Upkeep register](https://docs.google.com/spreadsheets/d/11viRvCFI9qTmsuEYfv9lGBI8edaZFRJNE9b7OQDVz-0/edit). Maintenance reference: **MNT-413**. The repository contains technical source and documentation; project/ticket state remains in its authoritative Luminary tracker. Do not infer approval for dependency, code, deployment, or release changes from this document.
