# Validation and evidence

The evidence is scoped to this Linux sandbox and the exact pinned model files.
Source, software, numerical, browser, resource and physical-device claims remain
separate. The final validation receipt lists the tested candidate hashes.

## Reproduce

    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace --locked
    cargo build --locked --release -p stitchlm-host -p stitchlm-connector
    cargo xtask build-web

The preserved development kit has its own Node acceptance suite. Tests requiring
an actual installed/enabled plugin need STITCHLM_TEST_INSTALLATION pointing to a
fresh verified installation receipt. A fixture-only run cannot prove native load.

Real-model tests require the model fetched by `cargo xtask fetch-model`:

    cargo run --release -p stitch-worker --example parity -- MODEL_DIR EVIDENCE_DIR
    python tests/distributed_inference/run.py --model-dir MODEL_DIR --output EVIDENCE_DIR
    python tests/browser_connector/run.py --model-dir MODEL_DIR --output EVIDENCE_DIR --browser CHROME_HEADLESS_SHELL

The shard test also needs the exact Bonsai GGUF fetched by `cargo xtask fetch-bonsai`:

    python tests/shard_execution/run.py --model-dir MODEL_DIR --gguf BONSAI_GGUF --browser CHROME_HEADLESS_SHELL --native-count 4 --output EVIDENCE_DIR
    python tests/shard_execution/run.py --model-dir MODEL_DIR --gguf BONSAI_GGUF --browser CHROME_HEADLESS_SHELL --native-count 2 --output EVIDENCE_DIR

Run those tests sequentially so no more than four connectors are active. The
mixed test starts two native processes and two real browser Web Workers. It tests
both output-row concatenation and reduction-axis summation, bounded multi-round
queues, reference agreement, checkpoint restore and tamper rejection.

## Resource interpretation

The native tests use `prlimit --as=1000000000`, a hard virtual-address-space limit
per connector. RSS and virtual size are sampled separately. The CPU duty setting
is cooperative pacing, with short bursts, and is not a cgroup percentage quota.
The host is not subject to the connector's 1 GB limit. Browser process memory is
not hard-capped per worker; do not describe the native limit as a browser guarantee.

Per-command working buffers and runtime overhead are separate. In the real
Bonsai tests, chunk budgets are approximately 2.9–9.7 MB, while each connector's
whole process/runtime uses more. The old 20-connector/10-MB-total test was replaced
by the user's four-connector/1-GB instruction and is not an acceptance target.

## Evidence boundaries

LFM native/WASM parity, real browser generation and Bonsai projection execution
are actual-weight tests. Attention merge, malformed inputs, leases, state CAS,
backpressure and recovery transitions also use intentionally synthetic unit cases.
Neither synthetic tests nor a matrix projection establish full Bonsai generation.

Physical Mac/Windows/mobile/TV devices, heterogeneous Wi-Fi, certificate deployment
and long-running production behavior remain unverified. Initial browser launch
failed with the system Chromium; official Chrome for Testing headless shell ran
successfully without using an external preview site. No raw.githack preview was used.

Generated artifacts, model weights, private launch logs and active access codes
are excluded from Git. Keep full local reports private; publish only scoped test
summaries and approved non-sensitive captures.

A sealed-checkpoint regression covers measured floating-point costs crossing JSON
interfaces. The workspace enables serde_json float_roundtrip so the parsed record
retains the values used to compute its integrity digest.

The final lifecycle tests cancel an active request, keep the existing native or
browser connector alive, wait for cache reset/readiness, and require nonzero
remote-block execution on the next generation. A host-local fallback alone does
not satisfy this recovery check.
