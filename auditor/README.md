# StitchLM scenario auditor

One root audit package drives the real Rust host and native/browser connectors.
Timing, overlapping actions and injected failures live in JSONL scenarios. There
is no separate LANLint module. NexusAuditor remains the Python audit entry point;
its existing `command` action invokes this package's Node adapter. This does not
rewrite NexusAuditor or replace Rust inference with JavaScript.

## Setup from a clean checkout

Use Linux with Node 22+, Python 3.9+, `venv`, `prlimit`, the repository-pinned Rust
toolchain, wasm-bindgen CLI, a supported Playwright browser, and enough disk/RAM
for the model and builds. The native connector test enforces a 1,000,000,000-byte
virtual-address cap with Linux `prlimit`; other operating systems are currently
explicitly blocked for this suite, not falsely certified.

From the repository root:

```sh
# Keep npm cache inside the checkout when the sandbox home is read-only.
export NPM_CONFIG_CACHE="$PWD/auditor/.runs/npm-cache"
node auditor/scripts/setup.mjs
cargo build --locked --release -p stitchlm-host -p stitchlm-connector
cargo xtask build-web
cargo xtask fetch-model --directory .stitchlm/models/lfm
cargo xtask fetch-bonsai --directory .stitchlm/models/bonsai
export STITCH_AUDIT_LFM="$PWD/.stitchlm/models/lfm"
export STITCH_AUDIT_BONSAI="$PWD/.stitchlm/models/bonsai"
node auditor/scripts/doctor.mjs full
node auditor/scripts/audit.mjs fast
node auditor/scripts/audit.mjs lfm
node auditor/scripts/audit.mjs bonsai
# Or run the complete real-model suite sequentially:
node auditor/scripts/audit.mjs full
```

NexusAuditor is a private GitHub dependency pinned in requirements.lock.txt.
Use existing authorized Git access; never put a token in a command, file or URL.
When a connector materializes the exact source instead, pass
`--auditor-source /path/to/NexusAuditor` to setup. Every source file is checked
against its pinned Git blob before installation. Setup never silently substitutes
a different auditor version. `--skip-browser` is for an already installed browser;
set `STITCH_AUDIT_BROWSER` to its executable. Playwright's default browser install
can also use its documented `PLAYWRIGHT_BROWSERS_PATH` environment variable.

If a tool's TLS trust store cannot validate the environment's proxy certificate,
use a download tool that validates against the already trusted system store and
verify the exact manifest sizes/hashes. Do not disable TLS verification or alter
the pinned model. No models, dependencies or launch credentials belong in Git.

`STITCH_AUDIT_ONLY=ST-AUDIT-006` selects one case within its model suite. `full`
is the acceptance suite; a filtered or fast-only pass is not full acceptance.
Missing prerequisites return BLOCKED / exit 2. Assertion or runner failures
return FAIL / exit 1. Only PASS returns exit 0. Python NexusAuditor also records
its own result; the top-level summary preserves a scenario's BLOCKED distinction.

## Authoring scenarios

Each file begins with `id`, readable `name`, supported `models`, and an overall
`timeout_ms`. Remaining lines are schema-validated actions. Unknown fields,
unsafe IDs, unsupported actions and duplicate generation IDs fail before launch.
Use `parallel` for actual overlapping actors and `wait.event` or `wait.active`
for causal ordering. Use `delay` only for an intentional spacing/delay condition,
not as an assertion that a server is ready. All timing values are milliseconds.

`fault` declares actor, ID, path, optional method and mode: `hold`, `delay`,
`abort` or `duplicate`. Each declaration is one-shot. `wait.event` proves the
request reached the injection point; `release` releases a hold. Untriggered
faults fail the scenario. These are application-request perturbations through
owned loopback proxies, not packet loss emulation, router changes or physical
LAN evidence. The fixed scenario schedule is reproducible in causal order;
wall-clock timestamps vary with CPU and operating-system scheduling.

`generate` can run in the background, but every request must be awaited.
`expect_status` checks HTTP admission. `expect` checks terminal generation events.
Require nonzero `remote_blocks` for LFM and `remote_shards` for Bonsai; local
fallback alone is not remote proof. `assert.tokens` compares nonempty token
sequences. The first-generation journey uses the actual host web console.

## Coverage and interpretation

The 16 files cover first use, wrong pairing/retry, spaced and simultaneous joins,
double generation, cancellation during a Bonsai weight fetch, late and duplicate
results, browser rejoin, host restart/auth invalidation, slow and interrupted
requests, native memory admission, Bonsai row/column token parity and repeated
requests. The interrupted-request journey explicitly reconnects after the fault;
it does not claim transparent retry for an arbitrary broken TCP connection.

Bonsai means the exact legacy Ternary Bonsai 27B Abliterated artifact, not Bonsai2.
The matrix backend keeps its reference checks enabled; token equality supplements
those checks. Existing independent full-logit reference evidence remains separate.
No new all-prompts quality, long-context, physical-device, throughput or universal
GGUF support claim follows from this suite.

## Evidence and isolation

Each invocation creates a unique `.runs/` directory with generated command
wrappers, the effective NexusAuditor profile, its manifest/results, per-case JSON,
actor/fault timelines, observed generation results and sampled process memory.
The browser console capture clears the access-code field before capture.
Native address limits are enforced; browser process memory is measured rather
than hard-capped. Host memory is separate. CPU pacing is cooperative, not an OS
quota. Short repeated runs detect only the stated bounded growth, not every leak.

The adapter accepts only owned `127.0.0.1` HTTP origins, refuses redirects,
blocks browser service workers, validates artifact paths, redacts known secrets,
and terminates only its owned processes and contexts. This is trusted test
software running inside the executor's existing sandbox, not an independent OS
security boundary. It does not scan a LAN, change firewall/trust settings or
interact with external devices. Raw runs remain local; only reviewed, sanitized
acceptance summaries should be committed. A failure keeps the outcome open.
