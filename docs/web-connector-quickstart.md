# Web connector

Build with `cargo xtask build-web`, then run the native host. The host serves
/host-web/ and /connector-web/ from dist/. Web apps do not listen on network ports.
On another LAN device, open the host's trusted HTTPS address followed by
/connector-web/. Enter the pairing code, device name and memory admission budget.
Join starts a dedicated Web Worker, downloads only assigned model block weights,
checks the shard hash, loads the shared Rust worker and reports readiness.

Keep the tab visible/open and device awake. Mobile/browser suspension can stop
inference. Stop worker terminates local execution; remove its old assignment from
the host before pairing again. Worker failures interrupt current generation and
must not be reported as successful model output. The host access code is separate
from the connector pairing code. Neither belongs in URLs or persistent storage.

A successful WASM build or Node-hosted WASM test is not a browser acceptance test.
The browser test must verify actual page controls, fetch, worker startup, numerical
execution, result transport, cancel/recovery and shutdown. Missing browser access
must remain an explicit validation gap.

See [LAN quickstart](lan-quickstart.md) for host setup, trusted certificates,
native `--ca-cert`, browser joining and the physical-device acceptance checklist.
