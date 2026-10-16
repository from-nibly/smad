# Development and validation

Use the repository Nix shell; it supplies Rust 1.95, GTK 4, WebKitGTK 6, the WASM target, WebKitWebDriver, Selenium, AT-SPI/Dogtail, Xvfb, Zellij, and installed-E2E desktop tooling.

```bash
nix-shell
./scripts/check.sh
```

The check script enforces formatting, warning-denied Clippy for all workspace targets, all workspace tests, and the `wasm32-wasip1` plugin build. Production workspace members are under `crates/`; there are no prototype binaries or alternate browser/launcher implementations.

For focused development:

```bash
cargo test --locked -p smad-protocol
cargo test --locked -p smad-core
cargo test --locked -p smad-control
cargo test --locked -p smad-cli
cargo test --locked -p smad
cargo build --locked -p smad-zellij --target wasm32-wasip1
```

Run native components from the shell with `cargo run --locked -p smad` and `cargo run --locked -p smad-cli --bin lavish-open -- <file>`. Upstream `npx`/`lavish-axi` is required for a real open.

Release validation additionally requires `./scripts/e2e-installed.sh`. It builds and installs into an isolated prefix, uses real GTK/WebKitGTK, upstream Lavish and polling, WebKitWebDriver, AT-SPI/Dogtail, and an isolated Zellij plugin session, then writes evidence to `target/e2e/`. A release PASS requires the complete generated matrix and manual Excalidraw integrity correlation; unit/component results cannot substitute.

Compatibility harness details and prior baseline versions are in [compatibility.md](compatibility.md). Keep implementation changes on the browser side of the boundary in [architecture.md](architecture.md); do not add an artifact server, custom content URI, or Lavish API implementation.
