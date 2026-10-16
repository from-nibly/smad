# Installation and operations

## Source installation

From a clean checkout, use the pinned Nix environment and lockfile:

```bash
nix-shell --run './scripts/check.sh'
nix-shell --run './packaging/install.sh --prefix "$HOME/.local"'
```

The installer accepts `--prefix`, `--destdir`, `--profile debug|release`, and `--no-build`. GTK 4, WebKitGTK 6 and a native SVG loader are server runtime requirements. `npx` and upstream `lavish-axi` are required only for Lavish reviews. HerdR/Zellij are optional.

## SMAD rename and launch environment

The primary entry points are `smad` (persistent GUI/server) and `smad-axi`. Use
`smad-axi open md://./file.md` or `smad-axi open lavish://./file.html`; read-only status/help
never starts or raises the server. Opening captures project identity and verifies readiness.

When the new SMAD state file is absent, metadata is read from the legacy
`$XDG_STATE_HOME/lavish-browser/state.json` (same HOME fallback). All new writes go to the
SMAD path, leaving the legacy file intact. The application ID and socket are new. Installation
does not stop a running old browser; close it explicitly when ready, and update the lifecycle
helper rather than running two helpers against the same socket.

On Pop!_OS the release uses native distro GTK/WebKit. The Home Manager wrapper must clear
`LD_LIBRARY_PATH`, `LIBGL_DRIVERS_PATH`, `__EGL_VENDOR_LIBRARY_FILENAMES`, and
`GBM_BACKENDS_PATH` on **`bin/smad`**. Wrapping only the compatibility `lavish-browser` alias
is insufficient: the AXI launches the canonical server. On NixOS keep the Nix-patched
GTK/WebKit/GApps environment instead; do not apply the Pop!_OS cleanup there.

The bundle retains `lavish-browser`, `lavish-browser-ctl`, and `lavish-browser-herdr-sync`
as aliases, plus the legacy `lavish-open`/`lavish-md-open` launchers. Legacy socket/executable
environment overrides remain accepted; use `SMAD_*` names for new integrations.

## Legacy launcher migration and rollback

Do not overwrite an existing local `lavish-open` script until the installed Rust launcher has passed a real open in your environment.

1. Install to a separate prefix and invoke `<prefix>/bin/lavish-open artifact.html` explicitly.
2. Confirm the upstream TOON is preserved and the session appears in SMAD.
3. Back up or rename the old script, then put the new prefix before it on `PATH`.
4. To roll back, restore the previous `PATH` order or script. This does not migrate or alter upstream Lavish state.

The launcher forwards its arguments to `lavish-axi` and appends `--no-open` when absent. Upstream failures and `user-ended` behavior are preserved. There is no launcher-owned `--help`; run without arguments for local usage (it prints usage and exits nonzero) and consult upstream Lavish for its open options. `smad-ctl --help` likewise prints the control usage through its invalid-argument path and exits nonzero; use the forms documented below rather than treating it as a health check.

## Runtime and state paths

| Purpose | Default | Override |
|---|---|---|
| Control socket | `$XDG_RUNTIME_DIR/smad/control.sock` | `SMAD_SOCKET` |
| Browser metadata | `$XDG_STATE_HOME/smad/state.json` | set `XDG_STATE_HOME`; fallback is `$HOME/.local/state` |
| Server executable used by launcher | sibling `smad`, then `PATH` | `SMAD_EXECUTABLE` |
| Spawned server log | SMAD state directory `server.log`; next to a custom socket | follows the state/socket override |

An isolated second instance needs a separate session bus, socket **and** state directory;
changing only the socket does not bypass GTK's application singleton.

The socket directory is mode `0700`; the socket is user-restricted and Linux peers are UID checked. Metadata contains browser project/document ordering, labels, source paths, session URLs, selections, and timestamps—not feedback or artifact contents. Writes are atomic and restrictive. Corrupt or unsupported state is ignored safely. On startup, Standalone entries remain and Zellij projects are reconciled by exact live session/stable-tab identity. Restored views begin dormant and materialize lazily.

## Diagnostics and recovery

- `smad-ctl ping` reports a healthy response or `{"status":"absent"}`.
- `smad-ctl status --json` returns the bounded read-only browser snapshot.
- If a view shows **Failed** or **Reconnect**, confirm upstream Lavish is running and run `lavish-open <file>` again. The browser does not start Lavish during reconnect or memory resume.
- If startup cannot create the socket, verify ownership and permissions of `$XDG_RUNTIME_DIR/smad`, or set `SMAD_SOCKET` to a private path.
- If Zellij routing fails, verify both `ZELLIJ_SESSION_NAME` and `ZELLIJ_PANE_ID` and that `zellij action list-panes --all --json` can find the invoking pane.
- Closing browser tabs does not end upstream review. Use upstream `lavish-axi end <file>` only when review semantics require it.

`SMAD_AUTOMATION`, `SMAD_MEMORY_WARNING_FILE`, and debug-build `SMAD_INSPECTOR` are test/diagnostic controls, not normal user configuration. Memory behavior is documented in [memory.md](memory.md).
