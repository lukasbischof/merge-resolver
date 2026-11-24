//! Cursor motions, text objects and search over the focused pane.

use crate::app::{App, MKind, Span};
use crate::doc::Pos;
use crate::keys::{Motion, Obj};
use crate::text::{Class, char_len, class, first_non_blank};

fn at(line: usize, col: usize) -> Pos {
    Pos { line, col }
}

impl App {
    /// Where a motion leads from the cursor, and how an operator
    /// should treat the stretch in between.
    pub(crate) fn target(&self, m: Motion, count: Option<usize>, op: bool) -> Option<(Pos, MKind)> {
        use MKind::*;
        let n = count.unwrap_or(1);
        let Pos { line, col } = self.cur;
        let last = self.last_line();
        let fnb = |l: usize| at(l, first_non_blank(self.line(l)));
        let sticky = |l: usize| at(l, self.want_col.min(self.len(l)));
        Some(match m {
            Motion::Left => (at(line, col.saturating_sub(n)), Excl),
            Motion::Right => (at(line, (col + n).min(self.len(line))), Excl),
            Motion::Down => (sticky((line + n).min(last)), Line),
            Motion::Up => (sticky(line.saturating_sub(n)), Line),
            Motion::Bol => (at(line, 0), Excl),
            Motion::FirstNonBlank => (fnb(line), Excl),
            Motion::Eol => {
                let l = (line + n - 1).min(last);
                (at(l, self.len(l).saturating_sub(1)), Incl)
            }
            Motion::Word(big) => {
                let (mut p, mut prev) = (self.cur, self.cur);
                for _ in 0..n {
                    prev = p;
                    p = self.next_word(p, big);
                }
                // An operator never eats the line break before the next word.
                if op && p.line > prev.line {
                    p = at(prev.line, self.len(prev.line));
                }
                (p, Excl)
            }
            Motion::Back(big) => ((0..n).fold(self.cur, |p, _| self.prev_word(p, big)), Excl),
            Motion::EndWord(big) => ((0..n).fold(self.cur, |p, _| self.word_end(p, big)), Incl),
            Motion::FileStart => (fnb(count.map_or(0, |c| c - 1).min(last)), Line),
            Motion::FileEnd => (fnb(count.map_or(last, |c| c - 1).min(last)), Line),
            Motion::Find { c, fwd, till } => (
                self.find_char(c, fwd, till, n, false)?,
                if fwd { Incl } else { Excl },
            ),
            Motion::RepeatFind(reverse) => {
                let (c, fwd, till) = self.last_find?;
                let fwd = fwd != reverse;
                (
                    self.find_char(c, fwd, till, n, true)?,
                    if fwd { Incl } else { Excl },
                )
            }
            Motion::ParaFwd => {
                let mut l = line;
                for _ in 0..n {
                    while l < last && self.line(l).is_empty() {
                        l += 1;
                    }
                    while l < last && !self.line(l).is_empty() {
                        l += 1;
                    }
                }
                (
                    at(
                        l,
                        if self.line(l).is_empty() {
                            0
                        } else {
                            self.len(l)
                        },
                    ),
                    Excl,
                )
            }
            Motion::ParaBack => {
                let mut l = line;
                for _ in 0..n {
                    while l > 0 && self.line(l).is_empty() {
                        l -= 1;
                    }
                    while l > 0 && !self.line(l).is_empty() {
                        l -= 1;
                    }
                }
                (at(l, 0), Excl)
            }
            Motion::Pair => (self.match_pair()?, Incl),
            Motion::ScreenTop => (
                fnb(self
                    .layout()
                    .line_toward(self.pane, self.top, true)
                    .min(last)),
                Line,
            ),
            Motion::ScreenMid => {
                let l = self.layout();
                let visible = self.view_h.min(l.rows.saturating_sub(self.top));
                (
                    fnb(l
                        .line_toward(self.pane, self.top + visible / 2, true)
                        .min(last)),
                    Line,
                )
            }
            Motion::NextLine => (fnb((line + n).min(last)), Line),
            Motion::PrevLine => (fnb(line.saturating_sub(n)), Line),
            Motion::CurLine => (fnb((line + n - 1).min(last)), Line),
        })
    }

    fn chars(&self, line: usize) -> Vec<char> {
        self.line(line).chars().collect()
    }

    fn next_word(&self, p: Pos, big: bool) -> Pos {
        let chars = self.chars(p.line);
        let mut c = p.col;
        if c < chars.len() {
            let k = class(chars[c], big);
            while k != Class::Blank && c < chars.len() && class(chars[c], big) == k {
                c += 1;
            }
            while c < chars.len() && class(chars[c], big) == Class::Blank {
                c += 1;
            }
            if c < chars.len() {
                return at(p.line, c);
            }
        }
        let last = self.last_line();
        let mut l = p.line;
        loop {
            if l >= last {
                return at(last, self.len(last));
            }
            l += 1;
            let s = self.line(l);
            // An empty line counts as a word.
            if s.is_empty() {
                return at(l, 0);
            }
            if let Some(i) = s.chars().position(|c| !c.is_whitespace()) {
                return at(l, i);
            }
        }
    }

    fn prev_word(&self, p: Pos, big: bool) -> Pos {
        let (mut l, mut c) = (p.line, p.col);
        loop {
            let chars = self.chars(l);
            let mut i = c.min(chars.len());
            while i > 0 && class(chars[i - 1], big) == Class::Blank {
                i -= 1;
            }
            if i > 0 {
                let k = class(chars[i - 1], big);
                while i > 0 && class(chars[i - 1], big) == k {
                    i -= 1;
                }
                return at(l, i);
            }
            if l == 0 {
                return at(0, 0);
            }
            l -= 1;
            c = self.len(l);
            if c == 0 {
                return at(l, 0);
            }
        }
    }

    pub(crate) fn on_non_blank(&self) -> bool {
        self.line(self.cur.line)
            .chars()
            .nth(self.cur.col)
            .is_some_and(|c| !c.is_whitespace())
    }

    /// Where `cw` ends: the last char of the word under the cursor,
    /// then `n - 1` further word ends.
    pub(crate) fn change_word_end(&self, big: bool, n: usize) -> Pos {
        let chars = self.chars(self.cur.line);
        let mut c = self.cur.col;
        let k = class(chars[c], big);
        while c + 1 < chars.len() && class(chars[c + 1], big) == k {
            c += 1;
        }
        (1..n).fold(at(self.cur.line, c), |p, _| self.word_end(p, big))
    }

    fn word_end(&self, p: Pos, big: bool) -> Pos {
        let last = self.last_line();
        let (mut l, mut c) = (p.line, p.col + 1);
        loop {
            let chars = self.chars(l);
            while c < chars.len() && class(chars[c], big) == Class::Blank {
                c += 1;
            }
            if c < chars.len() {
                let k = class(chars[c], big);
                while c + 1 < chars.len() && class(chars[c + 1], big) == k {
                    c += 1;
                }
                return at(l, c);
            }
            if l >= last {
                return at(last, self.len(last).saturating_sub(1));
            }
            l += 1;
            c = 0;
        }
    }

    fn find_char(&self, c: char, fwd: bool, till: bool, n: usize, repeat: bool) -> Option<Pos> {
        let chars = self.chars(self.cur.line);
        let step: isize = if fwd { 1 } else { -1 };
        let mut i = self.cur.col as isize;
        // Repeating a `t` must not get stuck on the char next to the cursor.
        if till && repeat {
            i += step;
        }
        let mut found = 0;
        while found < n {
            i += step;
            if i < 0 || i as usize >= chars.len() {
                return None;
            }
            if chars[i as usize] == c {
                found += 1;
            }
        }
        Some(at(
            self.cur.line,
            if till {
                (i - step) as usize
            } else {
                i as usize
            },
        ))
    }

    /// The unmatched `open` at or before `from`.
    fn find_open(&self, from: Pos, open: char, close: char) -> Option<Pos> {
        let mut depth = 0;
        let mut l = from.line;
        loop {
            let chars = self.chars(l);
            let end = if l == from.line {
                (from.col + 1).min(chars.len())
            } else {
                chars.len()
            };
            for i in (0..end).rev() {
                let on_cursor = l == from.line && i == from.col;
                if chars[i] == open {
                    if depth == 0 {
                        return Some(at(l, i));
                    }
                    depth -= 1;
                } else if chars[i] == close && !on_cursor {
                    depth += 1;
                }
            }
            if l == 0 {
                return None;
            }
            l -= 1;
        }
    }

    /// The `close` matching the `open` at `from`.
    fn find_close(&self, from: Pos, open: char, close: char) -> Option<Pos> {
        let mut depth = 0;
        let mut start = from.col + 1;
        for l in from.line..self.nlines() {
            let chars = self.chars(l);
            for (i, &c) in chars.iter().enumerate().skip(start) {
                if c == close {
                    if depth == 0 {
                        return Some(at(l, i));
                    }
                    depth -= 1;
                } else if c == open {
                    depth += 1;
                }
            }
            start = 0;
        }
        None
    }

    fn match_pair(&self) -> Option<Pos> {
        const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];
        let line = self.cur.line;
        let (col, c) = self
            .line(line)
            .chars()
            .enumerate()
            .skip(self.cur.col)
            .find(|(_, c)| "()[]{}".contains(*c))?;
        let (open, close) = *PAIRS.iter().find(|(o, cl)| *o == c || *cl == c)?;
        if c == open {
            self.find_close(at(line, col), open, close)
        } else {
            self.find_open(at(line, col), open, close)
        }
    }

    /// The stretch of text a text object covers at the cursor.
    pub(crate) fn object_span(&self, o: Obj) -> Option<Span> {
        let Pos { line, col } = self.cur;
        match o {
            Obj::Word { big, around } => {
                let chars = self.chars(line);
                if chars.is_empty() {
                    return None;
                }
                let n = chars.len();
                let is = |i: usize, k: Class| class(chars[i], big) == k;
                let col = col.min(n - 1);
                let k = class(chars[col], big);
                let (mut s, mut e) = (col, col + 1);
                while s > 0 && is(s - 1, k) {
                    s -= 1;
                }
                while e < n && is(e, k) {
                    e += 1;
                }
                if around && k == Class::Blank {
                    if e < n {
                        let word = class(chars[e], big);
                        while e < n && is(e, word) {
                            e += 1;
                        }
                    }
                } else if around && e < n && is(e, Class::Blank) {
                    while e < n && is(e, Class::Blank) {
                        e += 1;
                    }
                } else if around {
                    while s > 0 && is(s - 1, Class::Blank) {
                        s -= 1;
                    }
                }
                Some(Span::Chars(at(line, s), at(line, e)))
            }
            Obj::Pair {
                open,
                close,
                around,
            } => {
                let o = self.find_open(self.cur, open, close)?;
                let c = self.find_close(o, open, close)?;
                if around {
                    return Some(Span::Chars(o, at(c.line, c.col + 1)));
                }
                // A block whose braces sit on their own lines is linewise.
                let open_ends_line = o.col + 1 >= self.len(o.line);
                let close_starts_line = self
                    .line(c.line)
                    .chars()
                    .take(c.col)
                    .all(char::is_whitespace);
                if c.line > o.line && open_ends_line && close_starts_line {
                    return (c.line - o.line > 1).then(|| Span::Lines(o.line + 1, c.line - 1));
                }
                Some(Span::Chars(at(o.line, o.col + 1), c))
            }
            Obj::Quote { q, around } => {
                let quotes: Vec<usize> = self
                    .line(line)
                    .chars()
                    .enumerate()
                    .filter(|(_, c)| *c == q)
                    .map(|(i, _)| i)
                    .collect();
                let pair = quotes.as_chunks::<2>().0.iter().find(|p| col <= p[1])?;
                Some(if around {
                    Span::Chars(at(line, pair[0]), at(line, pair[1] + 1))
                } else {
                    Span::Chars(at(line, pair[0] + 1), at(line, pair[1]))
                })
            }
            Obj::Para { around } => {
                if self.nlines() == 0 {
                    return None;
                }
                let last = self.last_line();
                let blank = |l: usize| self.line(l).trim().is_empty();
                let k = blank(line);
                let (mut a, mut b) = (line, line);
                while a > 0 && blank(a - 1) == k {
                    a -= 1;
                }
                while b < last && blank(b + 1) == k {
                    b += 1;
                }
                while around && b < last && blank(b + 1) != k {
                    b += 1;
                }
                Some(Span::Lines(a, b))
            }
        }
    }

    /// The next match of `pat` from the cursor, wrapping around.
    /// Case-insensitive unless the pattern contains an uppercase char.
    pub(crate) fn find(&self, pat: &str, fwd: bool) -> Option<Pos> {
        let n = self.nlines();
        if n == 0 || pat.is_empty() {
            return None;
        }
        let fold = !pat.chars().any(char::is_uppercase);
        let needle = if fold {
            pat.to_lowercase()
        } else {
            pat.to_string()
        };
        let matches = |l: usize| -> Vec<usize> {
            let hay = if fold {
                self.line(l).to_lowercase()
            } else {
                self.line(l).to_string()
            };
            hay.match_indices(&needle)
                .map(|(b, _)| char_len(&hay[..b]))
                .collect()
        };
        let cur = self.cur;
        for off in 0..=n {
            let l = if fwd {
                (cur.line + off) % n
            } else {
                (cur.line + n - off % n) % n
            };
            let mut cols = matches(l);
            if !fwd {
                cols.reverse();
            }
            for c in cols {
                let behind = if fwd { c <= cur.col } else { c >= cur.col };
                if off == 0 && behind {
                    continue;
                }
                return Some(at(l, c));
            }
        }
        None
    }

    /// The word under the cursor, for `*` and `#`.
    pub(crate) fn word_under_cursor(&self) -> Option<String> {
        match self.object_span(Obj::Word {
            big: false,
            around: false,
        })? {
            Span::Chars(s, e) => {
                let word: String = self.chars(s.line)[s.col..e.col].iter().collect();
                (!word.trim().is_empty()).then_some(word)
            }
            Span::Lines(..) => None,
        }
    }
}
