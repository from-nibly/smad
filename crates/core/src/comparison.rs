//! Complete-file unified rows; no patch parser and no discarded context.
use crate::source::SourceText;
use imara_diff::{Algorithm, Diff, InternedInput};
use std::ops::Range;

/// Bound worst-case changed-region work after common prefix/suffix removal.
pub const MAX_COMPARISON_WORK: usize = 16_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Context,
    Removed,
    Added,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparisonRow {
    /// One-based original coordinates; never display row indices.
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub kind: RowKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    pub rows: Vec<ComparisonRow>,
    /// Half-open display-row intervals used for change navigation.
    pub changes: Vec<Range<usize>>,
}

pub fn compare(before: &SourceText, after: &SourceText) -> Result<Comparison, String> {
    let old_len = before.lines.len();
    let new_len = after.lines.len();
    let common = old_len.min(new_len);
    let prefix = (0..common)
        .take_while(|&i| before.raw_line(i) == after.raw_line(i))
        .count();
    let suffix = (0..common - prefix)
        .take_while(|&i| before.raw_line(old_len - i - 1) == after.raw_line(new_len - i - 1))
        .count();
    let old_end = old_len - suffix;
    let new_end = new_len - suffix;
    if (old_end - prefix).saturating_mul(new_end - prefix) > MAX_COMPARISON_WORK {
        return Err(
            "changed region exceeds the bounded comparison work budget; narrow the input files"
                .into(),
        );
    }
    let mut input = InternedInput::default();
    input.update_before((prefix..old_end).map(|i| before.raw_line(i)));
    input.update_after((prefix..new_end).map(|i| after.raw_line(i)));
    let diff = Diff::compute(Algorithm::Histogram, &input);
    let mut rows = Vec::with_capacity(old_len + new_len);
    let mut changes = Vec::new();
    let mut old = 0;
    let mut new = 0;
    for hunk in diff.hunks() {
        let old_start = prefix + hunk.before.start as usize;
        let new_start = prefix + hunk.after.start as usize;
        while old < old_start && new < new_start {
            rows.push(ComparisonRow {
                old: Some(old as u32 + 1),
                new: Some(new as u32 + 1),
                kind: RowKind::Context,
            });
            old += 1;
            new += 1;
        }
        let change_start = rows.len();
        while old < prefix + hunk.before.end as usize {
            rows.push(ComparisonRow {
                old: Some(old as u32 + 1),
                new: None,
                kind: RowKind::Removed,
            });
            old += 1;
        }
        while new < prefix + hunk.after.end as usize {
            rows.push(ComparisonRow {
                old: None,
                new: Some(new as u32 + 1),
                kind: RowKind::Added,
            });
            new += 1;
        }
        changes.push(change_start..rows.len());
    }
    while old < old_len && new < new_len {
        rows.push(ComparisonRow {
            old: Some(old as u32 + 1),
            new: Some(new as u32 + 1),
            kind: RowKind::Context,
        });
        old += 1;
        new += 1;
    }
    debug_assert_eq!((old, new), (old_len, new_len));
    Ok(Comparison { rows, changes })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text(value: &str) -> SourceText {
        SourceText::parse(value.into()).unwrap()
    }

    fn verify(old: &str, new: &str) -> Comparison {
        let old = text(old);
        let new = text(new);
        let result = compare(&old, &new).unwrap();
        let reconstructed_old = result
            .rows
            .iter()
            .filter_map(|row| row.old.map(|line| old.raw_line(line as usize - 1)))
            .collect::<String>();
        let reconstructed_new = result
            .rows
            .iter()
            .filter_map(|row| row.new.map(|line| new.raw_line(line as usize - 1)))
            .collect::<String>();
        assert_eq!(reconstructed_old, old.text);
        assert_eq!(reconstructed_new, new.text);
        for row in &result.rows {
            if let (Some(a), Some(b)) = (row.old, row.new) {
                assert_eq!(old.raw_line(a as usize - 1), new.raw_line(b as usize - 1));
            }
        }
        result
    }

    #[test]
    fn full_context_multiple_hunks_and_source_coordinates() {
        let result = verify(
            "start\nold\ncontext\nstable\nremoved\nend\n",
            "start\nnew\ncontext\nstable\nend\nadded\n",
        );
        assert_eq!(result.changes.len(), 3);
        assert_eq!(
            result.rows[0],
            ComparisonRow {
                old: Some(1),
                new: Some(1),
                kind: RowKind::Context
            }
        );
        assert_eq!(
            result.rows[3],
            ComparisonRow {
                old: Some(3),
                new: Some(3),
                kind: RowKind::Context
            }
        );
        assert!(
            result
                .rows
                .iter()
                .any(|row| row.old == Some(6) && row.new == Some(5))
        );
    }

    #[test]
    fn insertion_deletion_empty_newlines_and_unicode() {
        for (old, new) in [
            ("", ""),
            ("", "é🦀\n\nlast"),
            ("a\nb\n", ""),
            ("same\n", "same\n"),
            ("line", "line\n"),
            ("line\r\n", "line\n"),
            ("\n", ""),
            ("a\nx\na\nx\n", "x\na\nx\na\n"),
        ] {
            let result = verify(old, new);
            assert_eq!(result.changes.is_empty(), old == new);
        }
    }

    #[test]
    fn all_small_inputs_reconstruct_both_complete_sources() {
        let mut inputs = vec![String::new()];
        for length in 1..=4 {
            for bits in 0..(1 << length) {
                let mut value = String::new();
                for i in 0..length {
                    value.push_str(if bits & (1 << i) == 0 {
                        "é\n"
                    } else {
                        "🦀\r\n"
                    });
                }
                inputs.push(value.clone());
                inputs.push(value.trim_end_matches(['\r', '\n']).to_owned());
            }
        }
        for old in &inputs {
            for new in &inputs {
                let result = verify(old, new);
                assert_eq!(result.changes.is_empty(), old == new);
            }
        }
    }

    #[test]
    fn large_common_context_does_not_consume_changed_region_budget() {
        let prefix = "context\n".repeat(9000);
        let suffix = "tail\n".repeat(9000);
        let result = verify(
            &format!("{prefix}old\n{suffix}"),
            &format!("{prefix}new\n{suffix}"),
        );
        assert_eq!(result.rows.len(), 18002);
        assert_eq!(result.changes.len(), 1);
        assert!(compare(&text(&"old\n".repeat(4001)), &text(&"new\n".repeat(4001))).is_err());
    }
}
