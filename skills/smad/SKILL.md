---
name: smad
description: Show agent documents in SMAD's native desktop reader, using md:// for Markdown and lavish:// for interactive Lavish reviews.
---

# SMAD — Show Me Agent Document

Run `smad-axi` for live state or `smad-axi --help` for current commands. Neither starts the app.

- `smad-axi open md://./README.md` opens native Markdown with highlighting, tables, Mermaid,
  local images and live disk updates. Relative Markdown links stay in the native reader.
- `smad-axi open lavish://./review.html` opens an upstream Lavish review in the retained WebKit
  surface. Use upstream `lavish-axi` for polling, replies, export and review lifecycle.
- `smad-axi schemes` explains explicit local-file routing. Paths after `://` are local paths,
  not network hosts. Absolute paths use three slashes. Percent-encode literal `#` and `?`.
- `smad-axi status --json` is machine-readable. `--full` shows all documents and full paths.

Opening captures the invoking HerdR/Zellij identity and verifies the target reaches Ready.
Read `warnings` too: a document can be readable while a diagram has a visible diagnostic.
On timeout, inspect status before retrying. Do not claim a successful open after a nonzero exit.
Unknown flags fail; use per-command `--help` rather than inventing switches.

Do not reopen a user-ended Lavish session without permission. `--reopen` is an explicit opt-in.
Do not poll or consume review feedback merely to show a Markdown file.

`smad-axi setup hooks` prints optional session-start integration instructions. No hook or agent
configuration is installed automatically. Never restart the user's server or delete document
state as a troubleshooting shortcut.
