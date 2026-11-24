//! Aligns the three panes: every chunk and every stretch between
//! chunks occupies the same screen rows in each pane, padded with
//! filler rows where a pane has fewer lines.

use std::ops::Range;

use crate::doc::Doc;

pub struct Seg {
    pub chunk: Option<usize>,
    /// Line range per pane.
    pub r: [Range<usize>; 3],
    pub row: usize,
    pub rows: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowRef {
    Line(usize),
    Filler,
    /// Past the last row.
    None,
}

pub struct Layout {
    pub segs: Vec<Seg>,
    pub rows: usize,
}

impl Layout {
    pub fn build(doc: &Doc) -> Layout {
        let lens = [doc.left.len(), doc.lines.len(), doc.right.len()];
        let mut l = Layout {
            segs: Vec::new(),
            rows: 0,
        };
        let mut pos = [0usize; 3];
        for (i, c) in doc.chunks.iter().enumerate() {
            let r: [Range<usize>; 3] = std::array::from_fn(|p| {
                let s = c.range(p).start.clamp(pos[p], lens[p]);
                s..c.range(p).end.clamp(s, lens[p])
            });
            l.push(None, std::array::from_fn(|p| pos[p]..r[p].start));
            pos = std::array::from_fn(|p| r[p].end);
            l.push(Some(i), r);
        }
        l.push(None, std::array::from_fn(|p| pos[p]..lens[p]));
        l
    }

    fn push(&mut self, chunk: Option<usize>, r: [Range<usize>; 3]) {
        let rows = r.iter().map(|r| r.len()).max().unwrap_or(0);
        if rows > 0 || chunk.is_some() {
            self.segs.push(Seg {
                chunk,
                r,
                row: self.rows,
                rows,
            });
            self.rows += rows;
        }
    }

    pub fn seg_at(&self, row: usize) -> Option<&Seg> {
        self.segs
            .iter()
            .find(|s| row >= s.row && row < s.row + s.rows)
    }

    /// What a pane shows on a screen row.
    pub fn row_ref(&self, pane: usize, row: usize) -> RowRef {
        match self.seg_at(row) {
            None => RowRef::None,
            Some(s) if row - s.row < s.r[pane].len() => RowRef::Line(s.r[pane].start + row - s.row),
            Some(_) => RowRef::Filler,
        }
    }

    /// The row a pane's line is drawn on.
    pub fn row_of(&self, pane: usize, line: usize) -> usize {
        self.segs
            .iter()
            .find(|s| s.r[pane].contains(&line))
            .map_or(self.rows, |s| s.row + line - s.r[pane].start)
    }

    fn line_on(&self, pane: usize, row: usize, filler: impl Fn(&Range<usize>) -> usize) -> usize {
        let row = row.min(self.rows.saturating_sub(1));
        match self.seg_at(row) {
            None => 0,
            Some(s) if row - s.row < s.r[pane].len() => s.r[pane].start + row - s.row,
            Some(s) => filler(&s.r[pane]),
        }
    }

    /// The line to put the cursor on for a row; on filler rows the
    /// chunk's last line, or the line after the gap if it has none.
    pub fn nearest_line(&self, pane: usize, row: usize) -> usize {
        self.line_on(
            pane,
            row,
            |r| if r.is_empty() { r.start } else { r.end - 1 },
        )
    }

    /// Like `nearest_line`, but on filler rows picks the line in the
    /// direction of travel so that scrolling never gets stuck.
    pub fn line_toward(&self, pane: usize, row: usize, down: bool) -> usize {
        self.line_on(pane, row, |r| {
            if down { r.end } else { r.end.saturating_sub(1) }
        })
    }

    /// First row and height of a chunk.
    pub fn chunk_rows(&self, idx: usize) -> (usize, usize) {
        self.segs
            .iter()
            .find(|s| s.chunk == Some(idx))
            .map_or((0, 0), |s| (s.row, s.rows))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{LEFT, OUT, RIGHT};

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    /// Rows:  left | out | right
    ///   0     a    | a   | a
    ///   1     L1   | b   | R      (conflict)
    ///   2     L2   | ~~~ | ~~~
    ///   3     c    | c   | c
    ///   4     d    | d   | d
    ///   5     e    | E   | E      (right-only change, applied)
    fn sample() -> Doc {
        Doc::new(
            words("a b c d e"),
            words("a L1 L2 c d e"),
            words("a R c d E"),
            true,
        )
    }

    fn deleted_on_left() -> Doc {
        Doc::new(words("a b c"), words("a c"), words("a X c"), true)
    }

    #[test]
    fn a_chunk_is_as_tall_as_its_longest_side() {
        let l = Layout::build(&sample());
        assert_eq!(l.rows, 6);
        assert_eq!(l.chunk_rows(0), (1, 2));
        assert_eq!(l.chunk_rows(1), (5, 1));
    }

    #[test]
    fn lines_after_a_shorter_side_are_pushed_down_to_stay_aligned() {
        let l = Layout::build(&sample());
        assert_eq!(l.row_of(LEFT, 3), 3);
        assert_eq!(l.row_of(OUT, 2), 3);
        assert_eq!(l.row_of(RIGHT, 4), 5);
    }

    #[test]
    fn rows_below_a_shorter_side_are_filler() {
        let l = Layout::build(&sample());
        assert_eq!(l.row_ref(LEFT, 2), RowRef::Line(2));
        assert_eq!(l.row_ref(OUT, 2), RowRef::Filler);
        assert_eq!(l.row_ref(OUT, 3), RowRef::Line(2));
        assert_eq!(l.row_ref(OUT, 6), RowRef::None);
    }

    #[test]
    fn nearest_line_on_filler_is_the_last_line_of_the_chunk() {
        let l = Layout::build(&sample());
        assert_eq!(l.nearest_line(OUT, 2), 1);
        assert_eq!(l.nearest_line(LEFT, 2), 2);
    }

    #[test]
    fn nearest_line_for_a_side_without_lines_is_the_line_after_the_gap() {
        let l = Layout::build(&deleted_on_left());
        assert_eq!(l.row_ref(LEFT, 1), RowRef::Filler);
        assert_eq!(l.nearest_line(LEFT, 1), 1);
    }

    #[test]
    fn line_toward_leaves_filler_in_the_direction_of_travel() {
        let l = Layout::build(&sample());
        assert_eq!(l.line_toward(OUT, 2, true), 2);
        assert_eq!(l.line_toward(OUT, 2, false), 1);
        let l = Layout::build(&deleted_on_left());
        assert_eq!(l.line_toward(LEFT, 1, true), 1);
        assert_eq!(l.line_toward(LEFT, 1, false), 0);
    }

    #[test]
    fn lines_typed_outside_chunks_keep_the_other_panes_aligned() {
        let mut d = sample();
        d.splice(0, 0, words("new"));
        let l = Layout::build(&d);
        assert_eq!(l.row_of(OUT, 1), 1);
        assert_eq!(l.row_ref(LEFT, 1), RowRef::Filler);
        assert_eq!(l.row_of(LEFT, 1), 2);
        assert_eq!(l.chunk_rows(0), (2, 2));
    }
}
