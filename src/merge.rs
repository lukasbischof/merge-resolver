//! Three-way merge (diff3) over arbitrary token slices.

use similar::{Algorithm, DiffTag, capture_diff_slices};
use std::hash::Hash;
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Only the left side changed this region.
    Left,
    /// Only the right side changed this region.
    Right,
    /// Both sides made the identical change.
    Both,
    /// Both sides changed this region differently.
    Conflict,
}

/// A region where at least one side differs from the base.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hunk {
    pub kind: Kind,
    pub base: Range<usize>,
    pub left: Range<usize>,
    pub right: Range<usize>,
}

/// A changed region of one side: (base range, side range).
type Change = (Range<usize>, Range<usize>);

fn changes<T: Hash + Eq>(base: &[T], side: &[T]) -> Vec<Change> {
    let mut out: Vec<Change> = Vec::new();
    for op in capture_diff_slices(Algorithm::Patience, base, side) {
        let (tag, b, s) = op.as_tag_tuple();
        if tag == DiffTag::Equal {
            continue;
        }
        if let Some(last) = out.last_mut()
            && last.0.end == b.start
            && last.1.end == s.start
        {
            last.0.end = b.end;
            last.1.end = s.end;
            continue;
        }
        out.push((b, s));
    }
    out
}

/// Maps the base region `start..end` onto one side, given that side's
/// changes inside the region. `delta` carries the side's line offset
/// relative to the base across calls.
fn side_range(changes: &[Change], start: usize, end: usize, delta: &mut isize) -> Range<usize> {
    match (changes.first(), changes.last()) {
        (Some(first), Some(last)) => {
            let s = first.1.start - (first.0.start - start);
            let e = last.1.end + (end - last.0.end);
            *delta = e as isize - end as isize;
            s..e
        }
        _ => (start as isize + *delta) as usize..(end as isize + *delta) as usize,
    }
}

/// Computes the regions where `left` and/or `right` differ from `base`.
/// Changes that overlap or touch in the base are grouped into one hunk,
/// so edits to adjacent lines conflict, as they do in git.
pub fn diff3<T: Hash + Eq>(base: &[T], left: &[T], right: &[T]) -> Vec<Hunk> {
    let l = changes(base, left);
    let r = changes(base, right);
    let (mut i, mut j) = (0, 0);
    let (mut dl, mut dr) = (0isize, 0isize);
    let mut hunks = Vec::new();
    while i < l.len() || j < r.len() {
        let start = match (l.get(i), r.get(j)) {
            (Some(a), Some(b)) => a.0.start.min(b.0.start),
            (Some(a), None) => a.0.start,
            (None, Some(b)) => b.0.start,
            (None, None) => unreachable!(),
        };
        let mut end = start;
        let (i0, j0) = (i, j);
        loop {
            let mut grew = false;
            while i < l.len() && l[i].0.start <= end {
                end = end.max(l[i].0.end);
                i += 1;
                grew = true;
            }
            while j < r.len() && r[j].0.start <= end {
                end = end.max(r[j].0.end);
                j += 1;
                grew = true;
            }
            if !grew {
                break;
            }
        }
        let lr = side_range(&l[i0..i], start, end, &mut dl);
        let rr = side_range(&r[j0..j], start, end, &mut dr);
        let kind = match (i > i0, j > j0) {
            (true, false) => Kind::Left,
            (false, true) => Kind::Right,
            _ if left[lr.clone()] == right[rr.clone()] => Kind::Both,
            _ => Kind::Conflict,
        };
        hunks.push(Hunk {
            kind,
            base: start..end,
            left: lr,
            right: rr,
        });
    }
    hunks
}

#[cfg(test)]
mod tests {
    use super::*;

    type Row = (Kind, Range<usize>, Range<usize>, Range<usize>);

    fn run(base: &str, left: &str, right: &str) -> Vec<Row> {
        let split = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        diff3(&split(base), &split(left), &split(right))
            .into_iter()
            .map(|h| (h.kind, h.base, h.left, h.right))
            .collect()
    }

    #[test]
    fn unchanged_sides_produce_no_hunks() {
        assert_eq!(run("a b c", "a b c", "a b c"), vec![]);
    }

    #[test]
    fn change_on_one_side_is_attributed_to_that_side() {
        assert_eq!(
            run("a b c d e", "a X c d e", "a b c d e"),
            vec![(Kind::Left, 1..2, 1..2, 1..2)]
        );
        assert_eq!(
            run("a b c d e", "a b c d e", "a X c d e"),
            vec![(Kind::Right, 1..2, 1..2, 1..2)]
        );
    }

    #[test]
    fn separate_changes_on_both_sides_do_not_conflict() {
        assert_eq!(
            run("a b c d e", "a X c d e", "a b c d Y"),
            vec![
                (Kind::Left, 1..2, 1..2, 1..2),
                (Kind::Right, 4..5, 4..5, 4..5)
            ]
        );
    }

    #[test]
    fn different_changes_to_the_same_line_conflict() {
        assert_eq!(
            run("a b c", "a X c", "a Y c"),
            vec![(Kind::Conflict, 1..2, 1..2, 1..2)]
        );
    }

    #[test]
    fn identical_changes_on_both_sides_are_not_a_conflict() {
        assert_eq!(
            run("a b c", "a X c", "a X c"),
            vec![(Kind::Both, 1..2, 1..2, 1..2)]
        );
    }

    #[test]
    fn side_ranges_account_for_earlier_insertions() {
        assert_eq!(
            run("a b c d e", "N1 N2 a b c d e", "a b c d Y"),
            vec![
                (Kind::Left, 0..0, 0..2, 0..0),
                (Kind::Right, 4..5, 6..7, 4..5)
            ]
        );
    }

    #[test]
    fn insertions_at_the_same_point_conflict() {
        assert_eq!(
            run("a b c", "a b L c", "a b R c"),
            vec![(Kind::Conflict, 2..2, 2..3, 2..3)]
        );
    }

    #[test]
    fn changes_on_adjacent_lines_conflict() {
        assert_eq!(
            run("a b c d", "a X c d", "a b Y d"),
            vec![(Kind::Conflict, 1..3, 1..3, 1..3)]
        );
    }

    #[test]
    fn deletion_against_modification_conflicts() {
        assert_eq!(
            run("a b c", "a c", "a X c"),
            vec![(Kind::Conflict, 1..2, 1..1, 1..2)]
        );
    }
}
