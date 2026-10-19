//! Explicit local-file renderer schemes. `://` is followed by a path, never a network authority.
use smad_protocol::DocumentKind;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentUri {
    pub kind: DocumentKind,
    pub path: PathBuf,
    pub fragment: Option<String>,
    pub highlights: Vec<smad_protocol::LineRange>,
}

impl DocumentUri {
    pub fn parse(value: &str, cwd: &Path) -> Result<Self, String> {
        let (scheme, target) = value.split_once("://").ok_or(
            "use code://<file>, md://<file.md> or lavish://<file.html>; an explicit renderer scheme is required",
        )?;
        let kind = match scheme.to_ascii_lowercase().as_str() {
            "md" => DocumentKind::Markdown,
            "code" => DocumentKind::Code,
            "lavish" => DocumentKind::LavishReview,
            _ => return Err("unsupported document scheme; use code://, md:// or lavish:// (comparisons use smad-axi compare)".into()),
        };
        let (path, fragment) = target
            .split_once('#')
            .map_or((target, None), |(path, fragment)| {
                (path, Some(fragment.to_owned()))
            });
        let (path, query) = path
            .split_once('?')
            .map_or((path, None), |(path, query)| (path, Some(query)));
        if path.is_empty() {
            return Err(
                "expected a local file path; percent-encode literal ? and # in filenames".into(),
            );
        }
        let highlights = if let Some(query) = query {
            if !matches!(kind, DocumentKind::Code | DocumentKind::Markdown) {
                return Err("source-line highlight queries require code:// or md://".into());
            }
            let value = query
                .strip_prefix("highlight=")
                .filter(|value| !value.contains('&'))
                .ok_or("only one highlight= query parameter is supported")?;
            let decoded = percent_encoding::percent_decode_str(value)
                .decode_utf8()
                .map_err(|_| "invalid UTF-8 highlight query")?;
            smad_protocol::parse_ranges(&decoded)?
        } else {
            Vec::new()
        };
        if fragment
            .as_ref()
            .is_some_and(|fragment| fragment.len() > 512)
        {
            return Err("heading fragment exceeds 512 bytes".into());
        }
        if kind != DocumentKind::Markdown && fragment.is_some() {
            return Err("heading fragments are supported only for md:// documents".into());
        }
        for (index, byte) in path.bytes().enumerate() {
            if byte == b'%'
                && !path
                    .as_bytes()
                    .get(index + 1..index + 3)
                    .is_some_and(|pair| pair.iter().all(u8::is_ascii_hexdigit))
            {
                return Err(
                    "invalid percent escape in document path; encode a literal % as %25".into(),
                );
            }
        }
        let decoded = percent_encoding::percent_decode_str(path)
            .decode_utf8()
            .map_err(|_| "document path is not valid UTF-8")?;
        if decoded.contains('\0') {
            return Err("document path contains a NUL byte".into());
        }
        let path = Path::new(decoded.as_ref());
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            cwd.join(path)
        };
        let path = match kind {
            DocumentKind::Markdown => crate::markdown::canonical_markdown(&path)?,
            DocumentKind::Code => crate::source::canonical_source(&path)?,
            DocumentKind::Comparison => {
                return Err("use smad-axi compare for full-file comparisons".into());
            }
            DocumentKind::LavishReview => {
                let path = path.canonicalize().map_err(|e| e.to_string())?;
                if !path.is_file()
                    || !path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
                        s.eq_ignore_ascii_case("html") || s.eq_ignore_ascii_case("htm")
                    })
                {
                    return Err("lavish:// requires a local .html or .htm artifact".into());
                }
                path
            }
        };
        if path.to_str().is_none() {
            return Err("document path must be UTF-8".into());
        }
        Ok(Self {
            kind,
            path,
            fragment,
            highlights,
        })
    }

    pub fn canonical_uri(&self) -> String {
        let mut url = url::Url::from_file_path(&self.path).expect("canonical absolute path");
        url.set_fragment(self.fragment.as_deref());
        if !self.highlights.is_empty() {
            url.set_query(Some(&format!(
                "highlight={}",
                smad_protocol::format_ranges(&self.highlights)
            )));
        }
        let scheme = match self.kind {
            DocumentKind::Markdown => "md",
            DocumentKind::LavishReview => "lavish",
            DocumentKind::Code => "code",
            DocumentKind::Comparison => unreachable!("comparisons have no URI scheme"),
        };
        format!(
            "{scheme}://{}",
            url.as_str().strip_prefix("file://").expect("file URL")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_schemes_resolve_and_encode_local_files_without_guessing() {
        let directory = std::env::temp_dir().join(format!("smad-uri-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("A file#.md"), "# Title").unwrap();
        std::fs::write(directory.join("review.html"), "<h1>Review</h1>").unwrap();
        let parsed = DocumentUri::parse("md://./A%20file%23.md#title", &directory).unwrap();
        assert_eq!(parsed.kind, DocumentKind::Markdown);
        assert_eq!(parsed.fragment.as_deref(), Some("title"));
        assert!(parsed.canonical_uri().starts_with("md:///"));
        assert!(parsed.canonical_uri().contains("A%20file%23.md#title"));
        assert_eq!(
            DocumentUri::parse(&parsed.canonical_uri(), Path::new("/")),
            Ok(parsed)
        );
        assert_eq!(
            DocumentUri::parse("lavish://review.html", &directory)
                .unwrap()
                .kind,
            DocumentKind::LavishReview
        );
        for invalid in [
            "review.html",
            "https://example.com",
            "md://",
            "md://a.md?x",
            "lavish://review.html#x",
            "md://review.html",
            "lavish://A%20file%23.md",
        ] {
            assert!(
                DocumentUri::parse(invalid, &directory).is_err(),
                "{invalid}"
            );
        }
        let mapped =
            DocumentUri::parse("md://./A%20file%23.md?highlight=5,2-3,4#title", &directory)
                .unwrap();
        assert_eq!(smad_protocol::format_ranges(&mapped.highlights), "2-5");
        assert_eq!(mapped.fragment.as_deref(), Some("title"));
        assert_eq!(
            DocumentUri::parse(&mapped.canonical_uri(), Path::new("/")),
            Ok(mapped)
        );
        assert!(DocumentUri::parse("md://./A%20file%23.md?highlight=0", &directory).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
