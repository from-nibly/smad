//! Source-line provenance for immutable Markdown render buffers (rebuilt per revision).
use gtk::prelude::*;
use smad_protocol::LineRange;
use std::ops::Range;

#[derive(Debug)]
pub struct Span {
    pub lines: Range<usize>,
    pub chars: Range<i32>,
}

#[derive(Default)]
pub struct SourceMap {
    pub spans: Vec<Span>,
    pub widgets: Vec<(Range<usize>, gtk::Widget)>,
    pub ambiguous: Vec<Range<usize>>,
    css: std::cell::RefCell<Option<gtk::CssProvider>>,
}

pub struct Lines(Vec<usize>);
impl Lines {
    pub fn new(source: &str) -> Self {
        Self(
            std::iter::once(0)
                .chain(source.match_indices('\n').map(|(i, _)| i + 1))
                .collect(),
        )
    }
    pub fn at(&self, byte: usize) -> usize {
        self.0.partition_point(|start| *start <= byte)
    }
    pub fn range(&self, bytes: Range<usize>) -> Range<usize> {
        self.at(bytes.start)..self.at(bytes.end.saturating_sub(1).max(bytes.start)) + 1
    }
}

impl SourceMap {
    pub fn atomic(&mut self, lines: &Lines, bytes: Range<usize>, chars: Range<i32>) {
        if !chars.is_empty() {
            self.spans.push(Span {
                lines: lines.range(bytes),
                chars,
            });
        }
    }

    // Markdown transformations within a source line do not affect line provenance.
    // Preserve per-line boundaries even when indentation or CRLF has been removed.
    pub fn text(
        &mut self,
        lines: &Lines,
        source: &str,
        bytes: Range<usize>,
        text: &str,
        start: i32,
    ) {
        let raw = &source[bytes.clone()];
        let input: Vec<_> = raw.split_inclusive('\n').collect();
        let output: Vec<_> = text.split_inclusive('\n').collect();
        if input.len() == output.len() {
            let mut byte = bytes.start;
            let mut cursor = start;
            for (raw, rendered) in input.into_iter().zip(output) {
                let end = cursor + rendered.chars().count() as i32;
                self.atomic(lines, byte..byte + raw.len(), cursor..end);
                byte += raw.len();
                cursor = end;
            }
        } else if lines.range(bytes.clone()).len() == 1 {
            self.atomic(lines, bytes, start..start + text.chars().count() as i32);
        } else {
            // Do not invent a precise mapping when a transformation is not understood.
            self.ambiguous.push(lines.range(bytes));
        }
    }

    pub fn inline_code(
        &mut self,
        lines: &Lines,
        source: &str,
        bytes: Range<usize>,
        text: &str,
        start: i32,
    ) {
        let raw = &source[bytes.clone()];
        let ticks = raw.chars().take_while(|c| *c == '`').count();
        if ticks == 0 || raw.len() < ticks * 2 {
            self.text(lines, source, bytes, text, start);
            return;
        }
        let body = &raw[ticks..raw.len() - ticks];
        let mut chars = Vec::new();
        let mut iter = body.char_indices().peekable();
        while let Some((offset, ch)) = iter.next() {
            let line = lines.at(bytes.start + ticks + offset);
            if ch == '\r' && iter.peek().is_some_and(|(_, next)| *next == '\n') {
                iter.next();
                chars.push((' ', line));
            } else {
                chars.push((if ch == '\n' { ' ' } else { ch }, line));
            }
        }
        if chars.first().is_some_and(|(c, _)| *c == ' ')
            && chars.last().is_some_and(|(c, _)| *c == ' ')
            && chars.iter().any(|(c, _)| *c != ' ')
        {
            chars.remove(0);
            chars.pop();
        }
        if chars.iter().map(|(c, _)| *c).collect::<String>() != text {
            self.ambiguous.push(lines.range(bytes));
            return;
        }
        for (offset, (_, line)) in chars.into_iter().enumerate() {
            let cursor = start + offset as i32;
            if let Some(last) = self.spans.last_mut()
                && last.lines == (line..line + 1)
                && last.chars.end == cursor
            {
                last.chars.end += 1;
            } else {
                self.spans.push(Span {
                    lines: line..line + 1,
                    chars: cursor..cursor + 1,
                });
            }
        }
    }

    pub fn apply(&self, buffer: &gtk::TextBuffer, ranges: &[LineRange]) -> Option<String> {
        let tag = buffer
            .tag_table()
            .lookup("source-poc-highlight")
            .unwrap_or_else(|| {
                let tag = gtk::TextTag::builder()
                    .name("source-poc-highlight")
                    .background("#f9d76d")
                    .foreground("#18212b")
                    .build();
                buffer.tag_table().add(&tag);
                tag
            });
        tag.set_priority(buffer.tag_table().size() - 1);
        buffer.remove_tag(&tag, &buffer.start_iter(), &buffer.end_iter());
        let intersects = |lines: &Range<usize>| {
            ranges
                .iter()
                .any(|r| (r.start as usize) < lines.end && r.end as usize >= lines.start)
        };
        // Coalesce adjacent spans before touching GTK: a whole-file range must not
        // perform one tag-tree mutation per source line or inline formatting token.
        let mut selected: Vec<Range<i32>> = Vec::new();
        let mut blank = Vec::new();
        for span in self.spans.iter().filter(|s| intersects(&s.lines)) {
            let start = buffer.iter_at_offset(span.chars.start);
            if span.chars.end == span.chars.start + 1
                && start.starts_line()
                && start.char() == '\n'
                && blank.len() < 16
            {
                blank.push(span.lines.start);
            }
            if let Some(last) = selected.last_mut()
                && last.end >= span.chars.start
            {
                last.end = last.end.max(span.chars.end);
            } else {
                selected.push(span.chars.clone());
            }
        }
        for chars in selected {
            buffer.apply_tag(
                &tag,
                &buffer.iter_at_offset(chars.start),
                &buffer.iter_at_offset(chars.end),
            );
        }
        let mut provider = self.css.borrow_mut();
        let css = provider.get_or_insert_with(|| {
            let css = gtk::CssProvider::new();
            css.load_from_data(".source-poc-highlight { background-color: alpha(#e5ac00, 0.3); box-shadow: inset 0 0 0 3px #bf8700; }");
            css
        });
        for (lines, widget) in &self.widgets {
            #[allow(deprecated)]
            widget
                .style_context()
                .add_provider(&*css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
            if intersects(lines) {
                widget.add_css_class("source-poc-highlight");
            } else {
                widget.remove_css_class("source-poc-highlight");
            }
        }
        let covered: std::collections::HashSet<_> = self
            .spans
            .iter()
            .flat_map(|s| s.lines.clone())
            .chain(self.widgets.iter().flat_map(|(s, _)| s.clone()))
            .collect();
        let mut missing = Vec::new();
        // Report requested lines without a rendered counterpart; never redirect to a neighbour.
        for range in ranges {
            for line in range.start..=range.end {
                let line = line as usize;
                if !covered.contains(&line) {
                    missing.push(line);
                    if missing.len() >= 16 {
                        break;
                    }
                }
            }
            if missing.len() >= 16 {
                break;
            }
        }
        let ambiguous = self
            .ambiguous
            .iter()
            .filter(|lines| intersects(lines))
            .count();
        (!missing.is_empty() || !blank.is_empty() || ambiguous > 0).then(|| {
            format!(
                "Unmapped source lines (first 16): {missing:?}; blank rendered lines without visible markers (first 16): {blank:?}; ambiguous transforms: {ambiguous}"
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_crlf_code_and_normalized_inline_code() {
        let source = "é🦀\r\n  code\r\n";
        let lines = Lines::new(source);
        let mut map = SourceMap::default();
        map.text(&lines, source, 0..source.len(), "é🦀\ncode\n", 0);
        assert_eq!(map.spans[0].chars, 0..3);
        assert_eq!(map.spans[1].lines, 2..3);
        assert_eq!(map.spans[1].chars, 3..8);
        let source = "` first\r\nsecond `";
        let mut map = SourceMap::default();
        map.inline_code(
            &Lines::new(source),
            source,
            0..source.len(),
            "first second",
            0,
        );
        assert!(map.ambiguous.is_empty());
        assert_eq!(map.spans[0].lines, 1..2);
        assert_eq!(map.spans[0].chars, 0..6);
        assert_eq!(map.spans[1].lines, 2..3);
        assert_eq!(map.spans[1].chars, 6..12);
    }
}
