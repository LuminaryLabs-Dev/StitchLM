# Architecture

- `apps/host`: native model owner, HTTP/TLS coordinator and static web host.
- `apps/connector`: native wrapper around shared worker execution.
- `apps/host-web`: Rust/WASM control console served by the native host.
- `apps/connector-web`: Rust/WASM worker in a dedicated browser Web Worker.
- `crates/stitch-core`: identity, assignment, activation, shard and job contracts.
- `crates/stitch-model`: Candle LFM implementation, Bonsai text graph, packed
  kernels, bounded GGUF inspection and streamed tensor provider.
- `crates/stitch-worker`: stateful LFM blocks or stateless matrix execution.
- `crates/stitch-host`: model selection, request ownership, local/reference and
  distributed execution through the selected backend.
- `crates/stitch-platform`: same-origin browser network adapter.
- `crates/stitch-ui`: safe text/DOM helpers.
- `crates/stitch-nexus`: narrow pinned Nexus inference JSON adapter.
- `xtask`: pinned model/tokenizer fetch, web build and validation commands.

## LFM path

Host embeds prompt tokens. Each selected connector transforms its assigned
activation sequence through contiguous decoder blocks. Host executes gaps and
remaining blocks, applies the output head, selects the next token, then repeats.
Attention KV and convolution history stay on the same worker for the request.
Every job carries model digest, range, request, position and job identity.
Duplicate identical jobs return cached results without advancing state twice.

## Bonsai path

The native host executes the complete text graph and owns KV, convolution and
GatedDeltaNet recurrent state. The local path reads bounded packed matrix rows
from disk. The distributed path materializes one packed matrix, splits it into
bounded row or column commands, sends those to native/WASM workers, and assembles
the operation before continuing the graph. No connector owns the whole model or
its full state. The initial assignment is stateless and contains zero weights.

This reuses the same generic shard definitions, command leases, hashes, memory
admission, measured device profiles and result assembly as the diagnostic matrix
path. Nonlinear/stateful operations are still host-owned. Per-operation sealed
checkpoints do not constitute durable whole-model KV/recurrent checkpoints.

## Shared guarantees and limits

Changed inputs under the same identity, stale results, wrong assignments and
non-finite activations are rejected. New requests start with fresh state. A
failed/cancelled full Bonsai request discards partial model state and requires
prompt replay; it never silently resumes a partially mutated recurrent cache.
Admission checks precede worker kernel execution. Only one request runs at a time.

Transport is authenticated HTTP/TLS polling, binary weight slices and JSON float
activations/results. The design prioritizes inspectable correctness and bounded
working sets over throughput. The current Bonsai path rereads/transmits most
weights for each token and runs a host reference for remote matrix operations.
Binary activation transport, weight-cache placement, two-axis tiling and batched
prefill optimization need separate validation; none is an implied speed guarantee.
