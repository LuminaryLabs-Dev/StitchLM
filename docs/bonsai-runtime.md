# Pinned Bonsai text runtime

The Rust backend executes all 64 layers of the exact
`Hikari07jp/Ternary-Bonsai-27B-Abliterated-LowDeg-GGUF` artifact pinned in
`models/manifests/bonsai-27b-abliterated.json`. This is the older Ternary Bonsai
27B Abliterated model, **not Bonsai 2**. Other GGUF encodings and architectures are
not automatically supported.

## Run

From the repository root, after the native/web build prerequisites in the main
quickstart:

    cargo xtask fetch-bonsai --directory .stitchlm/models/bonsai
    cargo build --locked --release -p stitchlm-host -p stitchlm-connector
    cargo xtask build-web
    target/release/stitchlm-host --model-file .stitchlm/models/bonsai/Ternary-Bonsai-27B-Abliterated-LowDeg-Q2_0.gguf --tokenizer .stitchlm/models/bonsai/tokenizer.json --context 128

The fetch command verifies both the model and the separately pinned official
Qwen tokenizer. No weights are included in Git. The two inputs are checked against
repository-pinned hashes at startup, not hashes supplied by an arbitrary manifest.

Open the host console and pair connectors normally. A Bonsai connector receives
an empty initial assignment, then authenticated bounded matrix commands. It does
not download a contiguous LFM block or keep Bonsai KV/recurrent state. Native and
browser connectors run the same Rust packed-weight kernel.

For the smallest smoke test, select **Raw completion (testing)**, enter `Hello`,
and request two tokens. Chat template mode is the normal conversational path.
The raw fixture's expected continuation for this exact model is `, I`; that is a
correctness fixture, not a useful conversational answer. CPU runs can take tens
of seconds or minutes per token. Cancellation is checked between operations and
while distributed commands are pending; a running local matrix operation must
finish before that cancellation is observed.

For another device, follow [LAN setup](lan-quickstart.md), using the same Bonsai
model/tokenizer flags with the trusted HTTPS certificate/key flags. No physical
LAN or specific Mac/Windows/mobile device is certified by sandbox results.

## Responsibilities

- The host streams packed tensor slices from the GGUF and owns tokenization,
  embeddings, all normalization/nonlinear operations, Qwen3.5 partial RoPE,
  attention KV, gated-delta recurrent state, convolution history and sampling.
- The local path reads bounded matrix chunks rather than expanding all 27B
  parameters to floating-point RAM.
- The distributed path splits each matrix into row or quantization-aligned column
  commands. Row outputs concatenate; column contributions reduce. It assembles
  the complete operation before advancing the model graph.
- The host currently materializes one packed matrix for distributed planning and
  reference checking. Its peak RAM is therefore larger than the local streaming
  buffer. Connector memory and host memory are distinct budgets.
- Requests own fresh state. A failed or cancelled token discards the request's
  partial state. The next request starts with a fresh prompt replay; it does not
  reuse possibly corrupted recurrent/KV data.

## Controls and boundaries

Generation API fields include `raw_prompt`, `split_axis` (`rows` or `columns`),
`preferred_chunk`, `connector_count` (up to four) and `working_memory_bytes`.
Defaults are row chunks of 1024 or column chunks of 1280, a 32 MB per-command
working budget, and up to four ready compatible connectors. `local_only` runs on
the host. `remote_shards` counts completed matrix commands; `remote_blocks` remains
reserved for the older LFM block protocol. A local fallback is not remote proof.

The tokenizer and graph are text-only, greedy decoding, bounded to 512 tokens
maximum including prompt and output. The default CLI context is 256; smaller
explicit contexts are useful for initial tests. This is not validation of the
model's advertised 262K context, vision, tool calling, speculative decoding,
arbitrary KV migration or crash-resumable full-model state.

At small working budgets, the full vocabulary matrix cannot be partitioned only
by columns because even one aligned column group and its full output must fit.
Use row splitting or a larger working budget. Invalid requests fail explicitly.
Admission budgets do not impose OS memory caps. The tests use a separate native
address-space cap; browser process memory is not hard-capped per worker.

## Performance and evidence

This is a correctness/capacity baseline. It rereads and retransmits substantial
model weights each token, keeps the correctness reference enabled for remote
matrix operations, and uses conservative connector CPU duty. It can be slower
than local execution, especially on Wi-Fi. Do not claim a speedup or low network
traffic. Persistent weight caches and efficient batched prefill are future work.

The independent comparison uses the official frozen Prism legacy line, commit
`b06f74a9ea9fa5eaf751f7d575bf766486b4ae62`. That implementation uses quantized
activation dot products, while this Rust path uses F32 arithmetic. Full logits
are therefore compared with explicit cosine/normalized-error tolerances, not
claimed bitwise equal. See the final evidence receipt for actual passed fixtures,
full native/browser runs and remaining gaps. Matrix-only tests remain separate.
