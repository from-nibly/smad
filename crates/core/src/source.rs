//! Bounded, UTF-8 source inputs with explicit byte and character coordinates.
use std::io::Read;
use std::ops::Range;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

pub const MAX_SOURCE_BYTES: usize = 1024 * 1024;
pub const MAX_SOURCE_LINES: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLine {
    pub bytes: Range<usize>,
    pub content_end: usize,
    pub ending: LineEnding,
    /// Character coordinates in LF-normalized display text (not UTF-8 bytes).
    pub display_chars: Range<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceText {
    pub text: String,
    pub lines: Vec<SourceLine>,
}

impl SourceText {
    pub fn parse(text: String) -> Result<Self, String> {
        if text.len() > MAX_SOURCE_BYTES {
            return Err("source exceeds the 1 MiB limit".into());
        }
        if text
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\t' | '\r' | '\n'))
        {
            return Err(
                "source contains binary/control bytes; only UTF-8 text is supported".into(),
            );
        }
        let mut lines = Vec::new();
        let mut byte = 0;
        let mut character = 0;
        for line in text.split_inclusive('\n') {
            if lines.len() == MAX_SOURCE_LINES {
                return Err("source exceeds the 20,000-line limit".into());
            }
            let (content, ending) = if let Some(content) = line.strip_suffix("\r\n") {
                (content, LineEnding::CrLf)
            } else if let Some(content) = line.strip_suffix('\n') {
                (content, LineEnding::Lf)
            } else {
                (line, LineEnding::None)
            };
            if content.contains('\r') {
                return Err("bare CR line endings are unsupported; use LF or CRLF".into());
            }
            let end =
                character + content.chars().count() as i32 + i32::from(ending != LineEnding::None);
            lines.push(SourceLine {
                bytes: byte..byte + line.len(),
                content_end: byte + content.len(),
                ending,
                display_chars: character..end,
            });
            byte += line.len();
            character = end;
        }
        Ok(Self { text, lines })
    }

    pub fn content(&self, index: usize) -> &str {
        let line = &self.lines[index];
        &self.text[line.bytes.start..line.content_end]
    }

    pub fn raw_line(&self, index: usize) -> &str {
        &self.text[self.lines[index].bytes.clone()]
    }

    pub fn display_text(&self) -> String {
        self.text.replace("\r\n", "\n")
    }
}

pub fn canonical_source(path: &Path) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if !canonical.is_file() || canonical.to_str().is_none() {
        return Err("expected a regular source file with a UTF-8 path".into());
    }
    Ok(canonical)
}

/// Resolve a source identity even when a comparison's working-tree file is deleted.
pub fn canonical_location(path: &Path) -> Result<PathBuf, String> {
    if path.to_str().is_none_or(|path| path.contains('\0')) {
        return Err("source paths must be UTF-8 without NUL bytes".into());
    }
    if path.exists() {
        return canonical_source(path);
    }
    let mut ancestor = std::path::absolute(path).map_err(|error| error.to_string())?;
    let mut missing = Vec::new();
    while !ancestor.exists() {
        missing.push(
            ancestor
                .file_name()
                .ok_or("cannot resolve missing path components; use a normalized path")?
                .to_owned(),
        );
        if !ancestor.pop() {
            return Err("source has no existing ancestor".into());
        }
    }
    let mut resolved = ancestor.canonicalize().map_err(|error| error.to_string())?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    if resolved.to_str().is_none() {
        return Err("source paths must be UTF-8".into());
    }
    Ok(resolved)
}

pub fn read_source(path: &Path) -> Result<SourceText, String> {
    // Atomic replacement must not turn a reload into a blocking FIFO open.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_SOURCE_BYTES as u64 {
        return Err("expected a regular UTF-8 source file up to 1 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let text = String::from_utf8(bytes).map_err(|_| "source is not UTF-8 text")?;
    SourceText::parse(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_crlf_and_final_newlines_have_distinct_coordinates() {
        let source = SourceText::parse("é🦀\r\n\r\nx".into()).unwrap();
        assert_eq!(source.lines.len(), 3);
        assert_eq!(source.lines[0].bytes, 0..8);
        assert_eq!(source.lines[0].display_chars, 0..3);
        assert_eq!(source.lines[1].display_chars, 3..4);
        assert_eq!(source.lines[2].display_chars, 4..5);
        assert_eq!(source.content(0), "é🦀");
        assert_eq!(source.content(1), "");
        assert_eq!(source.lines[2].ending, LineEnding::None);
        assert_eq!(source.display_text(), "é🦀\n\nx");
        assert_eq!(SourceText::parse("x\n".into()).unwrap().lines.len(), 1);
        assert!(SourceText::parse(String::new()).unwrap().lines.is_empty());
        assert_eq!(SourceText::parse("\n".into()).unwrap().lines.len(), 1);
    }
    #[test]
    fn rejects_binary_and_bounded_work_violations() {
        for source in ["x\0y", "x\x1by", "old\rmac"] {
            assert!(SourceText::parse(source.into()).is_err());
        }
        assert!(SourceText::parse("x".repeat(MAX_SOURCE_BYTES + 1)).is_err());
        assert!(SourceText::parse("\n".repeat(MAX_SOURCE_LINES + 1)).is_err());
        assert!(SourceText::parse("\n".repeat(MAX_SOURCE_LINES)).is_ok());
    }
}
