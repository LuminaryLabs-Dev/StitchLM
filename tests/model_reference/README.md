# Model reference

The pinned LFM reference is `crates/stitch-model/examples/baseline.rs`. The
multi-step split comparison is `crates/stitch-worker/examples/parity.rs`.
`tests/native_wasm_parity/compare.cjs` executes the generated WASM worker against
that actual-weight corpus. Keep model files and raw activation dumps out of Git.
