use std::sync::LazyLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEMES: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

pub fn code_spans<'a>(code: &'a str, language: &str, dark: bool) -> Vec<(Option<Style>, &'a str)> {
    if code.len() > 64 * 1024 || code.lines().any(|line| line.len() > 4096) {
        return vec![(None, code)];
    }
    let token = language.split_whitespace().next().unwrap_or("");
    let token = match token {
        "sh" | "shell" | "zsh" => "bash",
        "js" => "javascript",
        "ts" | "typescript" => "javascript",
        other => other,
    };
    let Some(syntax) = SYNTAXES
        .find_syntax_by_token(token)
        .or_else(|| SYNTAXES.find_syntax_by_extension(token))
    else {
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
            Ok(parts) => spans.extend(parts.into_iter().map(|(style, text)| (Some(style), text))),
            Err(_) => spans.push((None, line)),
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn highlighting_preserves_source_and_uses_multiple_styles() {
        let code = "fn main() { println!(\"hello\"); }\n";
        let spans = code_spans(code, "rust", false);
        assert_eq!(
            spans.iter().map(|(_, text)| *text).collect::<String>(),
            code
        );
        let styles: std::collections::HashSet<_> = spans
            .iter()
            .filter_map(|(style, _)| {
                style.map(|s| (s.foreground.r, s.foreground.g, s.foreground.b))
            })
            .collect();
        assert!(styles.len() > 1);
        assert!(code_spans(code, "unknown-syntax", false)[0].0.is_none());
    }
}
