# Run a host and join over LAN

This is the first physical-device acceptance procedure, not a claim that a real
LAN or your operating system has already passed. The verified chat backend is
LFM2.5-350M. For the pinned Bonsai text backend, use the model/tokenizer
commands in [Bonsai runtime](bonsai-runtime.md) with the same TLS setup.

## 1. Pull and build on the host

Clone https://github.com/LuminaryLabs-Dev/StitchLM or pull main in your existing
clean checkout. Run commands from the repository root. Install Rust through the
official rustup distribution and your platform's native C/C++ linker/build tools.
The repository pins Rust 1.99.1. Windows binaries use the `.exe` suffix.

    rustup target add wasm32-unknown-unknown
    cargo install wasm-bindgen-cli --version 0.2.130 --locked
    cargo xtask fetch-model --directory .stitchlm/models/lfm2.5-350m
    cargo build --locked --release -p stitchlm-host -p stitchlm-connector
    cargo xtask build-web

Keep `dist/` beside the repository root when starting the host. Allow several GB
of available RAM and enough disk for the model and Rust build. The host currently
keeps the full reference model; connectors do not remove that requirement.

## 2. Prepare trusted HTTPS

Use an existing certificate and key whose subject alternative names include the
host LAN IP or hostname you will open. Each joining device must trust its issuer.
For local development, [mkcert's official instructions](https://github.com/FiloSottile/mkcert)
explain creating a local CA and certificates. Installing a CA changes device trust;
perform this yourself only on devices you control. Never bypass certificate warnings.

Example after installing mkcert from its official source, replacing 192.168.1.50
with your host's actual LAN IPv4 address:

    mkdir -p .stitchlm/certs
    mkcert -install
    mkcert -cert-file .stitchlm/certs/host.pem -key-file .stitchlm/certs/host-key.pem 192.168.1.50 localhost 127.0.0.1

Use `mkcert -CAROOT` to locate the public `rootCA.pem`. Transfer only that public
certificate through a trusted channel and follow your device/browser's supported
CA trust procedure. Never distribute `rootCA-key.pem` or the server private key.
Mobile devices may require a separate explicit full-trust step. Certificate files
and the entire `.stitchlm` directory are excluded from Git.

## 3. Start the host

    target/release/stitchlm-host --model-dir .stitchlm/models/lfm2.5-350m --listen 0.0.0.0:8787 --tls-cert .stitchlm/certs/host.pem --tls-key .stitchlm/certs/host-key.pem

Open `https://192.168.1.50:8787/host-web/` using your real address. Enter the
host-access code printed by this launch. Keep that code and the launch log private.
If necessary, allow this application/port only on your trusted private network
through your OS firewall. Do not forward the port to the Internet. Guest Wi-Fi
isolation can prevent devices reaching each other.

## 4. Join another device

Open `https://192.168.1.50:8787/connector-web/` on the other device. Enter the
separate pairing code, choose a device name and memory admission budget, and Join.
Keep the tab open and device awake. Start with one connector; confirm it becomes
ready in the host console before adding more. This baseline supports four.

For a native connector, build the connector on that device and set the
`STITCH_PAIRING_CODE` process environment variable to the current pairing code:

    target/release/stitchlm-connector --host https://192.168.1.50:8787 --ca-cert /path/to/rootCA.pem --name lan-worker --memory-mb 512 --cpu-percent 10

Omit `--ca-cert` for a certificate already trusted by the native client's public
root store. This flag adds one explicit CA; it never disables TLS verification.
The budget is admission planning, not an OS memory limit. CPU duty is cooperative.

## 5. Prove the connection

Generate a short answer from the host console and confirm its remote-block count
is greater than zero. Cancel a longer request, wait for the same connector to
become ready, and generate again. Record host/connector OS, browser, address,
model checksum, response, remote-block count and any error. Keep credentials out
of screenshots. Then test a second device. Wi-Fi throughput, ARM/Pi performance,
mobile suspension and mixed physical-device compatibility still need these tests.

If it fails: verify same LAN, correct address/port, certificate hostname and trust,
current pairing code, available memory, and the host's readiness/error message.
Do not treat a locally generated answer with zero remote blocks as LAN success.
