# Run the scenario audit

The canonical instructions are [auditor/README.md](../auditor/README.md). The root
`auditor/` package uses a pinned NexusAuditor Python runner and a local Node
adapter. JSONL owns the timing, concurrency, failures and observable assertions.

For a change affecting host/connector behavior, run the fast harness contracts,
then the applicable real-model scenarios, and run the full suite before claiming
this audit delivery accepted. A filtered pass or missing model is not a complete
pass. Keep the existing Rust, builder-kit and model reference regressions.

Use three outcome gates: a usable isolated audit environment, a validated suite,
and a verified source delivery. Repair failures and rerun affected evidence;
never change an expected result merely to turn a defect green. Record exact
source/build/model identities and which conditions were simulated. Main delivery
requires explicit authority and current-head verification; test success alone
never authorizes publishing, deploying or changing other repositories.
