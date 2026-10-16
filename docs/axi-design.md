# SMAD agent interface design

SMAD means **Show Me Agent Document**. The persistent GTK application is the local document
server; `smad-axi` is its short-lived agent-facing CLI. No additional HTTP/MCP bridge is needed:
we already have a versioned, bounded, same-user Unix-socket control protocol.

## Research

Inspected `chrome-devtools-axi` 0.1.43 help, published README and `dist/src/client.js`, plus
https://axi.md's ten principles. Chrome AXI separates argument parsing/output from a reusable
bridge, probes readiness before reuse, and returns compact metadata plus actionable hints.
SMAD follows those interface principles rather than copying its Chrome/MCP implementation:

- Bare invocation reports live state without launching or raising a window.
- `open` combines routing, startup if absent, opening, and bounded readiness verification.
- Default output is compact TOON-style data; `--json` provides machine-readable output.
- Explicit counts/empty states, bounded default document lists, and `--full` disclosure.
- Structured errors with guidance; unknown commands/flags fail before side effects.
- Contextual next steps and per-command help. An installable skill ships with the release;
  integrations are opt-in, never silently written into an agent's configuration.

This is AXI-style agent ergonomics, not a claim of formal certification or a dependency on an
unofficial Rust AXI SDK. Browser automation commands are out of scope.

## Explicit renderer schemes

These are **local document schemes**, not network hosts:

- `md://./README.md` — native Markdown, relative to the invoking CLI directory.
- `md:///absolute/path/README.md#next-steps` — native Markdown with heading navigation.
- `lavish://./review.html` — create/resume an upstream Lavish session, then display it in WebKit.

The text after `://` is a percent-encoded local path. A triple slash represents an absolute
path; there is no remote authority resolution. Schemes select the renderer, not a guess based
on the suffix. The CLI canonicalizes before IPC and captures the originating stable HerdR or
Zellij identity before startup. Labels never become project identity.

Lavish still owns serving, annotation, feedback, export and review lifecycle. SMAD does not
replace those APIs. Markdown stays local/native, with no HTML conversion or WebKit fallback.

## Native rendering choices

- Syntect 5.3 (MIT), embedded syntax/theme data and Rust regex backend for code highlighting.
- Merman 0.9 (MIT/Apache own code), native SVG with raster-safe output, finite resource policy,
  cooperative cancellation/deadlines and bounded background work. Optional ELK and math/font
  features are excluded; missing capabilities and malformed diagrams are visible errors with
  their source retained, never an unannounced browser fallback.
- GTK table widgets with alignment and inline formatting rather than padded monospaced text.
  Table content must remain available for copying and accessible cell reading.

Upstream references and licenses must ship alongside the release. The old GTK shell, retained
review views, project identity rules and no-raise lifecycle synchronization remain in place.
