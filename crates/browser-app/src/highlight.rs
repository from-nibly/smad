use std::{path::Path, sync::LazyLock};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(|| {
    syntect::dumps::from_binary(include_bytes!(concat!(
        env!("OUT_DIR"),
        "/syntaxes.packdump"
    )))
});
static THEMES: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

fn fence_syntax(language: &str) -> Option<&'static SyntaxReference> {
    let token = language
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let token = match token.as_str() {
        "sh" | "shell" | "shellscript" | "zsh" => "bash",
        "js" | "javascript" | "jsx" => "JavaScript (Babel)",
        "ts" | "typescript" => "TypeScript",
        "tsx" | "typescriptreact" => "TypeScriptReact",
        "c++" | "cplusplus" => "cpp",
        "c#" | "csharp" => "C#",
        "f#" | "fsharp" => "F#",
        "terraform" | "opentofu" => "tf",
        "protobuf" | "protocolbuffers" => "proto",
        "docker" | "containerfile" => "Dockerfile",
        "pwsh" | "powershell" => "PowerShell",
        "nushell" => "nu",
        "yml" => "yaml",
        "py" | "python3" => "python",
        "rb" => "ruby",
        "rs" => "rust",
        "golang" => "go",
        "kt" => "kotlin",
        "cs" => "C#",
        "fs" => "F#",
        "ex" | "exs" => "elixir",
        "erl" => "erlang",
        "hs" => "haskell",
        "clojurescript" | "edn" => "clojure",
        "jsonc" | "json5" => "JSON5",
        "bazel" | "starlark" => "Python",
        "scheme" | "elisp" | "emacs-lisp" | "common-lisp" => "Lisp",
        "tla" | "tlaplus" => "TLA+",
        "org" => "orgmode",
        "gdscript" => "gd",
        "batch" | "cmd" | "dos" => "bat",
        "vb" | "vbnet" | "visualbasic" | "vba" => "Visual Basic",
        "plsql" | "postgresql" | "mysql" | "tsql" => "SQL",
        "objc" => "Objective-C",
        "objective-cpp" => "Objective-C++",
        "f90" | "fortran" => "Fortran (Modern)",
        "assembly" | "asm" => "x86_64 Assembly",
        "reason" => "ReasonML",
        "gotemplate" | "go-template" | "gotmpl" => "Go Template",
        "helm" => "Helm",
        other => other,
    };
    SYNTAXES
        .find_syntax_by_name(token)
        .or_else(|| SYNTAXES.find_syntax_by_token(token))
}

fn file_syntax(path: &Path, code: &str) -> Option<&'static SyntaxReference> {
    let name = path.file_name()?.to_str()?;
    if path.extension().is_some_and(|e| e == "C") {
        return fence_syntax("cpp");
    }
    if path.extension().is_some_and(|e| e == "fs") {
        return fence_syntax("fsharp");
    }
    if path.extension().is_some_and(|e| e == "cls") {
        let vba = code.lines().take(12).any(|line| {
            let line = line.trim_start().to_ascii_lowercase();
            line.starts_with("version 1.0 class") || line.starts_with("attribute vb_")
        });
        return SYNTAXES.find_syntax_by_name(if vba { "Visual Basic" } else { "TeX" });
    }
    if matches!(
        name,
        "BUILD" | "BUILD.bazel" | "WORKSPACE" | "WORKSPACE.bazel"
    ) {
        return fence_syntax("starlark");
    }
    if name.starts_with("Dockerfile.")
        || name == "Containerfile"
        || name.starts_with("Containerfile.")
    {
        return fence_syntax("dockerfile");
    }
    if name.starts_with(".env.") {
        return fence_syntax("env");
    }
    // Exact names and compound suffixes precede the last extension (CMakeLists.txt,
    // Cargo.lock, go.mod, *.html.j2, *.pb.txt). Preserve case for .C vs .c.
    std::iter::once(name)
        .chain(name.match_indices('.').map(|(i, _)| &name[i + 1..]))
        .find_map(|suffix| SYNTAXES.find_syntax_by_extension(suffix))
        .or_else(|| SYNTAXES.find_syntax_by_first_line(code.lines().next().unwrap_or("")))
}

pub fn code_spans<'a>(code: &'a str, language: &str, dark: bool) -> Vec<(Option<Style>, &'a str)> {
    spans(code, || fence_syntax(language), dark)
}

pub fn file_spans<'a>(code: &'a str, path: &Path, dark: bool) -> Vec<(Option<Style>, &'a str)> {
    spans(code, || file_syntax(path, code), dark)
}

fn spans(
    code: &str,
    syntax: impl FnOnce() -> Option<&'static SyntaxReference>,
    dark: bool,
) -> Vec<(Option<Style>, &str)> {
    // Check limits before resolving syntax, including first-line regex detection.
    if code.len() > 64 * 1024 || code.lines().any(|line| line.len() > 4096) {
        return vec![(None, code)];
    }
    // Some regex-engine errors panic rather than returning a parsing error.
    // Discard the document-local parser on unwind; never take down the reader.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Some(syntax) = syntax() else {
            return vec![(None, code)];
        };
        let theme = &THEMES.themes[if dark {
            "base16-ocean.dark"
        } else {
            "InspiredGitHub"
        }];
        let mut highlighter = HighlightLines::new(syntax, theme);
        let mut spans = Vec::new();
        for line in LinesWithEndings::from(code) {
            match highlighter.highlight_line(line, &SYNTAXES) {
                Ok(parts) => {
                    spans.extend(parts.into_iter().map(|(style, text)| (Some(style), text)))
                }
                Err(_) => spans.push((None, line)),
            }
        }
        spans
    }))
    .unwrap_or_else(|_| vec![(None, code)])
}

#[cfg(test)]
#[path = "highlight_tests.rs"]
mod tests;
