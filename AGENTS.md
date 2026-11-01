# StitchLM Agent Instructions

Scope: `LuminaryLabs-Dev/StitchLM`. Read `README.md` and `.agent/memory.md` before starting. Follow the Luminary Framework, current Upkeep Guide, and the live Upkeep MNT-413 record for authority and concurrency. The repository is currently a documented concept, **not** a working inference engine.

## Operating contract

1. Read current repository tree, main head, relevant issues, task/Upkeep records and ownership before changing anything.
2. Define a single bounded task with acceptance criteria, allowed paths and exclusions. Obtain explicit authorization for the exact write; never assume documentation approval authorizes implementation.
3. Confirm exclusive repository maintenance checkout when working under Upkeep. Never override another session's In Progress claim.
4. Preserve clear boundaries: vLLM request scheduling vs. proposed finer-grained heterogeneous operation scheduling; control plane vs. data plane; confirmed implementation vs. planned capability.
5. Prefer reversible, inspectable changes. No secrets, credentials, model data without appropriate rights, external deployment, or unreviewed dependency upgrades.
6. Validate exact paths and content by readback. Only report builds, tests, benchmarks and device runs that actually happened, including commands, environment, failures and limitations.
7. Update `.agent/memory.md` with confirmed state, decisions, unresolved issues, exact evidence, next action and what was deliberately not changed. Keep historical context; do not silently overwrite confirmed facts.
8. Reconcile Upkeep/checkouts/records when the bounded authorized phase closes; never mark the overall distributed system complete on the strength of documentation alone.

## Engineering invariants for subsequent phases

- Model and operation schema must specify tensor shapes/dtypes, layouts, device/kernel support, dependencies, and semantics.
- Partitioned attention must correctly combine softmax statistics across key/value shards; no naive averaging.
- Reduction order and numerical drift require defined tolerances and reference tests.
- Every worker reports actual capacity and measured network costs; a worker may be excluded from the latency-critical path.
- KV cache ownership, per-request isolation, migration and model/weight versions must be explicit.
- Execution must handle disconnects, timeouts, cancellation and retry without corrupting generated state.
- Security and privacy apply to weights, activations, prompts, caches and peer connections.
- Do not promise performance gains until they are benchmarked against the same model, hardware budget, batch size and workload.

## Historical documentation-only initial pass

Permitted: README, AGENTS.md, .agent/memory.md. Excluded: runtime code, CI, binaries, licensing guesses, third-party submodules, deployments, new branches, tags, and releases. Remaining engineering design decisions require a new approved phase.


## Authorized development-plugin phase (2026-11-01)

The user separately authorized a development kit analogous to Arcade/Glass and delivery to main. `.agent/plugins/stitchlm-builder-kit/` is that harness; `.agents/plugins/marketplace.json` is discovery and `tests/stitchlm-builder.test.mjs` tests synthetic orchestration. Read the plugin README and engineering contract for kit work. The historical initial-pass exclusions are not a ban on this specifically authorized harness phase. Inference runtime, vLLM backend, peers, model downloads, deployment and releases remain unimplemented and require separate authorization. Future outcomes must still satisfy the operating contract; passing harness tests grants no new authority.
