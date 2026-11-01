# Model partitioning

The validated LFM path uses disjoint contiguous decoder block ranges. The host
embeds tokens, executes uncovered blocks, routes activation tensors through one
selected worker per assigned range, then applies final normalization/output head
and greedy sampling. Splitting layers preserves the exact operation order.

For this small 16-layer model, seven two-block ranges can be assigned and the host
keeps the final two blocks. Additional registered devices are replicas rather
than duplicate operations in the same request. The constrained sandbox test uses
up to four native connectors. Selected participants remain fixed for a request so
KV/convolution state is not silently moved. Recovery starts a new full prefill.

The separate Bonsai projection probe partitions output rows and concatenates
results. A reduction-axis partition would instead require summing partial dot
products with a defined accumulation order/tolerance. Attention requires its own
normalization-aware reductions; recurrent updates require ordered state ownership.
No generic key-value mapping can replace these mathematical contracts.

The data-driven operation path now separates immutable definitions, execution
records and observed device capabilities. It supports bounded row and column
projection chunks, including working buffers smaller than a complete layer.
Runtime/process memory remains separate, and unsupported model operations still
require a validated implementation rather than merely a descriptor.
