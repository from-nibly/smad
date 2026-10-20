# Syntax highlighting coverage

SMAD bundles **252 syntax definitions**, up from Syntect's original 75. This is
not a claim of 252 distinct languages: the collection includes embedded grammars,
file formats, build output and helper contexts. The [complete generated catalog](syntax-catalog.md)
records every definition and its filename/extension associations.

The same bundle serves native `code://` views, full-file comparisons and Markdown
code fences. No grammars are downloaded at runtime and no JavaScript renderer,
Tree-sitter, editor installation or language server is required.

## The coverage sweep

`tests/fixtures/syntaxes/coverage.json` contains **133 executable cases** covering
programming languages, shells, infrastructure/configuration, data/query languages,
markup, build files, shaders and hardware description languages. Each case checks
both filename and Markdown fence resolution, lossless source rendering, absence
of regex failures in both themes, and distinct styles in at least one theme.
Some markup scopes are deliberately monochrome in the existing light theme.
CUE, Nix, Terraform/HCL and other priority languages additionally require multiple
foreground colors in *both* themes. Native GTK tests capture CUE/Nix/Terraform
in light and dark mode with full-width attention highlights.

Examples covered include C/C++/C#/Java, JS/JSX/TypeScript/TSX, Python, Go, Rust,
Ruby, PHP, Swift, Kotlin, Scala, Dart, R, Julia, Lua, Perl/Raku, Bash/Zsh/Fish,
PowerShell, Nushell, Elixir/Erlang, Haskell, F#, OCaml, ReasonML/ReScript,
Clojure/EDN, Lisp/Scheme/Racket, Elm/PureScript, Gleam, Zig/Nim/Crystal/Odin,
Ada/Fortran/Pascal/COBOL/Visual Basic/Smalltalk/Forth, ABAP/SAS/Stata, Haxe,
Agda/Idris/Lean, CUE/Nix/Terraform/HCL/Bicep/Jsonnet/Dhall/Rego/Puppet,
YAML/JSON/JSON5/JSONC/TOML/XML, HTML/CSS/SCSS/Sass/Less/Stylus/Vue/Svelte/QML,
SQL/GraphQL/Protobuf/JQ, Make/CMake/Meson/Just/Ninja/Dockerfile/Starlark,
Verilog/SystemVerilog/VHDL, GLSL/HLSL/WGSL/CUDA, GDScript, Solidity/Vyper,
Markdown/LaTeX/Typst/Org/AsciiDoc/reStructuredText, TLA+, Astro/MDX, Go templates,
Helm templates, Liquid/Handlebars, Pug and Mermaid source.

Popularity has no stable cutoff at CUE's level. This is an explicit, tested sweep,
not a promise to support every language, dialect, extension or editor plugin.
Unknown languages remain readable plain text. Extend the manifest when a new
language is added, rather than silently aliasing it to an unrelated grammar.

## Sources and accuracy

- **220 maintained upstream definitions:** pinned
  `two-face 0.5.2+bat-0.26.1`, the `bat` collection packaged for Syntect.
- **Five pinned community supplements:** Nushell, Gleam, Dhall, Meson and TLA+.
  `assets/syntaxes/upstream.json` records repository, exact revision, original
  path, SHA-256 and license location. These files are copied without modification.
- **CUE:** SMAD-authored lexical rules for configuration CUE, not CD cue sheets:
  definitions/constraints, keywords/types, numbers, comments, quoted/raw/multiline
  strings and interpolation. The build enables Syntect's parent-match backreference
  flag for CUE escape/interpolation rules as well as closing delimiters, so raw
  strings honor the exact opening hash count. Regression tests cover active and
  inactive interpolation markers. This is highlighting, not CUE validation or evaluation.
- **Basic lexical supplements:** ABAP, Agda, Bicep, COBOL, Forth, Haxe, JSON5/JSONC,
  Just, Prolog, Raku, ReasonML, ReScript, SAS, Smalltalk, Stata and Visual Basic.
  These distinguish common keywords, constants, comments, strings, numbers and
  operators. They do **not** promise full dialect support, embedded-language
  parsing, macro expansion or validation. CUDA/HLSL add vocabulary over C++.
  Astro, MDX, Go/Helm templates, Liquid, Handlebars, Pug and Mermaid source also
  have basic lexical overlays. Host/embedded language interactions are not full
  framework parsers; ordinary `.yaml` files still use YAML, not a guessed Helm mode.
  Markdown Mermaid fences still use the existing native diagram renderer.
  They are intentionally identified as basic rather than presented as upstream
  editor-quality grammars.
- Starlark/Bazel uses the Python lexical grammar; Scheme/Emacs Lisp use Lisp;
  SQL dialect fence aliases use the general SQL grammar. These are language-family
  highlighting choices, not dialect validation.

Syntect's **Oniguruma** backend is used because the pure-Rust fancy-regex variant
of the upstream collection omits PowerShell, Sass, ARM assembly and other grammars.
Oniguruma builds from the locked vendored source; no new system shared library is
required by the normal build. The syntax set is linked and serialized at build
time to avoid rebuilding hundreds of grammar contexts on first document render.

## Selection rules and limits

- Fence language names are case-insensitive; aliases include `ts`, `tsx`, `js`,
  `jsx`, `sh`, `shell`, `pwsh`, `csharp`, `fsharp`, `golang` and `opentofu`.
  TypeScript now uses its own grammar, **not JavaScript**.
- Code/comparison views try the entire basename and then compound suffixes before
  the final extension: `Dockerfile`, `Dockerfile.dev`, `Containerfile`,
  `CMakeLists.txt`, `Makefile`, `Cargo.lock`, `go.mod`, `.env.local`, `.gitignore`,
  `page.html.j2`, `record.pb.txt`, `BUILD.bazel`, etc.
- An otherwise unrecognized file can use a recognized first line (e.g. a Python
  shebang). Explicit unknown Markdown fence names stay plain; they are not guessed.
- `.C` selects C++, `.c` C, `.fs` F#. Other inherently ambiguous extensions retain
  the catalog's defaults: for example `.m` selects Objective-C, not MATLAB;
  MATLAB is available as a `matlab` fence or `.matlab` file. `.cls` retains TeX
  unless a VBA class export header identifies Visual Basic. File grammar override
  UI/URI options are not introduced by this change.
- The existing 64-KiB and 4096-byte-per-line syntax limits remain. Larger sources
  stay fully readable but skip syntax coloring. This is not a parser-level wall
  clock guarantee. Returned parsing errors fall back losslessly for that line;
  a backend panic discards the document-local parser and preserves the whole source
  as plain text. Static grammar regexes are also validated at build time.
- Themes and full-line attention backgrounds remain unchanged. Highlighting never
  changes source text, copied text, original coordinates or snapshot contents.

## Maintenance and verification

Run `nix-shell --run './scripts/check.sh && python3 tests/e2e/native_markdown.py'`.
The test suite checks that the embedded upstream notices match the locked bundle
and that supplemental grammars introduce no unresolved include contexts.

After an intentional bundle/grammar update, regenerate the catalog with:

```sh
SMAD_UPDATE_SYNTAX_CATALOG=1 cargo test -p smad highlight::tests::catalog_is_current
```

Review the resulting catalog and coverage diff. Update upstream provenance hashes
and license notices alongside any vendored changes. No runtime grammar fetching
or unpinned build-time network downloads are allowed.

Attribution ships in `licenses/two-face-0.5.2/THIRD_PARTY_NOTICES.md`,
`licenses/syntaxes/`, and the Cargo `onig_sys` attribution directory (including
its nested `oniguruma/COPYING`), alongside the existing Cargo license inventory. No GPL/AGPL definitions or project license
change are included.
