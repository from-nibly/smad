# Native code and full-file comparisons (design notes)

These decisions were developed against v0.3.1. The shipped v0.4 contract and
remaining limitations are documented in [native-source.md](native-source.md).

Scope: `code://` reads whole local source files. `smad-axi compare` compares complete
before/after inputs, never a patch. There is no `diff://` renderer. Editing, patch
application and side-by-side layout are out of scope.

## Rendering choice

Reuse GTK TextView and the existing Syntect integration. GtkSourceView 0.11.2 was
considered, including its native line-number gutter and language/style managers.
It would add a system-library and language-data installation requirement; a unified
comparison still needs custom old/new numbering and independent before/after
syntax state. GTK TextView already provides native gutter widgets, source text
selection and viewport geometry. A dedicated gutter avoids inserting line numbers
into selectable source text. Keep before/after syntax parsing independent.

Focused references inspected: MIT-licensed Stet `app/src/compare.rs` (paragraph
backgrounds, empty-line caveat) and `domain/src/diff/mod.rs` (full-input histogram
diff). These are design references, not an editor code import. imara-diff 0.2 is
the selected line-diff engine. Existing Syntect limits must remain explicit;
plain-text fallback must preserve the entire accepted file and report a warning.

## Identity and compatibility

The v0.3.1 model keyed documents by project + source path and rejected opening
one path with different renderer kinds. v0.4 identity includes renderer,
and persists the selected renderer alongside the selected path. Legacy persisted
selection must infer its kind from the matching saved document. Retain protocol
v2's existing commands and default fields; add new commands and snapshot fields.
A repeated open replaces agent overlays, including clearing them when omitted.
A repeated comparison for the same file/project replaces its comparison inputs,
not other renderer tabs. No mutation of legacy installed state.

## Source coordinates

`?highlight=12-24,48,70-85` uses 1-based inclusive original source lines. Normalize
sorted overlapping/adjacent ranges, cap string length and range count, reject
zero/reversed/malformed ranges. Literal filename `?`/`#` stays percent-encoded.
Comparison overlays default to new-side coordinates; expose old-side ranges
explicitly. Out-of-bounds overlays must be visible diagnostics, not silently
interpreted as rendered row numbers. Gutter markers and a distinct emphasis
style must distinguish agent attention from additions/deletions and selection.

## Comparison inputs

`compare FILE --base REV` compares the exact fixed revision with a live working
tree file. `--base REV --head REV` uses two immutable revisions. An explicit
merge-base option, if offered, must resolve once and identify the resolved base.
`--old FILE --new FILE` captures file snapshots. `/dev/null` can explicitly mean
an absent side; missing arbitrary explicit filenames must not silently mean
added/deleted content. Git tree absence is different from a Git command error.

Resolve Git without shell interpolation or external diff/textconv commands;
validate revisions as commits, bound blob sizes before reading, and enforce
subprocess output/time bounds. Snapshot content must be fixed and durably stored
before acknowledgment, with content-addressed references in model metadata.
Live updates reload only the working-tree after side; do not re-resolve HEAD or
re-read the old input. Missing snapshots fail honestly instead of recomputing a
new base. Atomic saves must retain last-good content on transient failure.

Unified rows retain all common context, then removed/added rows for each change.
Each row records old/new source coordinates independently of display rows.
Different final newline and CRLF/LF states require explicit indicators. Empty
files, absent sides and no-change results must be distinguished.

## Bounded work and verification

Bound UTF-8 source bytes, source lines, line length for syntax work, range count,
comparison work and pending jobs. Compute comparisons off the GTK thread, discard
stale results, and preserve scroll on live refresh. Prefix/suffix trimming can
reduce the work bound without discarding displayed context.

Gate on parser/protocol/migration tests; Unicode character vs byte offsets;
multiple ranges, overlap and bounds; full context and dual numbering; empty,
inserted/deleted files and final newlines; immutable Git snapshots vs live after;
atomic saves and stable base; native-only process checks; existing Lavish/HerdR
regressions. Inspect real GUI captures in light/dark themes. Update help, skill,
operations, licensing and release packaging; publish only after CI and verify
actual downloaded assets. Do not install or activate.

Markdown source-coordinate overlays remain a separately assessed bonus: pulldown
byte-offset mapping must identify source spans, and embedded tables/images/
diagrams may only promise explicitly labeled whole-block precision. Never map
source line requests to rendered line counts.
