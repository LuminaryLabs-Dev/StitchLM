# StitchLM v0.0.1

Rust native host and native/browser connectors that cooperate on one model's
inference. The first verified backend runs **LiquidAI/LFM2.5-350M**. The host owns
model selection, embeddings, sampling and coordination; connectors execute real
contiguous decoder blocks through the same Rust core, compiled to native or WASM.

## Current evidence

- Native generation with checksum-pinned real model weights.
- Four independent native connector processes, each under a 1 GB virtual-address
  ceiling, matching local output tokens. Observed connector RSS approximately
  180–232 MB; low CPU duty target with measured averages and startup caveats.
- Native/WASM block activation parity, maximum observed error 3.1e-7.
- Real browser host console plus Web Worker connector generating the same answer.
- Duplicate-job idempotency, cancellation, removal and fresh-prefill recovery.

See [validation](docs/validation.md) for exact scope, commands and limitations.
These are sandbox tests, not physical Mac/Windows/mobile/TV or multi-device LAN
acceptance. No distributed speedup is promised. The host currently retains the
full small reference model; this is not yet host-memory offload.

## Bonsai and GGUF status

The new [shard contract](docs/shard-contract.md) connects immutable definitions,
execution records and measured device profiles to live native/WASM command buffers.
Row and column plans, reference comparison, live status and sealed checkpoints are
available in the host console.

The requested **Ternary-Bonsai-27B-Abliterated-LowDeg-Q2_0.gguf** has been downloaded
and checksum verified. Its actual packed projection was split across four native processes and a mixed
two-native/two-browser group, using multi-round row and column plans with exact
reference agreement in the recorded tests. Native processes had 1 GB address-space
limits; browser process memory was not hard-capped. That is an
operation-level proof, **not complete Bonsai text generation**.

The file uses Qwen3.5 hybrid attention and a legacy Prism group-128 encoding under
GGUF type 42. Modern upstream type 42 uses a different layout. This build inspects
GGUF safely and refuses unsupported full-model execution rather than producing
wrong text. It does **not** support every GGUF. See [compatibility](docs/gguf-compatibility.md).

## Run it

Follow [native quickstart](docs/native-quickstart.md), then
[web connector quickstart](docs/web-connector-quickstart.md).
For a host machine and other devices, use the complete [LAN quickstart](docs/lan-quickstart.md).

    cargo xtask fetch-model --directory .stitchlm/models/lfm2.5-350m
    cargo build --locked --release -p stitchlm-host -p stitchlm-connector
    cargo xtask build-web
    target/release/stitchlm-host --model-dir .stitchlm/models/lfm2.5-350m

Rust 1.99.1 and wasm-bindgen-cli 0.2.130 are pinned. Model downloads stay outside Git.
The host prints separate host-access and pairing codes; keep launch logs private.
LAN use requires a certificate trusted by participating devices. Do not bypass
certificate warnings. Model and third-party licenses remain applicable.

## Repository

- apps/host, apps/connector: native executable wrappers.
- apps/host-web, apps/connector-web: Rust/WASM web wrappers.
- crates/: shared protocol, model, worker, host, platform, Nexus bridge and UI.
- xtask/: model fetching, build and inspection tooling.
- docs/: architecture, scope, security, validation and Simon handoff.
- tests/ and evidence/v0.0.1/: reproducible tests and scoped measured results.
- .agent/plugins/stitchlm-builder-kit/: repository-local Codex development harness.

Before development, load the builder kit in the actual executor and read
[AGENTS.md](AGENTS.md). Kit installation, invocation, source checks and real-model
proof are separate evidence claims. No tags, release assets or deployment are
created merely by passing tests.
