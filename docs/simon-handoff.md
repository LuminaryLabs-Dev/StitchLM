# Simon handoff

## What you can try

Build the Rust host, native connector and WASM apps using the quickstarts. Start
the host with the verified LFM model, open its host console, enter its host-access
code, and pair a native or browser connector using the separate pairing code.
Generate a small answer and inspect real remote-block execution counts.

For the data-driven shard path, start the host with `--gguf-resource BONSAI_GGUF`.
The host verifies that exact file and registers its FFN gate projection. In the
console's shard section, choose the small deterministic matrix or Bonsai matrix,
select rows or columns, choose up to four ready connectors and a working-memory
budget. Inspect the command records, measured/reference costs and checkpoint.
This is a projection test, not a Bonsai chat endpoint.

## What remains

- Validate your actual native OS and a second physical device on trusted HTTPS.
- Complete and independently verify the Qwen3.5 hybrid attention/GatedDeltaNet
  backend before enabling full Bonsai text generation.
- Extend supported kernels operation by operation; never treat every GGUF or
  arbitrary model state as automatically shardable.
- Treat the capacity-calibration scheduler as measured experimentation, not an
  automatic promise that every extra device accelerates generation.
- Implement durable state storage/backend restoration before claiming transparent
  crash recovery across hosts or migrating a live model cache.

The current Drive handoff still describes the earlier documentation/vLLM plan;
this repository and its scoped validation evidence describe the implemented Rust
baseline. Reconcile the canonical Drive handoff in an authorized record update.
No release tag, public deployment or physical-device acceptance is implied by a
main-branch commit.

See [LAN quickstart](lan-quickstart.md) for host setup, trusted certificates,
native `--ca-cert`, browser joining and the physical-device acceptance checklist.
