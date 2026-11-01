Current state: read the Rust/WASM and shard-execution phase at the end of this file. Earlier documentation-only sections are historical.

# StitchLM — Agent Memory

Updated: 2026-10-31
Repository: https://github.com/LuminaryLabs-Dev/StitchLM
Maintenance register: Upkeep / MNT-413
Baseline prior to documentation: `7767a714fae44852bb7cc2fd56d79388f236fb78` on `main`
Source evidence: original repository root held only a 10-character `README.md` containing `# StitchLM`.

## Objective

Develop heterogeneous distributed inference: many phones, PCs and other devices measure available compute, RAM, storage, bandwidth, latency and thermal headroom and offer only the useful share they can provide to the execution of one LLM. Contributing tasks may differ by device. The desired scale is approximately 50 participating devices, not an assumed performance guarantee.

## Decisions / direction

- Project name: StitchLM.
- Preferred scheduling foundation: vLLM; custom layer/tensor execution orchestration is required.
- Dynamic resource contributions: layer and weight placement, attention-head/KV-head work, QKV and output projections, MLP shards, local KV attention, correct global reductions, MoE experts when supported, speculative work where independently useful.
- An explicit task dependency DAG and heterogeneous placement policy are required; minimal transfers and correctness dominate participation count.
- Contributing to a single decoding stream is distinct from concurrently processing many independent requests.

## Verified state versus planned capabilities

CONFIRMED before this documentation phase: public GitHub repo in LuminaryLabs-Dev, default branch main, only stub README, no confirmed implementation.
PLANNED, NOT IMPLEMENTED: vLLM adapter, coordinator, worker protocol, mobile clients, capacity probes, remote compute, distributed attention, KV migration, numerical tests, throughput benchmarks, releases and deployment.

## Key technical uncertainties

1. Which vLLM runtime/version and executor interfaces to extend, particularly when workers lack supported GPUs?
2. How to implement portable CPU/mobile workers without forcing vLLM's GPU assumptions onto them.
3. Tensor-transfer formats and activation-bandwidth economics for real phone/Wi-Fi networks.
4. Correct numerical merging for attention and MLP partitions with mixed dtypes/quantization.
5. KV-cache ownership, speculative decoding benefits, failure recovery and security model.
6. Whether participating low-power devices improve single-stream tokens/s relative to baseline.

## Next bounded task

After documentation review and separate authorization: write a technical architecture specification and a minimal single-host/two-worker correctness experiment plan. Establish baseline metrics before claiming distributed speedups.

## Handoff rules

Keep `README.md`, `AGENTS.md` and this file consistent. Capture tested commands, commit IDs and observed numerical results. Do not invent implemented capabilities. Check the live Upkeep checkout before a repository write. No non-documentation code was authorized or produced by this phase.


## Development-kit phase — 2026-11-01

User separately requested a kit analogous to Arcade/Glass and push to main when ready. Baseline: `9e0ace8877c1095ebc942e8f1bbe2cd5817756c4`. MNT-413 was Needs Review with no active owner at preflight. Implemented repository-local StitchLM Builder Kit: portable/compatibility manifests, seven explicit skills, three-stage resumable CLI, source/artifact integrity, locks/recovery, bounded iteration and conservative evidence gates. See plugin references/implementation-plan.md and references/validation.json for scope and actual checks. This changes the historical docs-only state to documentation plus development harness, NOT an inference implementation. No new vLLM backend, device work, numerical/model benchmarks, model session, credentials/downloads, deployment, tags/releases or tracker changes.

Next: select one separately authorized build-ready experiment/outcome from canonical tasking; single-host baseline then two-worker correctness precedes scale. Missing model/hardware is a blocker. Future run state belongs outside tracked source and must preserve full intent/pins.


## Rust/WASM and data-driven shard phase — 2026-11-01

The user explicitly authorized direct sandbox implementation, actual Rust crates,
model downloads/tests and main delivery through the GitHub connector. No AI build
workers were used. The separate daily check-in remained read-only. Product
engineering authority is the direct approved scope; Upkeep MNT-413 still retains
its older documentation-review state and no active checkout was observed.

The repository contains four flat app wrappers, seven shared Rust crates, native
and WASM builds, the installed development kit, pinned model fetches and scoped
validation evidence. LFM2.5-350M runs through native and browser connectors. The
new shard path uses immutable definitions, execution records and measured device
profiles; actual Bonsai projection rows and reduction-axis chunks run through
bounded multi-round command buffers on native and browser workers. Checkpoints
retain references and invalidate old assignments on restore. Missing retained
state requires replay. The final candidate is gated by software, numerical and
actual model/browser checks; see evidence/v0.0.1 and docs/validation.md.

A final HTTP checkpoint test exposed floating-point round-trip drift after
measured placement costs were added. Lossless serde_json float parsing and a
specific sealed-checkpoint regression test address it. Do not remove that feature
or assume a checksum over parsed floating metadata survives a lossy parser.

Boundaries at the initial delivery: complete Bonsai/Qwen3.5 generation and arbitrary GGUF backends
are not implemented. Attention merge and state-ownership rules have scoped unit
tests, not arbitrary cache export/migration proof. The projection interface uses
capacity calibration with measured cost ranking, not a globally optimal automatic
scheduler. Native tests enforce a 1 GB address-space ceiling; browser process RAM
is not hard-capped per worker. CPU pacing is cooperative. Physical Mac, Windows,
mobile, TV and heterogeneous LAN acceptance remain unverified. No release tag,
deployment, public model-weight upload, outreach or tracker closure is implied.

The kit's original engineering introduction records the earlier vLLM design
context. The approved Rust scope supersedes that preference; its integrity,
resource, numerical and evidence safeguards remain applicable. Installation and
local validation-driver execution do not claim a fresh AI model-session invocation.

Continuation: preserve the tested baseline and its evidence, validate a real
second device over trusted HTTPS, and implement further model/kernel support only
with independent full-output reference checks. Reconcile the stale canonical Drive
handoff through an authorized record update; do not fabricate a company LUM ID.


## Full pinned Bonsai text phase — 2026-11-01

The user separately authorized the full model engine, sandbox tests and main
push. The new text graph implements all 64 layers of the exact pinned legacy
Ternary Bonsai 27B Abliterated artifact, not Bonsai2. It streams local matrix
weights and keeps KV/convolution/GatedDeltaNet state on the host. Native and
browser connectors execute stateless matrix shards. LFM retains its separate
contiguous-block path. No AI implementation workers were used.

Reference validation uses the official frozen Prism legacy commit
b06f74a9ea9fa5eaf751f7d575bf766486b4ae62, with matching tokenization and F32 KV.
Rust arithmetic and the reference's quantized activation dot products differ;
use the explicit numerical gate and exact named-fixture continuations, not a
bitwise-logit claim. See current full-model evidence for final run outcomes.

This is a correctness and low-memory capacity baseline, not an interactive speed
claim. The distributed path rereads/transfers most matrix weights each token and
retains a host reference check. Native CPU pacing now uses measured kernel time
rather than multiplying network wait time. Browser RAM is not hard-capped.
Physical devices/LAN, long-context quality, vision, Bonsai2, arbitrary GGUFs and
durable full-model cache migration remain outside the verified scope. No tags,
releases, deployments or Drive/tracker edits are implied by source delivery.
