# Native source and whole-file comparisons

SMAD renders code and comparisons with GTK TextView, a separate native gutter, and
Syntect. These surfaces never use WebKit or HTML. They are read-only: no editing,
patch application or implicit Git mutation.

## Whole source files

```zsh
smad-axi open 'code://./src/main.rs?highlight=12-24,48,70-85'
smad-axi open 'code:///absolute/path/a%3F%23.rs?highlight=2-4'
```

The entire accepted file is shown. Line numbers are original 1-based source lines,
not visual rows; the source view scrolls horizontally rather than wrapping.
The gutter is outside the text buffer, so selection/copy excludes line numbers.
CRLF is displayed/copied as LF; original line endings and a missing final newline
are reported. Empty files have zero source lines, not a synthetic source line.

`highlight` accepts inclusive ranges and individual lines, sorts them, and merges
overlap/adjacency. Zero, reversed, malformed and excessive ranges are rejected.
There is one query parameter, `highlight`; repeated/unknown keys are errors.
Encode literal `?`, `#` and `%` in filenames. Code URIs do not accept fragments.

Agent attention uses an amber gutter marker and double underline, separately from
syntax colors, red/green change backgrounds and normal GTK text selection.
Opening scrolls to the first highlighted range. Previous/next and Clear buttons
are keyboard accessible. Out-of-bounds ranges produce a visible/API warning and
emphasize only existing lines; they never refer to wrapped/display rows.

Opening the same path/project/renderer reuses the document entry and replaces its
ranges. Opening without ranges clears the previous request's ranges. Markdown,
code and comparison views of one file may coexist; selection includes renderer
identity. Source highlights are persisted with the document.

## Full-file unified comparisons

```zsh
# Exact base, captured once; live working-tree after side:
smad-axi compare ./src/main.rs --base HEAD --highlight 12-24,48

# Two exact immutable revisions (not implicitly a PR merge base):
smad-axi compare ./src/main.rs --base main --head feature

# Explicit PR-style single merge base; both sides are immutable:
smad-axi compare ./src/main.rs --base main --head feature --merge-base

# Two immutable file snapshots:
smad-axi compare --old ./before.rs --new ./after.rs --highlight-old 3-8 --highlight 4-12

# Explicitly absent file side:
smad-axi compare --old /dev/null --new ./added.rs
smad-axi compare --old ./deleted.rs --new /dev/null
```

There is **no `diff://` scheme**. Comparisons consume complete inputs, not patches.
Every unchanged line remains visible. The left gutter column is the old source
line, the right column is the new source line; removed rows have only old numbers,
added rows only new numbers. Modifications appear as removed/added blocks. Empty
changed lines remain identifiable by their colored gutter and +/− marker even
when GTK does not paint an empty paragraph background. Previous/next change
buttons navigate changed blocks. There is no inline word diff or move detection.

`--highlight` uses **new-side** source coordinates; `--highlight-old` explicitly
uses old-side coordinates. Neither counts unified display rows. Syntax is computed
independently for each complete source before mapping it to displayed rows, so
removed comments/strings do not alter the other version's syntax state.

The CLI reports resolved commit IDs and exact-base/merge-base mode. `--merge-base`
requires an explicit `--head` and exactly one merge base; ambiguous histories
must use an explicit base. Git commands use separate arguments, literal pathspecs,
no external diff/textconv/filter pipeline, bounded output and a 10-second
per-command deadline. Lazy fetching is disabled where supported, and all Git network
transports and terminal prompts are disabled, including with older Git. Required objects
must already be local. No shell interpolation, worktree edits, commits or branch changes
are performed.

Before/after snapshots are captured into a content-addressed private store before
acknowledgment. Closing the CLI or changing source files/branches cannot change
immutable comparisons. Missing/corrupt snapshots fail visibly rather than silently
substituting current file contents. A working-tree comparison reloads only its
after path; moving HEAD does not change its pinned base. Reopening that comparison
with a new request deliberately replaces its inputs, in the same document entry.

File addition/deletion, empty files, and no-change comparisons are distinguished
in the status. `/dev/null` explicitly denotes absence in file-snapshot mode;
other missing `--old`/`--new` inputs are errors. A missing Git tree entry or live
working-tree path is an absent side. Git symlinks/submodules are not supported.

## Live updates, bounds and limitations

- UTF-8 regular files only, at most 1 MiB and 20,000 source lines per input.
  Binary/control bytes and bare-CR line endings are rejected; LF/CRLF are supported.
- At most 64 ranges per side, 2,048 bytes per range string, coordinates up to
  1,000,000. Actual file bounds are checked when rendering.
- Source syntax uses filename-extension detection and existing Syntect grammars.
  Unknown languages show plain text. Inputs over 64 KiB or with a line over 4,096
  bytes skip syntax work with a warning, but retain the whole accepted source.
- Histogram diff work is bounded by a 16-million changed-region line-pair budget
  after trimming equal prefix/suffix for computation only. All trimmed context is
  still displayed. Excess work fails explicitly, not by displaying an incomplete patch.
- One background source worker with a one-job queue. Each view retains only its
  latest pending request; obsolete revisions are discarded. Work bounds are not
  a hard real-time guarantee for filesystem IO or OS scheduling.
- Parent-directory monitoring and 180 ms debounce handle atomic file saves.
  Failed reloads retain the last good display, report failure, and can recover.
  Live working-tree deletion is shown as deletion. If the parent directory itself
  disappears, monitoring may require reopening; the visible warning offers Reload.
- Live refresh preserves approximate pixel scroll, not semantic diff anchors.
  Reload replaces the text buffer and may replace text selection.
- Light/dark syntax colors are adjusted for contrast against native/diff backgrounds.
  Reopen the view after changing the system theme to select the other palette.
- Snapshot storage is capped at 128 MiB / 4,096 entries. Snapshots are not automatically
  garbage-collected. Removing snapshots with SMAD closed can make saved comparisons
  unavailable; retain the snapshot directory with state backups.
- Unified view only. Side-by-side, inline word changes, move detection, editing,
  and patch application are not implemented.
- Markdown source ranges are supported separately: see [native Markdown mapping](native-markdown.md#source-line-highlights).

The server advertises `source_documents_v1`; AXI rejects older running servers for
these commands instead of claiming a successful open. Existing protocol-v2 review,
Markdown and HerdR commands retain their behavior. Older SMAD binaries cannot read
new document kinds: checkpoint state and snapshots before upgrading/downgrading.

## Verification

`nix-shell --run './scripts/check.sh'` runs formatting, warning-denied Clippy, unit /
protocol / migration / exact Git tests and the WASI plugin build.
`nix-shell --run 'python3 tests/e2e/native_markdown.py'` uses a private display, bus,
socket and state. GTK tests run in separate processes (GTK thread affinity), verify
selection, dual numbering, ranges/navigation/clear, live atomic saves, fixed base,
scroll retention, deletion/recovery and cleanup. Application checks exercise AXI
routing/reuse, file/Git snapshots vs live content, no native-only WebKit child,
and existing Markdown/Lavish/HerdR behavior. Light/dark evidence is written under
`target/markdown-poc/source-*.png`; these are test artifacts, not shipped resources.
