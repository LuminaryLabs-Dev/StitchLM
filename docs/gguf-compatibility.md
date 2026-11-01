# GGUF compatibility and Bonsai findings

GGUF is a container, not a universal inference algorithm. Correct execution also
requires the model architecture, tensor layout, quantization codec, transforms,
tokenizer and cache semantics. Unknown combinations must be rejected clearly.

The bounded inspector supports GGUF v2/v3 metadata and tensor descriptions. It
checks magic/version, item counts, string/header budgets, dimensions, integer
overflow, alignment, duplicate names and file-relative spans. It does not load
all weight tensors or mark an unknown architecture executable. Inspection is not
a full hostile-input audit or proof that tensor contents are semantically valid.

## Exact requested model

- Hikari07jp/Ternary-Bonsai-27B-Abliterated-LowDeg-GGUF.
- Revision:60b1880c4c2e30711e1c90867a057f71ab45d587.
- File:Ternary-Bonsai-27B-Abliterated-LowDeg-Q2_0.gguf.
- Bytes:7,165,121,600.
- SHA256:527f276ddf047b3494de964985b5529b9fc3ecf739ea64d8fab3a6ef8403e81d.
- Actual architecture:qwen35,64blocks,hidden width5120,FFN width17408.
- Actual types:498legacy ternary type42tensors and353F32tensors.
- Packed block storage is approximately99–102MB per decoder block.

This legacy Prism artifact stores128weights per34-byte quantization group under
type42. Current upstream type42uses64weights per18-byte group; current Prism uses
type142for the128group codec. Blindly selecting a decoder by type number can
therefore produce incorrect data. The test checks the exact artifact and layout;
it does not silently reinterpret all type42files as this legacy format.

## Proven operation-level split

The actual blk.0.ffn_gate.weight projection,5120input values and17408output rows,
was streamed using a1360-byte weight-row buffer. Four independent processes each
computed4352rows under a 1 GBvirtual-address limit. Concatenated results exactly
matched the unsplit projection. This supports the feasibility of bounded weight
streaming and row partitioning. Input/output buffers, metadata and process runtime
consume additional memory;1360bytes is not total process RAM.

## Still unsupported

Complete Bonsai text generation is not implemented by this release. It requires a
validated Qwen3.5 hybrid attention/GatedDeltaNet execution path, tokenizer/template
verification, cache lifecycle, full-output reference parity and integration with
the connector scheduler. The projection test does not establish those claims.
The existing host therefore refuses this GGUF as a full inference backend.

    cargo xtask fetch-bonsai --directory .stitchlm/models/bonsai
    cargo xtask inspect-gguf /path/to/Ternary-Bonsai-27B-Abliterated-LowDeg-Q2_0.gguf

Model source:https://huggingface.co/Hikari07jp/Ternary-Bonsai-27B-Abliterated-LowDeg-GGUF
Codec reference:https://github.com/PrismML-Eng/llama.cpp/tree/5628c7338963b00a598f2a588fd09a79af6d70db
