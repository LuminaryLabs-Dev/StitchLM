# Architecture

- apps/host: native model owner, HTTP/TLS coordinator and static web host.
- apps/connector: native wrapper around shared worker execution.
- apps/host-web: Rust/WASM control console served by the native host.
- apps/connector-web: Rust/WASM worker in a dedicated browser Web Worker.
- crates/stitch-core: bounded identity, assignment, activation and job contracts.
- crates/stitch-model: adapted Candle LFM kernels, verified dimensions and shards.
- crates/stitch-worker: one assignment's actual model blocks and session caches.
- crates/stitch-host: local reference and distributed sequential block pipeline.
- crates/stitch-platform: same-origin browser network adapter.
- crates/stitch-ui: shared safe text/DOM helpers.
- crates/stitch-nexus: narrow pinned Nexus inference JSON adapter.
- xtask: reproducible model fetch, web build and validation commands.

Host embeds prompt tokens. Each selected connector transforms its assigned
activation sequence through contiguous decoder blocks. Host executes gaps and
remaining blocks, applies the output head, selects the next token, then repeats.
Attention KV and convolution history remain owned by the same worker for the
request. Every job carries model digest, range, request, position and job identity.

Duplicate identical jobs return cached results without advancing state twice.
Changed input under the same job ID, stale results, wrong ranges and non-finite
activations are rejected. A new request starts at position zero and clears both
cache types. Admission checks happen before constructing model tensors.

The initial transport is bounded HTTP polling and JSON float activations. This
prioritizes inspectable correctness over bandwidth. Binary transport, dynamic
partitioning, true low-memory tiling and pipelined multi-request throughput are
future separately validated changes, not hidden claims in this implementation.
