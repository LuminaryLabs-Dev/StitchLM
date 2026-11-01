# Nexus integration boundary

Reference engine commit:7d2cb0398887d4d09569cb44d20b11febb58e826.

stitch-nexus produces the canonical inference request/result JSON fields from
NexusEngine's inference contract. A reproducible test compares the Rust adapter
output to the actual pinned JavaScript contract. Missing input-token usage stays
unknown instead of being invented as zero. This is an interoperability boundary,
not a Rust port of NexusEngine or a claim that its Kits are executing here.

Native/WASM product logic stays in the shared Rust crates. The first app wrappers
use Cargo and wasm-bindgen directly. Wiring these builds into Nexus n:build remains
an explicit integration step; no Engine Core files were changed or new Core
responsibilities invented. The adapters preserve a clean place for later reuse.
