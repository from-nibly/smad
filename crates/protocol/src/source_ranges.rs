//! Bounded source coordinates shared by URI, CLI and native-renderer requests.
use serde::{Deserialize, Serialize};

pub const MAX_HIGHLIGHT_RANGES: usize = 64;
pub const MAX_HIGHLIGHT_BYTES: usize = 2048;
pub const MAX_SOURCE_COORDINATE: u32 = 1_000_000;

/// One-based inclusive source lines, never visual/wrapped rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

impl LineRange {
    pub fn contains(self, line: u32) -> bool {
        self.start <= line && line <= self.end
    }
}

/// Validate wire inputs as well as user strings: deserialization alone does not
/// establish that a range is ordered, nonzero, or bounded.
pub fn normalize_ranges(mut ranges: Vec<LineRange>) -> Result<Vec<LineRange>, String> {
    if ranges.len() > MAX_HIGHLIGHT_RANGES {
        return Err(format!(
            "at most {MAX_HIGHLIGHT_RANGES} highlight ranges are supported"
        ));
    }
    if ranges.iter().any(|range| {
        range.start == 0 || range.end < range.start || range.end > MAX_SOURCE_COORDINATE
    }) {
        return Err(format!(
            "highlight ranges require 1-based inclusive lines, start <= end <= {MAX_SOURCE_COORDINATE}"
        ));
    }
    ranges.sort_unstable_by_key(|range| (range.start, range.end));
    let mut normalized: Vec<LineRange> = Vec::with_capacity(ranges.len());
    for range in ranges {
        if let Some(last) = normalized.last_mut()
            && range.start <= last.end + 1
        {
            last.end = last.end.max(range.end);
        } else {
            normalized.push(range);
        }
    }
    Ok(normalized)
}

pub fn parse_ranges(value: &str) -> Result<Vec<LineRange>, String> {
    if value.is_empty() || value.len() > MAX_HIGHLIGHT_BYTES {
        return Err(format!(
            "highlight must contain 1–{MAX_HIGHLIGHT_BYTES} bytes of line ranges"
        ));
    }
    let number = |value: &str| {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("expected highlight lines such as 12-24,48,70-85".to_owned());
        }
        value
            .parse::<u32>()
            .map_err(|_| "highlight line number is too large".to_owned())
    };
    let mut ranges = Vec::new();
    for part in value.split(',') {
        if ranges.len() == MAX_HIGHLIGHT_RANGES {
            return Err(format!(
                "at most {MAX_HIGHLIGHT_RANGES} highlight ranges are supported"
            ));
        }
        let (start, end) = part.split_once('-').unwrap_or((part, part));
        ranges.push(LineRange {
            start: number(start)?,
            end: number(end)?,
        });
    }
    normalize_ranges(ranges)
}

pub fn format_ranges(ranges: &[LineRange]) -> String {
    ranges
        .iter()
        .map(|range| {
            if range.start == range.end {
                range.start.to_string()
            } else {
                format!("{}-{}", range.start, range.end)
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_source_intervals_without_expanding_them() {
        let ranges = parse_ranges("70-85,12-24,48,22-30,31,48,0001").unwrap();
        assert_eq!(format_ranges(&ranges), "1,12-31,48,70-85");
        assert!(ranges[1].contains(12));
        assert!(ranges[1].contains(31));
        assert!(!ranges[1].contains(32));
        assert_eq!(parse_ranges(&format_ranges(&ranges)).unwrap(), ranges);
        assert_eq!(parse_ranges("1-1000000").unwrap().len(), 1);
        assert_eq!(normalize_ranges(Vec::new()).unwrap(), Vec::new());
    }

    #[test]
    fn rejects_malformed_and_unbounded_inputs() {
        for value in [
            "",
            "0",
            "2-1",
            "-1",
            "1-",
            "1-2-3",
            "1,",
            ",1",
            "1,,2",
            " 1",
            "+1",
            "1 -2",
            "١",
            "1000001",
            "4294967296",
        ] {
            assert!(parse_ranges(value).is_err(), "{value}");
        }
        assert!(parse_ranges(&"1".repeat(MAX_HIGHLIGHT_BYTES + 1)).is_err());
        assert!(parse_ranges(&vec!["1"; MAX_HIGHLIGHT_RANGES + 1].join(",")).is_err());
        assert!(normalize_ranges(vec![LineRange { start: 0, end: 1 }]).is_err());
        assert!(
            normalize_ranges(vec![LineRange {
                start: 1,
                end: u32::MAX
            }])
            .is_err()
        );
        assert!(
            normalize_ranges(vec![
                LineRange { start: 1, end: 1 };
                MAX_HIGHLIGHT_RANGES + 1
            ])
            .is_err()
        );
    }
}
