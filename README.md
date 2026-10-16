# SMAD — Show Me Agent Document

A small native Linux desktop surface for documents agents want people to see. One persistent
GTK window groups documents by the invoking HerdR or Zellij project. `smad-axi` is the agent CLI.

**The URI scheme chooses the renderer:**

```zsh
smad-axi open md://./README.md
smad-axi open 'md:///absolute/path/to/plan.md#next-steps'
smad-axi open lavish://./review.html
smad-axi status
```

- **`md://`** — native GTK Markdown: syntax-highlighted code, native Mermaid SVG, rich aligned
  tables, selectable text, local images, relative document links, and debounced live disk updates.
- **`lavish://`** — unchanged upstream [Lavish](https://github.com/kunchenguid/lavish-axi) review
  sessions in retained WebKit views. Upstream owns serving, annotations, feedback, export,
  whiteboards and session lifecycle; use `lavish-axi` for those operations.

No HTML conversion, JavaScript runtime or WebKit fallback is used for native Markdown.
Mermaid errors retain source and show diagnostics; AXI reports `ready_with_warnings` rather
than hiding incomplete diagrams. See [native rendering and limits](docs/native-markdown.md).

## Agent CLI

```zsh
smad-axi                         # live state; never starts or raises the app
smad-axi open md://./plan.md     # start/reuse server, open, verify Ready
smad-axi documents --full
smad-axi status --json
smad-axi schemes
smad-axi open --help
```

Default output uses the official Rust TOON encoder, with counts, compact document rows,
structured errors and contextual next steps. `--json` provides machine-readable JSON.
Unknown commands/flags exit 2 without side effects; operational failures exit 1. Readiness
is checked for the exact project/file opened, not whichever document happens to be selected.

Paths after `://` are **local paths**, not network authorities. Relative paths resolve from the
invoking directory. Percent-encode literal `#` and `?` in filenames; Markdown supports a heading
fragment. Only `.md`/`.markdown` and `.html`/`.htm` files are accepted by their respective schemes.

Opening captures the invoking HerdR session/workspace/tab IDs or Zellij session/stable tab ID.
Display labels never become identity. Opening the same file in one project reuses its view;
different projects can retain separate views. Outside either runtime, documents use Standalone.

An installable [agent skill](skills/smad/SKILL.md) ships in `share/smad/skills/smad/`.
`smad-axi setup hooks` prints opt-in session-context instructions; it does not edit any agent
configuration. [AXI research/design](docs/axi-design.md) records the interface decisions.

## Install

Download the Linux x86-64 bundle from [GitHub releases](https://github.com/from-nibly/smad/releases).
It includes both **`smad` (the persistent GUI/server)** and **`smad-axi`**, integrations, desktop
metadata, checksum manifest and third-party notices/source. Standalone server and AXI binaries
are also published; install the server as `smad` beside `smad-axi`, and retain the accompanying
third-party notices archive.

Runtime requirements:

- Linux x86-64 with GTK 4.10+ and WebKitGTK 6; published binaries target Ubuntu 24.04's ABI.
- Native SVG image loader (for example `librsvg2-common` on Ubuntu/Pop!_OS).
- `npx`/`lavish-axi` only for `lavish://` review sessions—not for native Markdown.
- HerdR or Zellij only for contextual grouping and optional lifecycle synchronization.

On NixOS, package/patchelf the native release against Nix GTK/WebKit as before. On Pop!_OS,
use distro GTK/WebKit and remove inherited nixGL/Mesa overrides in the **canonical `smad`
launcher**, not just a legacy alias. See [operations](docs/operations.md).

From source (Rust 1.95+, Nix recommended):

```zsh
git clone https://github.com/from-nibly/smad.git
cd smad
nix-shell --run './scripts/check.sh'
nix-shell --run 'python3 tests/e2e/native_markdown.py'
nix-shell --run './packaging/install.sh --prefix "$HOME/.local"'
```

Ensure `$HOME/.local/bin` is on PATH. `smad-axi` first looks for a sibling `smad`, then PATH;
`SMAD_EXECUTABLE` overrides it. `SMAD_SOCKET` selects an isolated control socket.
The installer supports `DESTDIR`, `--profile debug`, and `--no-build`.

## Existing Lavish Browser users

This project was previously `from-nibly/lavish-browser`.

- On first use, SMAD reads `~/.local/state/lavish-browser/state.json` only if its new state file
  is absent. Future writes go to `~/.local/state/smad/state.json`. The legacy file is untouched.
- `lavish-open` and `lavish-md-open` remain compatibility launchers. Legacy browser/control/sync
  executable names are aliases to the new binaries. New integrations should use `smad-axi`.
- SMAD uses its own application ID/socket. Installing does not kill or replace a running legacy
  browser. Close the old window when you choose; do not run two lifecycle sync services against
  the same new socket.
- A spawned server logs to the SMAD state directory (`server.log`), or next to a custom socket.
- Closing browser documents never invokes `lavish-axi end`. A user-ended review requires explicit
  permission and `smad-axi open lavish://... --reopen` to resume.

## Lifecycle integration and verification

`smad-herdr-sync` silently selects/renames existing projects and closes removed projects.
It never starts or raises the browser. See [HerdR setup](docs/herdr.md) and
[Zellij setup](docs/zellij.md). The low-level `smad-ctl` helper remains available for integrations.

`./scripts/check.sh` checks formatting, Clippy, workspace tests and the WASI plugin build.
The isolated native GUI test verifies rendering, selection/copy, links, atomic saves,
last-good recovery, scroll retention, cancellation, cleanup, CLI opens and review-surface smoke.
It uses a private bus/display/socket/state and never restarts the installed browser.

This is not a claim of complete GFM/Mermaid.js parity. Full upstream Lavish compatibility,
including manual whiteboard/export evidence, remains a separate gate described in
[compatibility](docs/compatibility.md) and [development](DEVELOPMENT.md).
