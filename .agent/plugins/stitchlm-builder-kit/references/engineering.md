# StitchLM engineering contract

Source of intent: repository README, AGENTS.md and .agent/memory.md; preserve those distinctions. StitchLM aims to split **one LLM execution** over heterogeneous capacity, eventually around 50 devices. This is not 50 independent replicas, 50 tokens at once, or a 50x speed guarantee. vLLM is a preferred scheduling starting point only; no backend is integrated.

For each applicable experiment, define and test:
- Typed operation schema: shapes, dtypes, layouts, supported kernels/devices, ordering and dependency DAG; model/weight/tokenizer/version identity.
- Context-sharded attention: merge global softmax using maxima, normalization sums and weighted value sums. For shard maximum m_i, sum l_i and unnormalized weighted sum u_i, use m=max(m_i), L=sum(exp(m_i-m)*l_i), U=sum(exp(m_i-m)*u_i), output U/L. Never average normalized shard outputs. Cover masked/empty shards and numerical stability explicitly.
- MLP down-projection contribution sums; reduction order, reference dtype, absolute/relative tolerances and quantization effects. Do not equate approximate floating arithmetic with bitwise identity.
- KV ownership, lifecycle, request isolation, positions, migration, model/weight version, stale response rejection and consistency under retry/cancellation.
- Usable RAM/VRAM/storage, supported arithmetic, measured bandwidth/latency, thermal/battery conditions; placement may exclude harmful contributions. Keep weights near compute and measure transfer cost.
- Disconnect, timeout, retry/idempotency, cancellation, backpressure and failure consistency. Authentication and privacy for prompts, weights, activations and caches; no credentials in artifacts.

Progress from a reproducible single-host reference to a two-worker correct forward pass **before** scaling. Record model/config/software pins, seed, inputs, precision, workload, actual devices and environment, commands, raw output/errors and numerical comparisons. Benchmarks compare equal model, hardware budget, batch and workload: single-stream and aggregate tokens/s, first-token latency, memory, drift, energy and bytes/token. Missing resources or measurements are blockers. Synthetic fixtures are not inference or device evidence.
