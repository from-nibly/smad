//! Explicit local-file renderer schemes. `://` is followed by a path, never a network authority.
use smad_protocol::DocumentKind;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentUri {
    pub kind: DocumentKind,
    pub path: PathBuf,
    pub fragment: Option<String>,
}

impl DocumentUri {
    pub fn parse(value: &str, cwd: &Path) -> Result<Self, String> {
        let (scheme, target) = value.split_once("://").ok_or(
            "use md://<file.md> or lavish://<file.html>; an explicit renderer scheme is required",
        )?;
        let kind = match scheme.to_ascii_lowercase().as_str() {
            "md" => DocumentKind::Markdown,
            "lavish" => DocumentKind::LavishReview,
            _ => return Err("unsupported document scheme; use md:// or lavish://".into()),
        };
        let (path, fragment) = target
            .split_once('#')
            .map_or((target, None), |(path, fragment)| {
                (path, Some(fragment.to_owned()))
            });
        if path.is_empty() || path.contains('?') {
            return Err("expected a local file path, without a query; percent-encode literal ? and # in filenames".into());
        }
        if fragment
            .as_ref()
            .is_some_and(|fragment| fragment.len() > 512)
        {
            return Err("heading fragment exceeds 512 bytes".into());
        }
        if kind == DocumentKind::LavishReview && fragment.is_some() {
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
        })
    }

    pub fn canonical_uri(&self) -> String {
        let mut url = url::Url::from_file_path(&self.path).expect("canonical absolute path");
        url.set_fragment(self.fragment.as_deref());
        let scheme = match self.kind {
            DocumentKind::Markdown => "md",
            DocumentKind::LavishReview => "lavish",
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
        std::fs::remove_dir_all(directory).unwrap();
    }
}
