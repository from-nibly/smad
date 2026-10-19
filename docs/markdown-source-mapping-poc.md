# Native Markdown source-line mapping POC

Discovery record for the mapping prototype, promoted to the supported v0.5.0 feature.
See [current URI/protocol semantics](native-markdown.md#source-line-highlights). The isolated
preview launcher remains available; it does not modify the installed browser.

## Try it

```sh
nix-shell --run 'cargo build -p smad -p smad-cli && python3 tests/e2e/markdown_mapping_preview.py'
```

This opens **SMAD — Markdown mapping POC (isolated)** with a private D-Bus session, state/data/cache/runtime directories and control socket. Closing its window ends that private application session. Preview identity/log location is recorded in `target/markdown-poc/mapping-preview.json`.

The sample is `tests/fixtures/markdown/source-mapping.md`. The POC toolbar accepts original, 1-based inclusive line ranges; press Enter to apply and jump to the first match. Empty input clears highlights. Expand the numbered-source inspector to find lines without switching documents. Highlight ranges stay absolute on live edits; they do not track moved text.

## Findings

- **Merged paragraphs:** precise. Only the rendered characters originating on the selected source line are highlighted, including when that text wraps over multiple display rows. Other source lines in the same paragraph remain unhighlighted.
- **Inline formatting, links, escapes and entities:** line provenance survives the transformation. `&amp;` becomes one highlighted `&`; formatting delimiters themselves disappear. Unicode and CRLF are covered.
- **Multiline inline code:** precise across CommonMark's newline-to-space normalization and single-edge-space trimming. This needs explicit normalization/provenance rather than guessing rendered offsets.
- **Fenced, indented, and list-nested code:** exact logical code lines, accounting for stripped indentation and syntax tags. Unknown grammars retain mapping in plain text.
- **Lists, tasks and quotations:** text on continuation lines can be highlighted independently. Generated list markers belong to the item's first source line.
- **GFM tables:** individual rendered rows, not the whole table. The current dialect places each row on one source line; every native cell in that row receives an outline/tint.
- **Mermaid:** any line in the fenced diagram selects the whole native diagram/source-expander widget. Failed diagrams retain exact source-code fallback mapping rather than assigning the generated error message to an arbitrary line.
- **Images:** the whole native picture/caption widget is highlighted for its source span, including reference images.
- **Raw HTML:** remains inert literal text; mapping does not introduce a browser or execute HTML.

## Unresolved policy / production work

Blank source lines, reference definitions, ordinary code fences, table delimiter rows and setext heading underlines can have no visible text counterpart. The POC reports these as **unmapped** and does not silently choose a neighbour. This is not necessarily a parser limitation: we could map structural syntax to its owning block, or reference definitions to their usages, but should explicitly choose that behavior.

Blank *code* lines have newline provenance, but GTK did not visibly paint the tested paragraph-background tag. v0.5.0 explicitly warns that these lines lack a visible marker; a separate visual adornment remains future work. Unsupported dialect extensions (math, footnotes, definition lists) remain subject to the existing parser's literal/reference behavior, not newly rendered features.

Unrecognized multiline transformations are reported as ambiguous instead of being assigned an approximate text range. v0.5.0 adds source/event work bounds, coalesced tag updates, URI/capability integration, navigation and persistence. Broader CommonMark/adversarial coverage and human accessibility/soak testing remain worthwhile. The POC uses immutable-buffer character offsets rebuilt on each successful render, not durable source anchors across revisions.

## Evidence

`nix-shell --run 'python3 tests/e2e/native_markdown.py'` now includes the mapping GUI test as its own GTK process. It verifies exact selected text, row/widget selection, clearing, CRLF/Unicode, diagram fallback, and atomic-save remapping. Existing Markdown, source/comparison, native-only/no-WebKit, and review/HerdR smoke gates remain included.

Screenshots, inspected during development:

- `target/markdown-poc/mapping-paragraph.png`
- `target/markdown-poc/mapping-code.png`
- `target/markdown-poc/mapping-widgets.png`
