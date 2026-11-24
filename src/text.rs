//! Character-level helpers: classes, columns, display width, tokens.

use similar::{Algorithm, DiffTag, capture_diff_slices};
use std::ops::Range;
use unicode_width::UnicodeWidthChar;

pub const TAB_WIDTH: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Blank,
    Word,
    Punct,
}

/// Vim's character classes; with `big` everything non-blank is one class.
pub fn class(c: char, big: bool) -> Class {
    if c.is_whitespace() {
        Class::Blank
    } else if big || c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Punct
    }
}

pub fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// Byte offset of the char column `col` (the string length if past the end).
pub fn byte_at(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map_or(s.len(), |(i, _)| i)
}

/// Column of the first non-blank char; the last column on a blank line.
pub fn first_non_blank(s: &str) -> usize {
    s.chars()
        .position(|c| !c.is_whitespace())
        .unwrap_or_else(|| char_len(s).saturating_sub(1))
}

/// The leading whitespace of a line.
pub fn indent_of(s: &str) -> &str {
    &s[..s.find(|c: char| !c.is_whitespace()).unwrap_or(s.len())]
}

/// Width in terminal cells of `c` when drawn at display column `at`.
pub fn char_width(c: char, at: usize) -> usize {
    if c == '\t' {
        TAB_WIDTH - at % TAB_WIDTH
    } else {
        c.width().unwrap_or(0)
    }
}

/// Display column at which the char column `col` starts.
pub fn display_col(s: &str, col: usize) -> usize {
    s.chars().take(col).fold(0, |d, c| d + char_width(c, d))
}

/// Splits text into word runs, blank runs, single newlines and single
/// punctuation chars. Returns byte ranges.
pub fn tokens(s: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        let mut end = i + c.len_utf8();
        let k = class(c, false);
        if c != '\n' && k != Class::Punct {
            while let Some(&(j, d)) = it.peek() {
                if d == '\n' || class(d, false) != k {
                    break;
                }
                end = j + d.len_utf8();
                it.next();
            }
        }
        out.push(i..end);
    }
    out
}

/// For each line of `side`, the char ranges that differ from `base`
/// at token level.
pub fn emphasis(base: &[String], side: &[String]) -> Vec<Vec<Range<usize>>> {
    let (b, s) = (base.join("\n"), side.join("\n"));
    let (br, sr) = (tokens(&b), tokens(&s));
    let bt: Vec<&str> = br.iter().map(|r| &b[r.clone()]).collect();
    let st: Vec<&str> = sr.iter().map(|r| &s[r.clone()]).collect();
    // Line and char column of every side token.
    let mut place = Vec::with_capacity(st.len());
    let (mut line, mut col) = (0, 0);
    for t in &st {
        if *t == "\n" {
            place.push((line, col..col));
            line += 1;
            col = 0;
        } else {
            let n = char_len(t);
            place.push((line, col..col + n));
            col += n;
        }
    }
    let mut out: Vec<Vec<Range<usize>>> = vec![Vec::new(); side.len()];
    for op in capture_diff_slices(Algorithm::Myers, &bt, &st) {
        let (tag, _, new) = op.as_tag_tuple();
        if tag == DiffTag::Equal {
            continue;
        }
        for (line, r) in &place[new] {
            if r.is_empty() {
                continue;
            }
            match out[*line].last_mut() {
                Some(last) if last.end == r.start => last.end = r.end,
                _ => out[*line].push(r.clone()),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<&str> {
        tokens(s).into_iter().map(|r| &s[r]).collect()
    }

    fn lines(s: &[&str]) -> Vec<String> {
        s.iter().map(|l| l.to_string()).collect()
    }

    #[test]
    fn tokens_split_words_blanks_and_punctuation() {
        assert_eq!(
            toks("foo(a, b_1)"),
            vec!["foo", "(", "a", ",", " ", "b_1", ")"]
        );
    }

    #[test]
    fn tokens_keep_each_newline_separate_from_indentation() {
        assert_eq!(toks("a\n\n  b"), vec!["a", "\n", "\n", "  ", "b"]);
    }

    #[test]
    fn display_col_expands_tabs_to_the_next_tab_stop() {
        assert_eq!(display_col("\tab", 1), 4);
        assert_eq!(display_col("ab\tc", 3), 4);
    }

    #[test]
    fn display_col_counts_wide_chars_as_two_cells() {
        assert_eq!(display_col("日本x", 2), 4);
    }

    #[test]
    fn first_non_blank_skips_indentation() {
        assert_eq!(first_non_blank("  \tx y"), 3);
        assert_eq!(first_non_blank("   "), 2);
        assert_eq!(first_non_blank(""), 0);
    }

    #[test]
    fn indent_of_returns_leading_whitespace() {
        assert_eq!(indent_of("\t  x "), "\t  ");
        assert_eq!(indent_of("x"), "");
    }

    #[test]
    fn class_distinguishes_words_from_punctuation_unless_big() {
        assert_eq!(class('_', false), Class::Word);
        assert_eq!(class('.', false), Class::Punct);
        assert_eq!(class('.', true), Class::Word);
        assert_eq!(class(' ', true), Class::Blank);
    }

    #[test]
    fn emphasis_marks_only_the_changed_word() {
        assert_eq!(
            emphasis(&lines(&["x = foo(a)"]), &lines(&["x = bar(a)"])),
            vec![vec![4..7]]
        );
    }

    #[test]
    fn emphasis_reports_ranges_per_line() {
        assert_eq!(
            emphasis(&lines(&["a", "b"]), &lines(&["a", "c d"])),
            vec![vec![], vec![0..3]]
        );
    }
}
