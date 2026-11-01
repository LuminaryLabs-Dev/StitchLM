# Bounded implementation and evidence

Authority: user explicitly requested a development kit analogous to Arcade/Glass and push to main when ready. Baseline `9e0ace8877c1095ebc942e8f1bbe2cd5817756c4`; original three-file source inspected. Live Upkeep MNT-413 row 419 read 2026-11-01: Needs Review, no In Progress owner; no open repository issues observed. Historical documentation-only restriction is not the authority for this new plugin phase.

Allowed: `.agent/plugins/stitchlm-builder-kit/`, `.agents/plugins/marketplace.json`, harness tests, narrow README/AGENTS/memory updates. Excluded: inference runtime, vLLM integration, peer/device actions, credentials/model downloads, CI infrastructure, deployment, branches, tags/releases, unrelated tracker/Drive changes. Source-derived harness ideas come from Glass and Arcade kits; no FrameFactory product/runtime dependency is carried over.

Outcome stages: define build-ready outcome; build approved candidate; validate and prepare delivery. Internal checks: exact source pins/intent; resumable immutable request; ownership/lock/recovery; independent software acceptance; external evidence blockers; bounded retries; delivery never publishes. Tests use synthetic fixtures and must identify them as such. Install/list in isolated CODEX_HOME does not invoke a model or establish fresh-session skill execution.

Publication: connector Git tree/commit/ref on main with expected-head lease; verify exact file bytes/remote tree and main head. If head or ownership changes, stop and reconcile. No CI workflow exists at baseline; inspect remote checks without inventing a passing CI run.

Canonical tasking remains the [Luminary Framework](https://docs.google.com/document/d/1lZLgr2A99EBzMwoLl4iEY7NiFVFG6LjU8snI2eP3ZWI/edit), [Upkeep Guide](https://docs.google.com/document/d/1nIacfvsNnmFg0kxZxs-tU1XF7w7d97HcfN3JEBOCGpw/edit), [Upkeep MNT-413](https://docs.google.com/spreadsheets/d/11viRvCFI9qTmsuEYfv9lGBI8edaZFRJNE9b7OQDVz-0/edit#gid=1785447018&range=A419:AC419) and the current explicitly authorized outcome. Do not invent a project backlog here.

Packaging reference checked 2026-11-01: https://developers.openai.com/plugins/build/plugins ; portable manifest with compatibility overlay, explicit skill directory and repository-relative marketplace discovery. Recheck actual installed CLI support, not assumed commands.
