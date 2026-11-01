# Native quickstart

Install Rust 1.99.1 from rustup's official distribution. From the repository root:

    cargo xtask fetch-model --directory .stitchlm/models/lfm2.5-350m
    cargo build --locked --release -p stitchlm-host -p stitchlm-connector
    cargo xtask build-web
    target/release/stitchlm-host --model-dir .stitchlm/models/lfm2.5-350m

`build-web` requires the wasm32-unknown-unknown target and wasm-bindgen-cli 0.2.130:

    rustup target add wasm32-unknown-unknown
    cargo install wasm-bindgen-cli --version 0.2.130 --locked

The CLI version must match the locked Rust wasm-bindgen dependency.

The host prints separate host-access and pairing codes. They expire on restart.
Open http://127.0.0.1:8787/host-web/ and enter the host-access code. To start a native
connector, set STITCH_PAIRING_CODE in its process environment, then:

    target/release/stitchlm-connector --host http://127.0.0.1:8787 --name worker1 --memory-mb 512

The admission budget is conservative model
planning, not an OS RAM quota. Never publish process logs containing access codes.
Windows executables have .exe suffix; Windows/macOS operation must be tested on
those systems before claiming validation. Close connector processes with Ctrl+C.

See [LAN quickstart](lan-quickstart.md) for host setup, trusted certificates,
native `--ca-cert`, browser joining and the physical-device acceptance checklist.
