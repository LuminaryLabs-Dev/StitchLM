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

## Prism legacy full-model reference

The Qwen3.5 text graph and legacy packed-weight layout were checked against the
MIT-licensed official PrismML llama.cpp legacy line at
`b06f74a9ea9fa5eaf751f7d575bf766486b4ae62`:
https://github.com/PrismML-Eng/llama.cpp/tree/b06f74a9ea9fa5eaf751f7d575bf766486b4ae62

Its license is retained in `third-party/prism-llama/LICENSE`. The C++ implementation
is an external validation reference, not a bundled production backend. StitchLM's
model graph, streaming provider and shared native/WASM kernels are Rust.

The separately downloaded tokenizer is pinned to official Qwen/Qwen3.5-27B
revision `fc05daec18b0a78c049392ed2e771dde82bdf654`. No tokenizer or model weights
are redistributed by this source repository. Their upstream licenses apply.
