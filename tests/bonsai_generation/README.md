# Full pinned Bonsai acceptance

These tests execute the entire 64-layer text graph, not a synthetic model or a
single matrix. Run connector tests sequentially, never exceeding four live
connectors. Python browser tests require Playwright and an official compatible
headless browser. Native tests use Linux `prlimit`; that is not portable OS proof.

1. Build the workspace and web apps. Fetch the pinned Bonsai GGUF/tokenizer.
2. Use `cargo run --release -p stitch-model --example bonsai_generate -- MODEL
   TOKENIZER PROMPT STEPS OUTPUT` for per-position logits and greedy continuation.
3. Compare against the official Prism legacy reference pinned in the runtime
   documentation. The reference must use the same tokenizer input IDs and F32 KV.
   Reference source/binaries are test dependencies, never production backends.
4. Run `run.py --gguf MODEL --tokenizer TOKENIZER --browser BROWSER --native-count
   4 --axis rows --steps 2 --output OUTPUT` for four native workers. Repeat with
   `--native-count 2 --axis columns` for two native and two actual browser workers.
   The raw `Hello` fixture must yield IDs 11,353, with nonzero remote matrix work.
5. Run `local_console.py --gguf MODEL --tokenizer TOKENIZER --browser BROWSER
   --reference-tokens REFERENCE_GENERATED_IDS --output OUTPUT` for the normal
   `Hi` chat-template path, cancellation and a fresh replay. The reference tokens
   must come from that exact serialized template and two greedy output steps.

The independent backend uses quantized activation dot products; Rust uses F32.
The bounded reference gate is exact next-token agreement on named short fixtures,
logit cosine >=0.999 and normalized RMSE <=0.02. It is not universal agreement or
long-context quality proof. The main fixture set also includes `2+2=` and
`The capital of France is`. Per-operation local/distributed comparisons retain
their existing absolute/relative tolerance; whole-model output is checked too.

All launch logs contain ephemeral host/pairing codes and must stay private. The
safe public reports include exact source/model pins, commands, measurements,
passed/failed cases and limitations. Do not commit model weights or raw state.

## Independent reference harness

`reference.cpp` is a small test executable against the external official Prism
library. It is not linked into any product crate or WASM binary. Build that pinned
library with CMake (`GGML_NATIVE=OFF`, `GGML_OPENMP=OFF`, `LLAMA_OPENSSL=OFF`, tests,
tools, examples and common disabled), then link this harness against `llama`,
`ggml-base` and `ggml`, with its include directories and library search path.
Invoke it as `reference-run MODEL PROMPT STEPS OUTPUT`. It records tokenizer IDs,
per-position logits and greedy continuation IDs using two CPU threads and F32 KV.
`STITCH_TRACE=1` optionally records intermediate tensors for diagnosis; keep these
raw files outside Git. `compare_reference.py REFERENCE CANDIDATE --output REPORT`
uses NumPy, verifies tokenization and continuation, and enforces the numerical gate.

Add `--cancel-check` to a mixed run to force the weight-fetch cancellation race.
The harness pauses a real browser request long enough to cancel the host, resumes
that request, records the actual stale-work409response, checks native processes
and browser workers remain healthy, then requires fresh complete generation.
No model weights, activations or result packets are mocked by this test.
