//! Keep leading YAML metadata opaque in both document and diagram parsing.
use pulldown_cmark::{Event, Options, Parser, Tag};
use std::ops::Range;

pub fn events(content: &str) -> impl Iterator<Item = (Event<'_>, Range<usize>)> {
    let yaml = Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
    let end = match Parser::new_ext(content, yaml).into_offset_iter().next() {
        Some((Event::Start(Tag::MetadataBlock(_)), bytes)) if bytes.start == 0 => bytes.end,
        _ => 0,
    };
    // Enabling metadata for the entire document would also consume later rules
    // and setext headings. Only the leading block is frontmatter.
    Parser::new_ext(&content[..end], yaml)
        .into_offset_iter()
        .chain(
            Parser::new_ext(
                &content[end..],
                Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
            )
            .into_offset_iter()
            .map(move |(event, bytes)| (event, bytes.start + end..bytes.end + end)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulldown_cmark::{HeadingLevel, TagEnd};

    #[test]
    fn leading_metadata_preserves_offsets_and_later_markdown() {
        let source = "---\r\ntitle: é🦀\r\n---\r\n\r\n# Body\r\n\r\n---\r\n\r\nSetext\r\n---\r\n";
        let parsed: Vec<_> = events(source).collect();
        assert!(matches!(parsed[0].0, Event::Start(Tag::MetadataBlock(_))));
        let end = source.find("\r\n\r\n").unwrap();
        assert_eq!(parsed[0].1, 0..end);
        assert_eq!(
            parsed
                .iter()
                .filter(|(e, _)| matches!(e, Event::Start(Tag::MetadataBlock(_))))
                .count(),
            1
        );
        assert!(
            parsed
                .iter()
                .any(|(e, r)| matches!(e, Event::Text(t) if t.as_ref() == "Body")
                    && &source[r.clone()] == "Body")
        );
        assert!(parsed.iter().any(|(e, _)| matches!(e, Event::Rule)));
        assert!(
            parsed
                .iter()
                .any(|(e, _)| matches!(e, Event::End(TagEnd::Heading(HeadingLevel::H2))))
        );
    }

    #[test]
    fn incomplete_or_nonleading_metadata_remains_markdown() {
        for source in [
            "---\ntitle: incomplete\n",
            "# Body\n\n---\ntitle: later\n---\n",
            "---\n---\n",
        ] {
            assert!(!events(source).any(|(e, _)| matches!(e, Event::Start(Tag::MetadataBlock(_)))));
        }
    }
}
