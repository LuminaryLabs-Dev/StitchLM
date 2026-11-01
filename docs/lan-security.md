# LAN security

Default binding is 127.0.0.1:8787 over HTTP for same-device development. Non-loopback
binding requires --tls-cert and --tls-key. Supply an existing certificate trusted
by all connector devices and matching the hostname used to connect. The program
does not generate credentials, install trust roots, change firewalls or bypass
browser certificate warnings. Those are separate user-controlled setup steps.

Host-access code authorizes generation/status/removal. Pairing code authorizes
bounded enrollment. Each enrolled connector receives its own random lease token
and can retrieve only its assigned shard and jobs. Codes expire on host restart.
Treat participating devices as trusted: they receive intermediate activations and
model weights, and their outputs are not cryptographically proven correct.

No CORS access is enabled. Browser adapters require same-origin relative paths.
CSP restricts scripts/workers/connects to self and prevents framing. Secrets are
kept in memory, never query strings or localStorage. Do not share launch logs.
TLS and authentication are necessary but not a full hostile-network security audit.

See [LAN quickstart](lan-quickstart.md) for host setup, trusted certificates,
native `--ca-cert`, browser joining and the physical-device acceptance checklist.
