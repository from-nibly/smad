//! Local Markdown identity and navigation; never interpreted as a review-server URL.
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use url::Url;

pub const MAX_MARKDOWN_BYTES: u64 = 1024 * 1024;

pub fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("md") || s.eq_ignore_ascii_case("markdown"))
}

pub fn canonical_markdown(path: &Path) -> Result<PathBuf, String> {
    let path = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !is_markdown_path(&path) || !path.is_file() {
        return Err("expected a regular .md or .markdown file (MDX is not supported)".into());
    }
    if path.to_str().is_none() {
        return Err("Markdown paths must be UTF-8".into());
    }
    Ok(path)
}

pub fn read_markdown(path: &Path) -> Result<String, String> {
    // An atomic replacement could turn the watched path into a FIFO. Do not
    // block the GTK thread waiting for its writer before checking the file type.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_MARKDOWN_BYTES {
        return Err("Markdown POC accepts regular files up to 1 MiB".into());
    }
    let mut content = String::new();
    file.take(MAX_MARKDOWN_BYTES + 1)
        .read_to_string(&mut content)
        .map_err(|e| e.to_string())?;
    if content.len() as u64 > MAX_MARKDOWN_BYTES {
        return Err("Markdown file grew beyond 1 MiB".into());
    }
    Ok(content)
}

#[derive(Debug, PartialEq, Eq)]
pub enum LinkTarget {
    Markdown {
        path: PathBuf,
        fragment: Option<String>,
    },
    External(String),
}

pub fn resolve_link(source: &Path, href: &str) -> Result<LinkTarget, String> {
    let base = Url::from_file_path(source).map_err(|_| "document path must be absolute")?;
    let target = base.join(href).map_err(|e| e.to_string())?;
    match target.scheme() {
        "file" => {
            if target.query().is_some() {
                return Err("queries on local Markdown links are not supported".into());
            }
            let path = target
                .to_file_path()
                .map_err(|_| "invalid local file link")?;
            if !is_markdown_path(&path) {
                return Err("local links must target .md or .markdown files".into());
            }
            Ok(LinkTarget::Markdown {
                path,
                fragment: target.fragment().map(str::to_owned),
            })
        }
        "https" | "http" | "mailto" => Ok(LinkTarget::External(target.to_string())),
        _ => Err("unsupported link scheme".into()),
    }
}

/// GitHub-like heading anchors for the POC (duplicate suffixes assigned by renderer).
pub fn heading_slug(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                Some(c)
            } else if c.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links_use_document_directory_not_process_cwd() {
        assert_eq!(
            resolve_link(
                Path::new("/tmp/docs/start.md"),
                "../next%20page.md#some-heading"
            )
            .unwrap(),
            LinkTarget::Markdown {
                path: PathBuf::from("/tmp/next page.md"),
                fragment: Some("some-heading".into())
            }
        );
        assert_eq!(
            resolve_link(Path::new("/tmp/docs/start.md"), "#intro").unwrap(),
            LinkTarget::Markdown {
                path: PathBuf::from("/tmp/docs/start.md"),
                fragment: Some("intro".into())
            }
        );
    }
    #[test]
    fn local_html_and_active_schemes_are_not_browser_navigation() {
        for link in [
            "page.html",
            "javascript:alert(1)",
            "data:text/html,test",
            "ftp://host/a.md",
            "a.md?x=1",
        ] {
            assert!(
                resolve_link(Path::new("/tmp/start.md"), link).is_err(),
                "{link}"
            );
        }
        assert!(matches!(
            resolve_link(Path::new("/tmp/start.md"), "https://example.org").unwrap(),
            LinkTarget::External(_)
        ));
    }
    #[test]
    fn markdown_identity_and_heading_rules() {
        assert!(is_markdown_path(Path::new("README.MD")));
        assert!(!is_markdown_path(Path::new("page.mdx")));
        assert_eq!(
            heading_slug("Hello, *Native* Reader!"),
            "hello-native-reader"
        );
    }
}
