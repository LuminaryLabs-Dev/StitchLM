# Resumable protocol v1

The CLI exports `validateRequest`, `verifySource`, `initialize`, `resume`, `inspect`, `recover`, `integrate`, `verifyPlugin`, `installationReceipt`; all are standard-library Node ES modules. Commands: `init`, `resume`, `inspect`, `recover`, `verify-install`, `integrate`. Exit 0 means READY/COMPLETE, 2 BLOCKED/error, 3 RETRY. Recovery needs `--reason`; refuses a live PID lock and preserves interrupted attempts. Inspect external effects before recovery; it does not replay them.

## Request

JSON fields (all required):
- `schema`: `stitchlm-build-request/1`
- `repository`: `LuminaryLabs-Dev/StitchLM`; `commit`: full 40-character lowercase SHA
- `outcome`, `intent`, `tracker`, `authorization`: nonempty exact strings; use canonical task identity/link and actual authority, not a parallel backlog
- `acceptance`, `exclusions`, `allowedPaths`: nonempty arrays of strings; enumerate the authorized candidate target paths and explicit no-go actions
- `pins`: nonempty array of `{path, sha}` for every relevant source/dependency file, SHA being Git blob SHA-1; include root instructions and relevant implementation, not merely a convenient subset
- `requiredGates`: array containing `source`, optionally `software`, `numerical`, `model`, `hardware`, `benchmark`
- `fixture`: boolean; `publish`: false; `maxAttempts`: integer 1..100

Commit is an approved identity label, not independently fetched by CLI; caller must establish pins against that remote commit. Byte verification works with connector-materialized trees. Re-pinning or changing intent requires a new authorized run. No model/version defaults are guessed.

## Trusted worker configuration and exchange

Config: `{ "schema":"stitchlm-worker/1", "fixture":true, "command":["node","/absolute/path/to/reviewed-worker.mjs"], "softwareChecks":[["node","/absolute/path/to/acceptance.mjs"]], "timeoutMs":120000 }`. Fixture must match request. Command uses argument arrays, no shell expansion. Configure only explicitly authorized commands; do not use a success-only command as acceptance evidence.

One JSON task arrives on stdin: schema `stitchlm-worker-task/1`, stage (`Stage1`, `Stage2`, `Stage3`), full request and requestHash, attempt, outputDirectory, accepted plan/candidate, prior artifact paths/hashes, and constraints forbidding source/repository/tracker writes/publication. Workers write artifacts inside outputDirectory and return one JSON stdout object:

`{schema:"stitchlm-worker-result/1", requestHash, stage, attempt, status, summary, ...}`

Status `PASS`, `ASSUMPTION`, `DEFECT`, or `BLOCKED`; identity must match the task. Stage1 PASS adds `plan` with exact request `intent`, `acceptance`, `exclusions` plus nonempty `approach`; include source-grounded reasoning, exact scope, applicable invariants, resource needs and rollback. Stage2 PASS adds `candidate:{summary,files:["relative-file"]}`; files must exist inside the attempt. Stage3 PASS is only a request to run independent checks, not proof itself. Checks run with run directory as cwd and stdin JSON `{request,candidate,artifacts}`. Use artifact paths to validate the candidate; candidate file names are relative to its originating attempt.

ASSUMPTION clears accepted work and returns Stage1 (inspect/define); DEFECT returns Stage2 if a plan exists, otherwise Stage1; BLOCKED holds stage. Evidence/source drift returns Stage1. Iteration budget stops further worker execution, including repeated blockers. There is no implicit budget reset. Plans and evidence are hash-chained snapshots; artifact bytes are rechecked on resume and complete readback. This detects accidental drift, not adversarial state forgery. Run locks provide local process exclusion only; remote ownership must be checked separately.

## External evidence gates

`worker.evidenceChecks` maps gate names to explicitly authorized command argument arrays, e.g. `{"numerical":["python","/approved/check_numerics.py"]}`. These run in the current attempt with stdin schema `stitchlm-evidence-task/1`, gate, full request/requestHash, candidateHash, candidate, artifacts, runDirectory and outputDirectory. The validator must actually inspect the candidate, execute the required comparison/device/model/benchmark procedure, and write nonempty raw evidence files. It must return JSON `schema:stitchlm-evidence-result/1`, exact gate/requestHash/candidateHash, `fixture:false`, `passed:true`, nonempty `procedure`, nonempty `environment` and `measurements` objects, `limitations` array, and `files` relative to this attempt. Pin versions, model/hardware/workload, tolerances and actual results in those fields as applicable. No supplied command or missing resources -> BLOCKED. Nonzero, malformed, false or unbound results do not pass. A worker can explicitly report DEFECT/ASSUMPTION to route repairs. Fixture requests cannot satisfy these gates.

This is an execution/evidence contract, not an oracle that detects dishonest validators. Review command semantics and raw evidence; a fabricated receipt is not real proof. Tests simulate the contract and label it synthetic, even when exercising live-mode schema branches. Delivery is FIXTURE_ONLY, SOURCE_ONLY, SOURCE_SOFTWARE_ONLY or SCOPED_EVIDENCE_ONLY; the last supports only explicitly recorded gates, not extrapolated performance or deployment claims.

## Scoped integration

Optional immutable request field: `integration:{authorization:"exact authority",targetRoot:"/absolute/approved/worktree",expected:{"candidate/file.py":"<prior Git blob SHA>","new-file.py":null}}`. `null` requires absence. Allowed paths are exact relative files or explicit directory prefixes ending `/`. Candidate files must be inside that scope. Provision authorized parent directories before integration; it does not create directories or delete files.

Only explicit `integrate` after COMPLETE performs copies, and only with verified native installation and non-fixture scope. It verifies artifact integrity, every target precondition and parent containment, rejects symlinks/conflicting edits, journals before effects, writes each file atomically and verifies readback. Retry accepts only original or exact desired bytes. Multi-file integration is resumable, not transactional. An interrupted temporary file is reused only if non-symlink and byte-identical to the desired candidate while destination preconditions still hold; mismatches block for manual inspection. Inspect other interrupted external effects before recovery. Recheck ownership before execution; caller must prevent concurrent worktree writers. No git push, tracker write, deployment, tag or release is implemented. Integration receipt records exact target/file hashes and `published:false`.

## Verified native load gate

Real initialize/resume/integrate requires the native receipt described in plugin README. Each call verifies installed/enabled state and current package hash against installed cache using actual `codex plugin list --json`. Version/path/hash must match; changed or uninstalled packages block until reinstalled/reverified. Fixture bypass is synthetic-only. `verifyPlugin` additionally reads manifest and seven explicit skills before work. Actual fresh-session skill invocation remains a distinct unverified claim.
