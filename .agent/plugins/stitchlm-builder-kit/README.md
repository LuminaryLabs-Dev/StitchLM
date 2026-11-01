# StitchLM Builder Kit

A repository-local **development plugin**, not a vLLM backend or inference engine. It helps a trusted Codex worker produce one explicitly authorized candidate and a reviewable delivery packet. Small outcomes count. It never publishes on a test pass.

## Three tangible stages

1. **Define a build-ready experiment/outcome** — inspect source and authority, preserve full intent and pins, decide the bounded result, acceptance evidence and exclusions. Resolve material assumptions before implementation.
2. **Build the approved candidate** — produce only the requested candidate in the run directory, against the accepted plan. Technical dependencies are internal checklists, not new backlog tasks.
3. **Validate and prepare delivery** — run authorized checks, retain evidence and limitations, return defects to build and failed assumptions to definition. Delivery is not permission to merge, deploy or run devices.

Read `skills/load/SKILL.md` first. See [protocol](references/protocol.md), [engineering invariants](references/engineering.md), and [implementation/validation plan](references/implementation-plan.md).

## Install and inspect

From a checkout with Node.js (standard library only):

```sh
codex plugin marketplace add /absolute/path/to/StitchLM
codex plugin add stitchlm-builder-kit@stitchlm
codex plugin list --json
node --test tests/stitchlm-builder.test.mjs
```

Check `codex plugin --help` against your installed version. Root `plugin.json` is the portable canonical manifest; `.codex-plugin/plugin.json` supplies Codex compatibility metadata. There is deliberately no inline `extensions.com.openai` override. If one is added later it replaces, rather than merges with, that overlay. Marketplace source paths are relative to the repository root.

## Run one outcome

Create a request following the protocol; do not reuse illustrative authorization as real consent. Keep runs outside the tracked source tree.

```sh
node .agent/plugins/stitchlm-builder-kit/scripts/stitchlm-builder.mjs init --run /tmp/stitchlm-outcome --request request.json
node .agent/plugins/stitchlm-builder-kit/scripts/stitchlm-builder.mjs resume --run /tmp/stitchlm-outcome --root . --worker worker.json
node .agent/plugins/stitchlm-builder-kit/scripts/stitchlm-builder.mjs inspect --run /tmp/stitchlm-outcome
```

Each resume performs at most one stage attempt. Repeat within the explicit iteration budget, not indefinitely. Missing worker/model/hardware is a blocker, never a fabricated pass. Source/software-only results cannot satisfy model, numerical, hardware or benchmark gates. Explicitly configured evidence commands can satisfy each requested gate only with request/candidate-bound receipts and raw evidence; missing resources/commands block. Synthetic fixtures prove orchestration only. No new model session, credentials, model download, peer connection or device benchmark is implied by installing the plugin.

Workers and check commands are **trusted local programs**, not OS-sandboxed by this CLI. Review them before execution and use an appropriate sandbox. Workers receive the request over stdin and may write only run artifacts. A separate explicitly authorized `integrate` step can copy validated candidate files into exact preconditioned worktree targets; it never publishes. The CLI does not enforce OS permissions or stop malicious commands from external effects. No worker is bundled for live inference.

## Verify native loading before real work

Real (`fixture:false`) `init`, `resume` and `integrate` require `--installation receipt.json`. The CLI re-runs native plugin listing and verifies enabled identity, version and exact installed-package byte parity, rather than trusting a stale receipt. Synthetic fixtures bypass only this host installation gate and cannot establish external proof or integrate. Package preflight also loads all seven skill files. Neither check proves a model has invoked the skills in a fresh session.

```sh
node .agent/plugins/stitchlm-builder-kit/scripts/stitchlm-builder.mjs verify-install --codex-home /absolute/CODEX_HOME --installed-path /absolute/CODEX_HOME/plugins/cache/stitchlm/stitchlm-builder-kit/0.1.0 --codex /path/to/codex --receipt /tmp/stitchlm-installation.json
# Add --installation /tmp/stitchlm-installation.json to real init/resume/integrate commands.
node .agent/plugins/stitchlm-builder-kit/scripts/stitchlm-builder.mjs integrate --run /tmp/stitchlm-outcome --installation /tmp/stitchlm-installation.json
```

Integration requires an immutable request `integration` contract (see protocol); no automatic source changes on validation success. This supplies a scoped integration step without forcing every future outcome into manual handoff. Recheck current ownership before integration; no global repository lock is implied by a run lock.
