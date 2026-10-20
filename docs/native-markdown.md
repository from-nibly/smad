# Native Markdown in SMAD

`smad-axi open md://./README.md` selects the native renderer explicitly. A single GTK
TextView/TextBuffer holds the document, with native table/diagram child widgets. No HTML
intermediate, browser subprocess, JavaScript runtime, or WebKit fallback is used.

## Implemented

- Headings, paragraphs, emphasis, strong text, strikethrough, nested/ordered lists, quotes,
  task markers, rules, inline code and fenced/indented code.
- Expanded [Syntect syntax highlighting](syntax-highlighting.md), including CUE, Nix,
  Terraform/HCL and dedicated TypeScript/TSX grammars. Unknown languages and unusually
  large code blocks stay readable as plain code. The [catalog](syntax-catalog.md)
  lists bundled definitions; coverage tests exercise both filenames and fence aliases.
- Native GFM table grids: column alignment, wrapping, bold/italic/code cells, working links,
  selectable cell text and visible headers. Cross-block Copy serializes tables as tab-separated
  rows rather than dropping embedded content; cell selection has its normal native copy behavior.
- Native Mermaid SVG via Merman 0.9. Six common families (flowchart, sequence, class, state,
  entity-relationship and pie) have a verified fixture corpus. Other compiled families are
  available but do not carry a blanket parity claim.
- Diagrams are rasterized by the native SVG loader, with a collapsible selectable source view.
  Malformed/unsupported/over-budget diagrams retain their source with a visible diagnostic.
  The document can be Ready with warnings; `smad-axi` reports that explicitly.
- Local images and visible alt text. Remote images are not fetched; raw HTML stays inert text.
- Relative `.md`/`.markdown` links resolve against the current document's directory and stay
  in its stable project. Heading fragments, percent-encoded paths and duplicate heading suffixes
  are supported. Text links and the keyboard-accessible Links menu share the same routing.
- Parent-directory GIO monitoring with a 180 ms debounce survives atomic replacement saves.
  Read/UTF-8/size errors retain the last-good document and recover on later valid saves.
- Approximate pixel scroll retention. Selection is not retained across buffer replacements;
  inserting text above the viewport is not compensated with a semantic scroll anchor.

HTTP(S)/mailto links use the system handler. Other schemes and local non-Markdown links inside
the reader are refused; open reviews explicitly with `smad-axi open lavish://...` instead.
There is no Back/Forward history; use explicit return links or the document list.

## Source-line highlights

```zsh
smad-axi open 'md://./plan.md?highlight=12-24,48'
smad-axi open 'md://./plan.md?highlight=12-24#decisions'
```

Ranges are original 1-based inclusive **source lines**, not visual rows. Paragraphs,
quotes and lists highlight only the rendered text from those lines, including decoded
entities and normalized multiline inline code. Fenced/indented/list-nested code highlights
exact code lines. Blank rendered code lines have no visible glyph/marker and produce an
explicit warning rather than a false visual-success claim. Tables select
individual rows. Any line of a Mermaid section or image selects its entire native widget.
Failed diagrams retain exact source-code fallback highlighting.

Use the Source highlights field (Enter applies), Previous/Next, or Clear. Ranges persist
with the document and are rebuilt against the current file on live updates; they do not
track text moved to other line numbers. Reopening without a highlight query clears them.
Heading fragments retain scroll precedence when both a fragment and ranges are supplied.

Invisible source syntax has no automatic neighbouring/block fallback: blank source lines,
reference definitions, ordinary code fences, table delimiters and setext underlines may be
unmapped. Unknown multiline transformations are not guessed. These cases produce a visible
warning and AXI `ready_with_warnings` (first 16 unmapped coordinates reported). The renderer
does not add new dialect features such as math or footnotes just to map their source.

The additive protocol-v2 capability `markdown_source_ranges_v1` gates highlighted Markdown
opens. AXI verifies request identity as well as readiness on supporting servers and rejects
older servers for highlighted opens instead of claiming success.

## Background work and resource policy

Merman runs on one shared background worker with an eight-job queue, cooperative cancellation,
finite interactive resource policy, and per-document/per-diagram deadlines. New revisions cancel
old work, and only the current revision may replace the buffer. Closing/suspending a view cancels
its pending work. A bounded 8 MiB/16-entry cache retains successful SVG results, not failed ones.

SVG uses Merman's **resvg-safe** pipeline: HTML foreign objects are removed and structural/image
resources are constrained before the native loader sees the output. Optional ELK and math/font
features are not compiled; requesting those capabilities produces a diagnostic, never an
unbounded retry or silent layout substitution. The implementation does not promise Mermaid.js
pixel parity, interactive diagram links, diagram-text selection, zoom controls, or editable
whiteboards. The original Mermaid source remains copyable.

Reader limits:

- 1 MiB UTF-8 Markdown, 20,000 source lines, 20,000 parser events, 16 list levels and 256 links.
- Source ranges: 64 normalized ranges, 2,048 query bytes, coordinates through 1,000,000.
  Mapping work is bounded by source/event limits; adjacent tag applications are coalesced.
- Native tables: 1,000 rows, 12 columns, 4,096 total cells, 256 characters per cell.
- Highlighting: up to 64 KiB per code block and 4,096 bytes per line; larger blocks use plain text.
- Mermaid: eight diagrams per document, 32 KiB source and 2 MiB SVG per diagram; four-second
  document and two-second diagram execution deadlines. These deadlines are cooperative, not
  forced thread interruption. Raster dimensions are bounded to 1000 × 1000.
- Local images: 32 per document, 8 MiB/file, source dimensions at most 8192 × 8192, displayed
  within 720 × 480. Asset changes alone are not watched; explicit Reload refreshes assets.

These are host guardrails, not a hostile-file sandbox. GTK widget creation/highlighting and
bounded local-file reads still run on the GTK thread. Tables containing images expose their
alt text rather than nested image widgets. Footnotes, math, bare-URL autolinking, editing and
complete GFM conformance are not implemented. Screen-reader interoperability and long-duration
resource behavior need further human/soak testing.

## Verification and examples

```zsh
nix-shell --run './scripts/check.sh'
nix-shell --run 'python3 tests/e2e/native_markdown.py'
smad-axi open md://./tests/fixtures/markdown/features.md
```

`features.md` includes highlighted code, a rich aligned table, two native diagrams and one
**deliberately invalid diagram** to demonstrate explicit warnings. `reader.md` is the broader
reading fixture; `showcase.md` records the earlier POC tour and its former limitations.

The GUI test uses an isolated Xvfb display, private bus/socket and temporary XDG state. It checks
selection/copy, native table allocation, local SVG decoding, link-menu routing, atomic saves,
deletion/recovery, scroll retention, 30 updates, latest-revision cancellation and watcher cleanup.
It also launches the real server/CLI, verifies duplicate reuse, no WebKit web subprocess for
Markdown-only content, no-raise HerdR rename/select, and a Ready WebKit review fixture.

Evidence in ignored `target/markdown-poc/` includes `reader.png`, `renderer-features.png`,
`mermaid-diagrams.png`, `native-test.log`, `application-state.json` and `memory.txt`.
RSS measurements include GTK/shared-library/software-renderer startup; they are not PSS, a
long-duration leak benchmark, or a promise of a minimal total process footprint.

Existing Lavish reviews keep their independent WebKit path. Full upstream review/export/
whiteboard validation is separate from this native-renderer test.
