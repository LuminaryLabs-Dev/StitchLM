# Protocol v1

Every assignment pins model identity, full-weight digest, configuration, contiguous
block range, shard digest/size and context limit. A random per-connector lease
scopes access to that connector. Host access and pairing credentials are separate.

POST /api/register pairs a native or web connector. Its bounded assignment points
to GET /api/connectors/{id}/weights. After hash checking and model initialization,
POST /api/connectors/{id}/ready enables selection. GET .../job polls a bounded job;
POST .../result returns its activation output. Host-only routes expose status,
generation, cancellation and connector removal. Generation streams NDJSON events.

A job identifies protocol, request, job, full-model digest, range, token position,
activation shape and values. Results must match those fields and preserve shape.
Finite-value and context checks run before the output is accepted. Identical job
retries are idempotent; reused IDs with changed inputs fail. Missing prefill or
out-of-order positions fail. On cancellation or failure, stale pending jobs are
cleared and participating connectors become not-ready until explicitly restored.
A new request resets both attention KV and convolution state.

The complete LFM path assigns contiguous decoder blocks. The additional generic
shard path uses immutable definitions, bounded commands and scoped weight fetches
on the same registered connectors. It currently executes dense F32 and verified
legacy Prism group128 projections with row/column assembly. Read shard-contract.md
for live status, checkpoint APIs, supported semantics and remaining limits.
