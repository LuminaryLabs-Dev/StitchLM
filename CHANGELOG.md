# Changelog

## 0.0.1 — development baseline

- Added a Rust workspace with four flat native/web apps and seven shared crates.
- Added real LFM native and WASM block execution, HTTP coordination and browser UI.
- Added bounded pairing, per-device leases, TLS options, state validation and recovery.
- Added pinned model fetches, native/web parity and constrained four-process tests.
- Added safe GGUF inspection and a real-weight Bonsai projection-splitting probe.
- Preserved the repository-local builder kit and its independent acceptance gates.

## Full pinned Bonsai text extension

- Added the 64-layer Qwen3.5 text graph, local streamed tensor provider and pinned tokenizer.
- Added stateless native/WASM matrix participation during complete generation.
- Added separate matrix-shard telemetry, request reset and cancellation checks.
- Added full-model reference fixtures and native/browser generation tests.

Arbitrary GGUF/Bonsai2 models, automatic optimal scheduling, long-context quality
and physical heterogeneous-device acceptance remain outside the validated scope.

## Scenario audit package — 2026-11-01

- Added the root `auditor/` package: pinned NexusAuditor Python entry point,
  local Node/browser adapters, strict JSONL schema and 16 user-like journeys.
- Scenarios own delays, overlap, cancellation, reconnection and injected request
  faults. No separate LANLint module or changes to the shared NexusAuditor repo.
- Validated all 16 real-model scenarios from a clean checkout, including exact
  pinned Bonsai recovery, four-native row splitting and mixed native/browser
  column splitting. Added 42 harness contracts with three consecutive passes.
- Preserved 42 passing Rust tests and 36 native-loaded builder-kit tests; kept
  inference Rust/WASM unchanged. Updated kit validation guidance and setup docs.
- Added explicit stream deadlines, bounded artifact locations, owned-resource
  cleanup and sanitized evidence. See `auditor/evidence/acceptance-summary.md`.
- Evidence remains sandbox/application-level only; physical LAN, production
  uptime, device certification and throughput improvement are not claimed.
