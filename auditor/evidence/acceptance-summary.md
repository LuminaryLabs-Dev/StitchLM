# Scenario auditor acceptance

PASS: 16/16 real-model scenarios, 42/42 harness contracts on three consecutive
clean-checkout runs, 42 Rust tests, and 36 builder-kit tests with native installation
verified. Rust formatting and Clippy passed.

The source baseline is `952967dc7700d6b0a6e24340c9b1f9d2c98881ed`. Exact candidate files,
clean-built native/WASM identities, model results and evidence hashes are in
[acceptance-results.json](acceptance-results.json). Runtime Rust files were unchanged.
The checked working candidate preceded the delivery commit; source hashes bind it
to delivery without pretending the commit existed before testing. Final narrative
receipt/changelog updates are not runtime code changes.

- ST-AUDIT-001 (lfm): PASS, 6.7 seconds.
- ST-AUDIT-002 (lfm): PASS, 5.9 seconds.
- ST-AUDIT-003 (lfm): PASS, 9.7 seconds.
- ST-AUDIT-004 (lfm): PASS, 8.5 seconds.
- ST-AUDIT-005 (lfm): PASS, 6.5 seconds.
- ST-AUDIT-006 (bonsai): PASS, 678.9 seconds.
- ST-AUDIT-007 (lfm): PASS, 6.6 seconds.
- ST-AUDIT-008 (lfm): PASS, 6.4 seconds.
- ST-AUDIT-009 (lfm): PASS, 10.1 seconds.
- ST-AUDIT-010 (lfm): PASS, 11.2 seconds.
- ST-AUDIT-011 (lfm): PASS, 6.3 seconds.
- ST-AUDIT-012 (lfm): PASS, 9.6 seconds.
- ST-AUDIT-013 (lfm): PASS, 6.7 seconds.
- ST-AUDIT-014 (bonsai): PASS, 175.2 seconds.
- ST-AUDIT-015 (bonsai): PASS, 213.2 seconds.
- ST-AUDIT-016 (lfm): PASS, 7.3 seconds.

## Boundaries

- Application-request timing/failure simulation on loopback, not physical LAN or packet-level validation.
- Exact pinned legacy Ternary Bonsai artifact and LFM only, not Bonsai2 or arbitrary GGUFs.
- Token parity plus preserved runtime matrix-reference checks, not a new independent full-logit reference run.
- Browser memory sampled, not hard-capped. Host memory is separate.
- No speedup, production uptime, arbitrary prompts, mobile/TV or physical device certification.
- Setup validated with connector-materialized, Git-blob-verified NexusAuditor source. Authenticated private Git install route not exercised here.

## Repairs found during validation

- Browser cleanup raced context shutdown; unified owned-browser cleanup.
- DevTools response body capture failed for streaming UI; observe a clone of the actual response in-page.
- Built-in fetch had implicit 300000ms stream timeout; explicit scenario deadline now governs Node HTTP inference stream.
- Normalized/symlink artifact paths are restricted to auditor/.runs.
- Host-only reference scenarios explicitly request zero remote connectors; preflight rejects contradictory inputs.

Only sanitized summaries are committed. Raw runs, dependencies, weights, launch
credentials and temporary services remain local. No release tag, deployment,
tracker closure or changes to the shared NexusAuditor repository are implied.
