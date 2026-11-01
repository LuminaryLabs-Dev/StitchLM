# Third-party notices

The portable LFM implementation in `crates/stitch-model/src/lfm2.rs` is adapted
from Hugging Face Candle commit `2cf0e66dcb2953e2aa7f4000cdd697239e9c16c5`.
Candle is MIT OR Apache-2.0; both original license texts are retained in
`third-party/`. Changes expose contiguous block execution, preserve cache state,
and derive the adjusted feed-forward dimension from validated tensor shapes.

Other Rust dependencies and exact versions are recorded in Cargo.lock. Their
individual licenses continue to apply. Generated wasm-bindgen JavaScript is
build output, not a separate inference implementation.

The optional downloaded model is LiquidAI/LFM2.5-350M, pinned to revision
`9e6c6ccf47cd318696e137d381a7ded8fe4df09f`. Model weights are not included in Git.
The upstream model uses the LFM Open License v1.0, including its commercial
revenue condition. Read the downloaded LICENSE and the upstream license before
using or redistributing weights. Code licensing does not relicense the model.

NexusEngine contract reference:
`LuminaryLabs-Dev/NexusEngine@7d2cb0398887d4d09569cb44d20b11febb58e826`.
The Rust adapter implements the inference JSON boundary. It does not embed or
claim to execute the JavaScript engine runtime.
