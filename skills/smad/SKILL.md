---
name: smad
description: Show whole code files, source-range highlights, full-file comparisons, native Markdown and interactive Lavish reviews in SMAD.
---

# SMAD — Show Me Agent Document

Run `smad-axi` for live state or `smad-axi --help` for current commands. Neither starts the app.

- `smad-axi open 'code://./src/main.rs?highlight=12-24,48,70-85'` shows the WHOLE file.
  Highlights use original 1-based inclusive source lines, not display rows. Reopening the
  same file/project/renderer replaces its highlights; omit the query to clear them.
- `smad-axi compare ./src/main.rs --base HEAD` captures that exact base once and watches
  the working-tree after side. `--base main --head feature` captures two exact revisions;
  add `--merge-base` deliberately for PR-style base selection. Check resolved commit IDs.
- `smad-axi compare --old ./before.rs --new ./after.rs` captures immutable complete files.
  Use `/dev/null` explicitly for an absent side. All unchanged context and deleted lines
  remain visible; this is not a patch view. There is NO `diff://` scheme.
- Comparison `--highlight 12-24,48` refers to NEW-side source lines; `--highlight-old 3-8`
  refers to OLD-side source lines. Never count unified display rows.
- Markdown source-line overlays are not implemented; do not invent md:// highlight queries.
- `smad-axi open md://./README.md` opens native Markdown with highlighting, tables, Mermaid,
  local images and live disk updates. Relative Markdown links stay in the native reader.
- `smad-axi open lavish://./review.html` opens an upstream Lavish review in the retained WebKit
  surface. Use upstream `lavish-axi` for polling, replies, export and review lifecycle.
- `smad-axi schemes` explains explicit local-file routing. Paths after `://` are local paths,
  not network hosts. Absolute paths use three slashes. Percent-encode literal `#` and `?`.
- `smad-axi status --json` is machine-readable. `--full` shows all documents and full paths.

Opening captures the invoking HerdR/Zellij identity and verifies the target reaches Ready.
Read `warnings` too: a readable document can contain diagram diagnostics, bounded syntax
fallback, or highlights extending past EOF. Do not claim those warnings are absent.
On timeout, inspect status before retrying. Do not claim a successful open after a nonzero exit.
Unknown flags fail; use per-command `--help` rather than inventing switches.

Do not reopen a user-ended Lavish session without permission. `--reopen` is an explicit opt-in.
Do not poll or consume review feedback merely to show a Markdown file.

`smad-axi setup hooks` prints optional session-start integration instructions. No hook or agent
configuration is installed automatically. Never restart the user's server or delete document
state as a troubleshooting shortcut.
