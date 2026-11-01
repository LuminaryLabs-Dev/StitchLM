# Recovery checks

`tests/distributed_inference/run.py` checks real process removal, fresh prefill,
cancellation and another successful request. Core shard tests cover expired leases,
retry generations, duplicate/stale results, resource ownership, sealed checkpoints,
missing retained outputs and state compare-and-swap. Restoring an opaque pointer
without its underlying data is deliberately not treated as recovered inference.
