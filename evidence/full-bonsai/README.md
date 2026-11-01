# Full pinned Bonsai validation

This is the current full-model extension receipt. The older `../v0.0.1` files
record the earlier LFM and projection-only delivery; do not confuse the phases.

- [Receipt and exact source hashes](validation-receipt.json)
- [Independent full-logit/token fixtures](reference-fixtures.json)
- [Four-native complete generation](native-generation.json)
- [Native/browser complete column-split generation](mixed-generation.json)
- [Ordinary chat and fresh state after cancel](normal-chat.json)
- [Forced stale-weight cancellation and full replay](cancel-and-replay.json)
- [Preserved LFM, WASM and projection regressions](regressions.json)

The initial native full run used earlier wall-time pacing; the later mixed run
used kernel-time pacing. The final recovery run uses the final protocol fixes and
observes real409responses before the same connectors finish a complete request.
Independent arithmetic fixtures and normal chat were captured before additive
state/transport guards; the arithmetic/template stayed unchanged. The receipt
records this scope rather than pretending every earlier capture used one binary.

This is a working correctness/capacity prototype. It is slow, text-only and
bounded. Physical LAN and device acceptance remain open. The device-list block
range/job fields are LFM-specific; use the generation's remote matrix shard count
and matrix execution status for Bonsai. No released binaries or model weights
are included by this source delivery.
