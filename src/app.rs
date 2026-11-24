//! Application state and key handling: vim-style editing of the
//! result, conflict commands, leader menu, saving and quitting.

use std::ops::Range;
use std::path::PathBuf;

use crate::doc::{Doc, OUT, Pos, RIGHT, Side};
pub use crate::keys::Key;
use crate::keys::{Act, Cmd, Motion, Op, Parse, Target, parse};
use crate::layout::Layout;
use crate::text::{
    Class, byte_at, char_len, class, display_col, emphasis, first_non_blank, indent_of,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    Insert,
    Visual,
    VisualLine,
    /// Typing a `:` command.
    Cmd,
    /// Typing a search pattern; `true` searches forward.
    Search(bool),
    Leader,
    Help,
}

/// How an operator treats the text between cursor and motion target.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MKind {
    Excl,
    Incl,
    Line,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Span {
    /// From the first position up to, not including, the second.
    Chars(Pos, Pos),
    /// Whole lines, inclusive.
    Lines(usize, usize),
}

#[derive(Default)]
struct Register {
    lines: Vec<String>,
    linewise: bool,
}

/// The `<space>` menu: key and description.
pub const LEADER_MENU: &[(char, &str)] = &[
    ('a', "Apply all non-conflicting changes"),
    ('l', "Apply non-conflicting changes from the left"),
    ('r', "Apply non-conflicting changes from the right"),
    ('m', "Resolve simple conflicts automatically"),
    ('L', "Accept the left file as the whole result"),
    ('R', "Accept the right file as the whole result"),
    ('u', "Reset the change under the cursor to the base"),
    ('w', "Toggle word-level highlighting"),
    ('?', "Show all key bindings"),
];

pub struct App {
    pub doc: Doc,
    pub pane: usize,
    pub cur: Pos,
    pub(crate) want_col: usize,
    /// First visible aligned row.
    pub top: usize,
    pub hscroll: usize,
    pub view_h: usize,
    pub text_w: usize,
    pub mode: Mode,
    /// Keys of an unfinished normal-mode command.
    pub keys: Vec<Key>,
    /// Where the visual selection started.
    pub anchor: Pos,
    /// Text typed after `:`, `/` or `?`.
    pub input: String,
    /// Status message and whether it is an error.
    pub message: Option<(String, bool)>,
    /// Exit code once the app should quit.
    pub quit: Option<i32>,
    pub written: bool,
    pub path: PathBuf,
    pub eol: &'static str,
    pub final_newline: bool,
    pub word_hl: bool,
    pub titles: [String; 3],
    /// Per line of the left and right file: char ranges that differ from the base.
    pub emph: [Vec<Vec<Range<usize>>>; 2],
    reg: Register,
    indent: String,
    last_search: Option<(String, bool)>,
    pub(crate) last_find: Option<(char, bool, bool)>,
    last_change: Vec<Key>,
    recording: Option<Vec<Key>>,
    replaying: bool,
    free_scroll: bool,
}

fn head(s: &str, col: usize) -> &str {
    &s[..byte_at(s, col)]
}

fn tail(s: &str, col: usize) -> &str {
    &s[byte_at(s, col)..]
}

fn detect_indent(lines: &[String]) -> String {
    let mut width = usize::MAX;
    for l in lines {
        if l.starts_with('\t') {
            return "\t".into();
        }
        let n = l.len() - l.trim_start_matches(' ').len();
        if n > 0 && n < l.len() {
            width = width.min(n);
        }
    }
    " ".repeat(if width == usize::MAX {
        2
    } else {
        width.clamp(2, 8)
    })
}

fn plural(n: usize, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}

impl App {
    pub fn new(doc: Doc, path: PathBuf) -> App {
        let mut emph = [
            vec![Vec::new(); doc.left.len()],
            vec![Vec::new(); doc.right.len()],
        ];
        for c in &doc.chunks {
            let sides = [(0, &doc.left, &c.left), (1, &doc.right, &c.right)];
            for (i, lines, range) in sides {
                if c.base.is_empty() || range.is_empty() {
                    continue;
                }
                let marks = emphasis(&doc.base[c.base.clone()], &lines[range.clone()]);
                emph[i].splice(range.clone(), marks);
            }
        }
        let mut app = App {
            indent: detect_indent(&doc.left),
            doc,
            pane: OUT,
            cur: Pos::default(),
            want_col: 0,
            top: 0,
            hscroll: 0,
            view_h: 20,
            text_w: 40,
            mode: Mode::Normal,
            keys: Vec::new(),
            anchor: Pos::default(),
            input: String::new(),
            message: None,
            quit: None,
            written: false,
            path,
            eol: "\n",
            final_newline: true,
            word_hl: true,
            titles: ["Local".into(), "Result".into(), "Incoming".into()],
            emph,
            reg: Register::default(),
            last_search: None,
            last_find: None,
            last_change: Vec::new(),
            recording: None,
            replaying: false,
            free_scroll: false,
        };
        if let Some(first) = app.doc.chunks.iter().position(|c| !c.resolved()) {
            app.goto_chunk(first);
        }
        app
    }

    // ----- buffer access ---------------------------------------------

    pub(crate) fn lines(&self) -> &[String] {
        self.doc.pane_lines(self.pane)
    }

    pub(crate) fn nlines(&self) -> usize {
        self.lines().len()
    }

    pub(crate) fn last_line(&self) -> usize {
        self.nlines().saturating_sub(1)
    }

    pub(crate) fn line(&self, l: usize) -> &str {
        self.lines().get(l).map_or("", String::as_str)
    }

    pub(crate) fn len(&self, l: usize) -> usize {
        char_len(self.line(l))
    }

    pub(crate) fn layout(&self) -> Layout {
        Layout::build(&self.doc)
    }

    pub fn is_visual(&self) -> bool {
        matches!(self.mode, Mode::Visual | Mode::VisualLine)
    }

    /// The chunk under the cursor: the target of the conflict commands.
    pub fn active_chunk(&self) -> Option<usize> {
        self.doc.chunk_at(self.pane, self.cur.line)
    }

    fn msg(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), false));
    }

    fn err(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), true));
    }

    /// Reports and returns true if the focused pane cannot be edited.
    fn read_only(&mut self) -> bool {
        if self.pane != OUT {
            self.err("This pane is read-only; edit the result in the middle (^h / ^l)");
        }
        self.pane != OUT
    }

    /// The buffer needs a line before text can be typed into it.
    fn ensure_line(&mut self) {
        if self.doc.lines.is_empty() {
            self.doc.splice(0, 0, vec![String::new()]);
            self.cur = Pos::default();
        }
    }

    /// The result position on the cursor's row, to return to on undo.
    fn out_cursor(&self) -> Pos {
        if self.pane == OUT {
            return self.cur;
        }
        let l = self.layout();
        Pos {
            line: l.nearest_line(OUT, l.row_of(self.pane, self.cur.line)),
            col: 0,
        }
    }

    // ----- key dispatch ----------------------------------------------

    pub fn handle_key(&mut self, key: Key) {
        self.message = None;
        self.free_scroll = false;
        self.dispatch(key);
        if !self.free_scroll {
            self.follow_cursor();
        }
        self.follow_hscroll();
    }

    fn dispatch(&mut self, key: Key) {
        match self.mode {
            Mode::Normal | Mode::Visual | Mode::VisualLine => self.normal_key(key),
            Mode::Insert => self.insert_key(key),
            Mode::Cmd | Mode::Search(_) => self.input_key(key),
            Mode::Leader => self.leader_key(key),
            Mode::Help => self.mode = Mode::Normal,
        }
        self.clamp_cursor();
    }

    fn clamp_cursor(&mut self) {
        self.cur.line = self.cur.line.min(self.last_line());
        let len = self.len(self.cur.line);
        self.cur.col = self.cur.col.min(if self.mode == Mode::Insert {
            len
        } else {
            len.saturating_sub(1)
        });
    }

    fn normal_key(&mut self, key: Key) {
        if matches!(key, Key::Esc | Key::Ctrl('c')) {
            if self.keys.is_empty() && self.is_visual() {
                self.mode = Mode::Normal;
            }
            self.keys.clear();
            return;
        }
        self.keys.push(key);
        match parse(&self.keys, self.is_visual()) {
            Parse::Incomplete => {}
            Parse::Invalid => self.keys.clear(),
            Parse::Done(count, cmd) => {
                let keys = std::mem::take(&mut self.keys);
                self.doc.begin(self.out_cursor());
                let changed = self.exec(count, cmd);
                if self.mode == Mode::Insert {
                    // The change continues until insert mode is left.
                    if !self.replaying {
                        self.recording = Some(keys);
                    }
                } else {
                    self.clamp_cursor();
                    self.doc.commit(self.out_cursor());
                    if changed && !self.replaying {
                        self.last_change = keys;
                    }
                }
            }
        }
    }

    /// Runs a command; returns whether it is a change `.` can repeat.
    fn exec(&mut self, count: Option<usize>, cmd: Cmd) -> bool {
        if self.is_visual() {
            self.exec_visual(count, cmd);
            return false;
        }
        match cmd {
            Cmd::Move(m) => {
                self.do_move(m, count);
                false
            }
            Cmd::Operate(op, target) => self.operate(op, target, count),
            Cmd::Select(_) => false,
            Cmd::Act(a) => self.act(a, count),
        }
    }

    fn do_move(&mut self, m: Motion, count: Option<usize>) {
        if let Motion::Find { c, fwd, till } = m {
            self.last_find = Some((c, fwd, till));
        }
        let Some((p, _)) = self.target(m, count, false) else {
            return;
        };
        self.cur = p;
        match m {
            Motion::Down | Motion::Up => {}
            Motion::Eol => self.want_col = usize::MAX,
            _ => self.want_col = p.col,
        }
    }

    // ----- operators -------------------------------------------------

    fn operate(&mut self, op: Op, target: Target, count: Option<usize>) -> bool {
        if op != Op::Yank && self.read_only() {
            return false;
        }
        if self.nlines() == 0 {
            return false;
        }
        let span = match target {
            Target::Lines => Span::Lines(
                self.cur.line,
                (self.cur.line + count.unwrap_or(1) - 1).min(self.last_line()),
            ),
            Target::Obj(o) => match self.object_span(o) {
                Some(s) => s,
                None => return false,
            },
            Target::Motion(m) => {
                if let Motion::Find { c, fwd, till } = m {
                    self.last_find = Some((c, fwd, till));
                }
                match (op, m) {
                    // `cw` on a word changes up to the end of that word, not the space after.
                    (Op::Change, Motion::Word(big)) if self.on_non_blank() => {
                        let end = self.change_word_end(big, count.unwrap_or(1));
                        self.span_between(self.cur, end, MKind::Incl)
                    }
                    _ => {
                        let Some((p, kind)) = self.target(m, count, true) else {
                            return false;
                        };
                        self.span_between(self.cur, p, kind)
                    }
                }
            }
        };
        self.apply_op(op, span);
        op != Op::Yank
    }

    fn span_between(&self, a: Pos, b: Pos, kind: MKind) -> Span {
        let (s, mut e) = (a.min(b), a.max(b));
        match kind {
            MKind::Line => Span::Lines(s.line, e.line),
            MKind::Incl => {
                e.col = (e.col + 1).min(self.len(e.line));
                Span::Chars(s, e)
            }
            MKind::Excl => {
                // Ending at the start of a line means ending at the previous line's end.
                if e.col == 0 && e.line > s.line {
                    e = Pos {
                        line: e.line - 1,
                        col: self.len(e.line - 1),
                    };
                }
                Span::Chars(s, e)
            }
        }
    }

    fn apply_op(&mut self, op: Op, span: Span) {
        if self.nlines() == 0 {
            return;
        }
        match op {
            Op::Yank => {
                self.yank(span);
                match span {
                    Span::Chars(s, _) => self.cur = s,
                    Span::Lines(a, _) => self.cur.line = a,
                }
            }
            Op::Delete => {
                self.yank(span);
                self.delete(span);
            }
            Op::Change => {
                self.yank(span);
                match span {
                    Span::Chars(..) => self.delete(span),
                    Span::Lines(a, b) => {
                        let indent = indent_of(self.line(a)).to_string();
                        self.cur = Pos {
                            line: a,
                            col: char_len(&indent),
                        };
                        self.doc.splice(a, b + 1, vec![indent]);
                    }
                }
                self.mode = Mode::Insert;
            }
            Op::Indent | Op::Dedent => {
                let (a, b) = match span {
                    Span::Chars(s, e) => (s.line, e.line),
                    Span::Lines(a, b) => (a, b),
                };
                for l in a..=b {
                    let line = self.line(l);
                    let new = if op == Op::Indent {
                        if line.is_empty() {
                            String::new()
                        } else {
                            format!("{}{line}", self.indent)
                        }
                    } else if let Some(rest) = line.strip_prefix(&self.indent) {
                        rest.to_string()
                    } else {
                        let n = indent_of(line).len().min(self.indent.len());
                        line[n..].to_string()
                    };
                    self.doc.splice(l, l + 1, vec![new]);
                }
                self.cur = Pos {
                    line: a,
                    col: first_non_blank(self.line(a)),
                };
            }
        }
        self.want_col = self.cur.col;
    }

    fn yank(&mut self, span: Span) {
        let lines = self.lines();
        match span {
            Span::Lines(a, b) if a < lines.len() => {
                self.reg = Register {
                    lines: lines[a..=b.min(lines.len() - 1)].to_vec(),
                    linewise: true,
                };
            }
            Span::Chars(s, e) if s != e => {
                let text = if s.line == e.line {
                    vec![head(tail(self.line(s.line), s.col), e.col - s.col).to_string()]
                } else {
                    let mut v = vec![tail(self.line(s.line), s.col).to_string()];
                    v.extend(lines[s.line + 1..e.line].iter().cloned());
                    v.push(head(self.line(e.line), e.col).to_string());
                    v
                };
                self.reg = Register {
                    lines: text,
                    linewise: false,
                };
            }
            _ => {}
        }
    }

    fn delete(&mut self, span: Span) {
        match span {
            Span::Chars(s, e) => {
                if s != e {
                    let joined = format!(
                        "{}{}",
                        head(self.line(s.line), s.col),
                        tail(self.line(e.line), e.col)
                    );
                    self.doc.splice(s.line, e.line + 1, vec![joined]);
                }
                self.cur = s;
            }
            Span::Lines(a, b) => {
                self.doc.splice(a, b + 1, vec![]);
                let line = a.min(self.last_line());
                self.cur = Pos {
                    line,
                    col: first_non_blank(self.line(line)),
                };
            }
        }
    }

    fn paste(&mut self, after: bool, count: usize) {
        if self.reg.lines.is_empty() {
            return;
        }
        let text = self.reg.lines.clone();
        if self.reg.linewise {
            let at = if self.nlines() == 0 {
                0
            } else {
                self.cur.line + after as usize
            };
            self.cur = Pos {
                line: at,
                col: first_non_blank(&text[0]),
            };
            self.doc.splice(
                at,
                at,
                text.iter()
                    .cycle()
                    .take(text.len() * count)
                    .cloned()
                    .collect(),
            );
        } else {
            self.ensure_line();
            let col = (self.cur.col + after as usize).min(self.len(self.cur.line));
            self.paste_chars(
                Pos {
                    line: self.cur.line,
                    col,
                },
                &text,
                count,
            );
        }
    }

    fn paste_chars(&mut self, at: Pos, pieces: &[String], count: usize) {
        let line = self.line(at.line).to_string();
        let (h, t) = (head(&line, at.col), tail(&line, at.col));
        if let [piece] = pieces {
            let ins = piece.repeat(count);
            self.cur = Pos {
                line: at.line,
                col: (at.col + char_len(&ins)).saturating_sub(1),
            };
            self.doc
                .splice(at.line, at.line + 1, vec![format!("{h}{ins}{t}")]);
        } else {
            let last = pieces.len() - 1;
            let mut new = vec![format!("{h}{}", pieces[0])];
            new.extend(pieces[1..last].iter().cloned());
            new.push(format!("{}{t}", pieces[last]));
            self.doc.splice(at.line, at.line + 1, new);
            self.cur = at;
        }
    }

    /// Joins `count` lines starting at `first` with single spaces.
    fn join(&mut self, first: usize, count: usize) {
        let last = (first + count.max(2) - 1).min(self.last_line());
        if last <= first {
            return;
        }
        let mut joined = self.line(first).to_string();
        let mut col = char_len(&joined).saturating_sub(1);
        for l in first + 1..=last {
            let next = self.line(l).trim_start();
            col = char_len(&joined);
            if !joined.is_empty() && !next.is_empty() && !joined.ends_with(char::is_whitespace) {
                joined.push(' ');
            }
            joined.push_str(next);
        }
        self.doc.splice(first, last + 1, vec![joined]);
        self.cur = Pos { line: first, col };
    }

    /// Rewrites `n` chars from the cursor with `f`; returns false if the line is too short.
    fn map_chars(&mut self, n: usize, f: impl Fn(char) -> char) -> bool {
        let Pos { line, col } = self.cur;
        if col + n > self.len(line) {
            return false;
        }
        let new: String = self
            .line(line)
            .chars()
            .enumerate()
            .map(|(i, c)| if (col..col + n).contains(&i) { f(c) } else { c })
            .collect();
        self.doc.splice(line, line + 1, vec![new]);
        true
    }

    // ----- visual mode -----------------------------------------------

    fn visual_span(&self) -> Span {
        let (s, e) = (self.anchor.min(self.cur), self.anchor.max(self.cur));
        if self.mode == Mode::VisualLine {
            Span::Lines(s.line, e.line)
        } else {
            Span::Chars(
                s,
                Pos {
                    line: e.line,
                    col: (e.col + 1).min(self.len(e.line)),
                },
            )
        }
    }

    /// The selection as inclusive (start, end) positions, for drawing.
    pub fn selection(&self) -> Option<(Pos, Pos)> {
        let (s, e) = (self.anchor.min(self.cur), self.anchor.max(self.cur));
        match self.mode {
            Mode::Visual => Some((s, e)),
            Mode::VisualLine => Some((
                Pos {
                    line: s.line,
                    col: 0,
                },
                Pos {
                    line: e.line,
                    col: usize::MAX,
                },
            )),
            _ => None,
        }
    }

    fn exec_visual(&mut self, count: Option<usize>, cmd: Cmd) {
        use Act::*;
        let act = match cmd {
            Cmd::Move(m) => return self.do_move(m, count),
            Cmd::Select(o) => {
                match self.object_span(o) {
                    Some(Span::Chars(s, e)) if s != e => {
                        self.anchor = s;
                        self.cur = if e.col == 0 {
                            Pos {
                                line: e.line - 1,
                                col: self.len(e.line - 1).saturating_sub(1),
                            }
                        } else {
                            Pos {
                                line: e.line,
                                col: e.col - 1,
                            }
                        };
                    }
                    Some(Span::Lines(a, b)) => {
                        self.mode = Mode::VisualLine;
                        self.anchor = Pos { line: a, col: 0 };
                        self.cur = Pos { line: b, col: 0 };
                    }
                    _ => {}
                }
                return;
            }
            Cmd::Operate(op, _) => return self.visual_op(op),
            Cmd::Act(a) => a,
        };
        match act {
            DelChar | DeleteEol => self.visual_op(Op::Delete),
            Subst | SubstLine | ChangeEol => self.visual_op(Op::Change),
            YankLine => self.visual_op(Op::Yank),
            Visual => {
                self.mode = if self.mode == Mode::Visual {
                    Mode::Normal
                } else {
                    Mode::Visual
                }
            }
            VisualLine => {
                self.mode = if self.mode == Mode::VisualLine {
                    Mode::Normal
                } else {
                    Mode::VisualLine
                }
            }
            OpenBelow => std::mem::swap(&mut self.anchor, &mut self.cur),
            Join | ToggleCase | PasteAfter | PasteBefore => {
                let span = self.visual_span();
                self.mode = Mode::Normal;
                if self.read_only() {
                    return;
                }
                match (act, span) {
                    (Join, Span::Chars(s, e)) => self.join(s.line, e.line - s.line + 1),
                    (Join, Span::Lines(a, b)) => self.join(a, b - a + 1),
                    (ToggleCase, _) => self.toggle_case_in(span),
                    _ => self.paste_over(span),
                }
            }
            // Navigation keeps the selection.
            NextConflict(_) | NextChunk(_) | SearchNext(_) | SearchWord(_) | HalfDown | HalfUp
            | PageDown | PageUp | LineDown | LineUp | Center | ScrollTop | ScrollBottom => {
                self.act(act, count);
            }
            InsertBefore | Append | InsertBol | AppendEol | OpenAbove | Replace(_) | Repeat => {}
            _ => {
                self.mode = Mode::Normal;
                self.act(act, count);
            }
        }
    }

    fn visual_op(&mut self, op: Op) {
        let span = self.visual_span();
        self.mode = Mode::Normal;
        if op == Op::Yank || !self.read_only() {
            self.apply_op(op, span);
        }
    }

    fn toggle_case_in(&mut self, span: Span) {
        let (s, e) = match span {
            Span::Chars(s, e) => (s, e),
            Span::Lines(a, b) => (
                Pos { line: a, col: 0 },
                Pos {
                    line: b,
                    col: usize::MAX,
                },
            ),
        };
        for l in s.line..=e.line {
            let from = if l == s.line { s.col } else { 0 };
            let to = if l == e.line { e.col } else { usize::MAX };
            let new: String = self
                .line(l)
                .chars()
                .enumerate()
                .map(|(i, c)| if i >= from && i < to { flip(c) } else { c })
                .collect();
            self.doc.splice(l, l + 1, vec![new]);
        }
        self.cur = s;
    }

    /// Replaces the selection with the register, as visual `p` does.
    fn paste_over(&mut self, span: Span) {
        let text = std::mem::take(&mut self.reg);
        if text.lines.is_empty() {
            self.reg = text;
            return;
        }
        self.yank(span);
        if self.nlines() > 0 {
            self.delete(span);
        }
        match (span, text.linewise) {
            (Span::Lines(a, _), _) => {
                self.doc.splice(a, a, text.lines);
                self.cur = Pos { line: a, col: 0 };
            }
            (Span::Chars(s, _), true) => {
                let at = (s.line + 1).min(self.doc.lines.len());
                self.doc.splice(at, at, text.lines);
                self.cur = Pos { line: at, col: 0 };
            }
            (Span::Chars(s, _), false) => {
                self.ensure_line();
                self.paste_chars(s, &text.lines, 1);
            }
        }
    }

    // ----- single commands -------------------------------------------

    fn act(&mut self, act: Act, count: Option<usize>) -> bool {
        use Act::*;
        let n = count.unwrap_or(1);
        match act {
            InsertBefore | Append | InsertBol | AppendEol | OpenBelow | OpenAbove => {
                if self.read_only() {
                    return false;
                }
                let empty = self.doc.lines.is_empty();
                self.ensure_line();
                let Pos { line, col } = self.cur;
                let len = self.len(line);
                let indent = indent_of(self.line(line)).to_string();
                match act {
                    Append => self.cur.col = (col + 1).min(len),
                    InsertBol => self.cur.col = char_len(&indent),
                    AppendEol => self.cur.col = len,
                    OpenBelow | OpenAbove if !empty => {
                        let at = line + (act == OpenBelow) as usize;
                        self.cur = Pos {
                            line: at,
                            col: char_len(&indent),
                        };
                        self.doc.splice(at, at, vec![indent]);
                    }
                    _ => {}
                }
                self.mode = Mode::Insert;
                return true;
            }
            DelChar => return self.operate(Op::Delete, Target::Motion(Motion::Right), count),
            Subst => return self.operate(Op::Change, Target::Motion(Motion::Right), count),
            SubstLine => return self.operate(Op::Change, Target::Lines, count),
            ChangeEol => return self.operate(Op::Change, Target::Motion(Motion::Eol), count),
            DeleteEol => return self.operate(Op::Delete, Target::Motion(Motion::Eol), count),
            YankLine => {
                self.operate(Op::Yank, Target::Lines, count);
            }
            Join => {
                if !self.read_only() {
                    self.join(self.cur.line, n);
                    return true;
                }
            }
            PasteAfter | PasteBefore => {
                if !self.read_only() {
                    self.paste(act == PasteAfter, n);
                    return true;
                }
            }
            Replace(c) => {
                if !self.read_only() && self.map_chars(n, |_| c) {
                    self.cur.col += n - 1;
                    return true;
                }
            }
            ToggleCase => {
                let n = n.min(self.len(self.cur.line) - self.cur.col.min(self.len(self.cur.line)));
                if !self.read_only() && n > 0 && self.map_chars(n, flip) {
                    self.cur.col += n;
                    return true;
                }
            }
            Undo | Redo => {
                for _ in 0..n {
                    let pos = if act == Undo {
                        self.doc.undo()
                    } else {
                        self.doc.redo()
                    };
                    match pos {
                        Some(p) if self.pane == OUT => self.cur = p,
                        Some(_) => {}
                        None => {
                            self.msg(if act == Undo {
                                "Already at the oldest change"
                            } else {
                                "Already at the newest change"
                            });
                            break;
                        }
                    }
                }
            }
            Repeat => {
                self.replaying = true;
                for key in self.last_change.clone() {
                    self.dispatch(key);
                }
                self.replaying = false;
            }
            Visual | VisualLine => {
                self.anchor = self.cur;
                self.mode = if act == Visual {
                    Mode::Visual
                } else {
                    Mode::VisualLine
                };
            }
            CmdLine => self.prompt(Mode::Cmd),
            Search(fwd) => self.prompt(Mode::Search(fwd)),
            SearchNext(same) => match self.last_search.clone() {
                Some((pat, fwd)) => self.search(&pat, fwd == same),
                None => self.err("No previous search"),
            },
            SearchWord(fwd) => {
                if let Some(word) = self.word_under_cursor() {
                    self.last_search = Some((word.clone(), fwd));
                    self.search(&word, fwd);
                }
            }
            NextConflict(fwd) => self.jump_chunk(fwd, true, n),
            NextChunk(fwd) => self.jump_chunk(fwd, false, n),
            AcceptLeft => self.resolve(Side::Left, true),
            AcceptRight => self.resolve(Side::Right, true),
            IgnoreLeft => self.resolve(Side::Left, false),
            IgnoreRight => self.resolve(Side::Right, false),
            Leader => self.mode = Mode::Leader,
            WriteQuit => self.write_quit(false),
            QuitNoSave => self.quit = Some(self.exit_code()),
            HalfDown => self.scroll_page(true, (self.view_h / 2).max(1)),
            HalfUp => self.scroll_page(false, (self.view_h / 2).max(1)),
            PageDown => self.scroll_page(true, self.view_h.saturating_sub(2).max(1)),
            PageUp => self.scroll_page(false, self.view_h.saturating_sub(2).max(1)),
            LineDown => self.scroll_view(true, n),
            LineUp => self.scroll_view(false, n),
            Center | ScrollTop | ScrollBottom => {
                let row = self.layout().row_of(self.pane, self.cur.line);
                self.top = match act {
                    Center => row.saturating_sub(self.view_h / 2),
                    ScrollTop => row,
                    _ => (row + 1).saturating_sub(self.view_h),
                };
                self.free_scroll = true;
            }
            PaneLeft => self.switch_pane(self.pane.saturating_sub(1)),
            PaneRight => self.switch_pane((self.pane + 1).min(RIGHT)),
            PaneNext => self.switch_pane((self.pane + 1) % 3),
        }
        false
    }

    fn prompt(&mut self, mode: Mode) {
        self.input.clear();
        self.mode = mode;
    }

    fn search(&mut self, pat: &str, fwd: bool) {
        match self.find(pat, fwd) {
            Some(p) => {
                self.cur = p;
                self.want_col = p.col;
            }
            None => self.err(format!("Pattern not found: {pat}")),
        }
    }

    fn switch_pane(&mut self, target: usize) {
        if target == self.pane {
            return;
        }
        if self.is_visual() {
            self.mode = Mode::Normal;
        }
        let l = self.layout();
        let row = l.row_of(self.pane, self.cur.line);
        self.pane = target;
        self.cur = Pos {
            line: l.nearest_line(target, row),
            col: self.want_col,
        };
    }

    // ----- conflicts -------------------------------------------------

    fn goto_chunk(&mut self, idx: usize) {
        let start = self.doc.chunks[idx].range(self.pane).start;
        self.cur = Pos {
            line: start.min(self.last_line()),
            col: 0,
        };
        self.want_col = 0;
        // Show the whole chunk with a little context where it fits.
        let (row, rows) = self.layout().chunk_rows(idx);
        let h = self.view_h.max(1);
        let margin = 2.min(h / 4);
        if row < self.top + margin {
            self.top = row.saturating_sub(margin);
        }
        if row + rows + margin > self.top + h {
            self.top = (row + rows + margin - h).min(row.saturating_sub(margin));
        }
    }

    fn jump_chunk(&mut self, fwd: bool, unresolved_only: bool, times: usize) {
        let chunks = &self.doc.chunks;
        let all: Vec<usize> = (0..chunks.len())
            .filter(|&i| !unresolved_only || !chunks[i].resolved())
            .collect();
        let (Some(&first), Some(&last)) = (all.first(), all.last()) else {
            self.msg(if unresolved_only {
                "All conflicts are resolved"
            } else {
                "No changes"
            });
            return;
        };
        let mut here = self.active_chunk();
        let line = self.cur.line;
        let pane = self.pane;
        for _ in 0..times {
            let start = |i: usize| self.doc.chunks[i].range(pane).start;
            let next = if fwd {
                all.iter()
                    .copied()
                    .find(|&i| here.map_or(start(i) > line, |h| i > h))
            } else {
                all.iter()
                    .rev()
                    .copied()
                    .find(|&i| here.map_or(start(i) < line, |h| i < h))
            };
            if next.is_none() && here == Some(if fwd { first } else { last }) && all.len() == 1 {
                self.msg("No other unresolved conflict");
                return;
            }
            if next.is_none() {
                self.msg(if fwd {
                    "Wrapped around to the first"
                } else {
                    "Wrapped around to the last"
                });
            }
            here = Some(next.unwrap_or(if fwd { first } else { last }));
        }
        if let Some(i) = here {
            self.goto_chunk(i);
        }
    }

    fn resolve(&mut self, side: Side, accept: bool) {
        let Some(i) = self.active_chunk() else {
            self.err("No conflict under the cursor (n jumps to the next one)");
            return;
        };
        let result = if accept {
            self.doc.accept(i, side)
        } else {
            self.doc.ignore(i, side)
        };
        match result {
            Err(e) => self.err(e),
            Ok(()) => {
                self.stay_on(i);
                self.msg(format!(
                    "{} {}",
                    if accept { "Accepted" } else { "Ignored" },
                    side.name()
                ));
            }
        }
    }

    /// Keeps the cursor on a chunk whose result lines just changed.
    fn stay_on(&mut self, idx: usize) {
        if self.pane == OUT {
            let r = self.doc.chunks[idx].out.clone();
            self.cur.line = self.cur.line.clamp(r.start, r.end.max(r.start + 1) - 1);
        }
    }

    pub fn progress(&self) -> String {
        match self.doc.unresolved() {
            (0, 0) => "All conflicts resolved".into(),
            (c, 0) => format!("{} unresolved", plural(c, "conflict")),
            (0, n) => format!("{} pending", plural(n, "change")),
            (c, n) => format!(
                "{} unresolved, {} pending",
                plural(c, "conflict"),
                plural(n, "change")
            ),
        }
    }

    fn report_progress(&mut self) {
        let text = self.progress();
        self.msg(text);
    }

    fn leader_key(&mut self, key: Key) {
        self.mode = Mode::Normal;
        self.doc.begin(self.out_cursor());
        match key {
            Key::Char('a') => {
                let n = self.doc.apply_non_conflicting(None);
                self.msg(format!("Applied {}", plural(n, "non-conflicting change")));
            }
            Key::Char(c @ ('l' | 'r')) => {
                let side = if c == 'l' { Side::Left } else { Side::Right };
                let n = self.doc.apply_non_conflicting(Some(side));
                self.msg(format!(
                    "Applied {} from the {}",
                    plural(n, "non-conflicting change"),
                    side.name()
                ));
            }
            Key::Char('m') => {
                let n = self.doc.resolve_all_simple();
                self.msg(format!(
                    "Resolved {}; {}",
                    plural(n, "simple conflict"),
                    self.progress()
                ));
            }
            Key::Char(c @ ('L' | 'R')) => {
                let side = if c == 'L' { Side::Left } else { Side::Right };
                self.doc.accept_file(side);
                self.msg(format!(
                    "The result is now the {} file (u to undo)",
                    side.name()
                ));
            }
            Key::Char('u') => match self.active_chunk() {
                Some(i) => {
                    self.doc.reset(i);
                    self.stay_on(i);
                    self.report_progress();
                }
                None => self.err("No change under the cursor"),
            },
            Key::Char('w') => {
                self.word_hl = !self.word_hl;
                self.msg(if self.word_hl {
                    "Word highlighting on"
                } else {
                    "Word highlighting off"
                });
            }
            Key::Char('?') => self.mode = Mode::Help,
            Key::Esc | Key::Ctrl('c') => {}
            _ => self.err("Unknown <space> command"),
        }
        self.clamp_cursor();
        self.doc.commit(self.out_cursor());
    }

    // ----- insert mode -----------------------------------------------

    fn insert_key(&mut self, key: Key) {
        if let Some(rec) = &mut self.recording {
            rec.push(key);
        }
        let Pos { line, col } = self.cur;
        let text = self.line(line).to_string();
        let len = char_len(&text);
        let (h, t) = (head(&text, col), tail(&text, col));
        let set = |app: &mut App, new: String, col: usize| {
            app.doc.splice(line, line + 1, vec![new]);
            app.cur.col = col;
        };
        match key {
            Key::Esc | Key::Ctrl('c') => {
                self.mode = Mode::Normal;
                self.cur.col = col.saturating_sub(1);
                self.want_col = self.cur.col;
                self.doc.commit(self.cur);
                if let Some(rec) = self.recording.take() {
                    self.last_change = rec;
                }
            }
            Key::Char(c) => set(self, format!("{h}{c}{t}"), col + 1),
            Key::Tab => {
                let indent = self.indent.clone();
                set(self, format!("{h}{indent}{t}"), col + char_len(&indent));
            }
            Key::Enter => {
                let indent = indent_of(h);
                self.cur = Pos {
                    line: line + 1,
                    col: char_len(indent),
                };
                self.doc
                    .splice(line, line + 1, vec![h.to_string(), format!("{indent}{t}")]);
            }
            Key::Backspace | Key::Ctrl('h') if col > 0 => {
                set(self, format!("{}{t}", head(&text, col - 1)), col - 1)
            }
            Key::Backspace | Key::Ctrl('h') if line > 0 => {
                let prev = self.line(line - 1).to_string();
                self.cur = Pos {
                    line: line - 1,
                    col: char_len(&prev),
                };
                self.doc.splice(line - 1, line + 1, vec![prev + &text]);
            }
            Key::Delete if col < len => set(self, format!("{h}{}", tail(&text, col + 1)), col),
            Key::Delete if line < self.last_line() => {
                let next = self.line(line + 1).to_string();
                self.doc.splice(line, line + 2, vec![text.clone() + &next]);
            }
            Key::Ctrl('w') => {
                let chars: Vec<char> = h.chars().collect();
                let mut i = chars.len();
                while i > 0 && class(chars[i - 1], false) == Class::Blank {
                    i -= 1;
                }
                if i > 0 {
                    let k = class(chars[i - 1], false);
                    while i > 0 && class(chars[i - 1], false) == k {
                        i -= 1;
                    }
                }
                set(self, format!("{}{t}", head(&text, i)), i);
            }
            Key::Ctrl('u') => set(self, t.to_string(), 0),
            Key::Left => self.cur.col = col.saturating_sub(1),
            Key::Right => self.cur.col = col + 1,
            Key::Up => self.cur.line = line.saturating_sub(1),
            Key::Down => self.cur.line = line + 1,
            Key::Home => self.cur.col = 0,
            Key::End => self.cur.col = len,
            _ => {}
        }
    }

    // ----- command line ----------------------------------------------

    fn input_key(&mut self, key: Key) {
        match key {
            Key::Char(c) => self.input.push(c),
            Key::Ctrl('u') => self.input.clear(),
            Key::Backspace | Key::Ctrl('h') if !self.input.is_empty() => {
                self.input.pop();
            }
            Key::Esc | Key::Ctrl('c') | Key::Backspace | Key::Ctrl('h') => self.mode = Mode::Normal,
            Key::Enter => {
                let text = std::mem::take(&mut self.input);
                let mode = std::mem::replace(&mut self.mode, Mode::Normal);
                match mode {
                    Mode::Search(fwd) if !text.is_empty() => {
                        self.last_search = Some((text.clone(), fwd));
                        self.search(&text, fwd);
                    }
                    Mode::Cmd => self.run_command(text.trim()),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn run_command(&mut self, cmd: &str) {
        match cmd {
            "" => {}
            "w" | "w!" | "write" => {
                self.write();
            }
            "q" | "q!" | "quit" | "qa" | "qa!" => self.quit = Some(self.exit_code()),
            "wq" | "x" | "wqa" | "xa" => self.write_quit(false),
            "wq!" | "x!" | "wqa!" | "xa!" => self.write_quit(true),
            "cq" | "cq!" => self.quit = Some(1),
            "help" | "h" => self.mode = Mode::Help,
            _ => match cmd.parse::<usize>() {
                Ok(n) => {
                    let line = n.saturating_sub(1).min(self.last_line());
                    self.cur = Pos {
                        line,
                        col: first_non_blank(self.line(line)),
                    };
                }
                Err(_) => self.err(format!("Not an editor command: {cmd}")),
            },
        }
    }

    // ----- saving and quitting ---------------------------------------

    /// Success only if the result was written at some point.
    fn exit_code(&self) -> i32 {
        if self.written { 0 } else { 1 }
    }

    fn write(&mut self) -> bool {
        let mut text = self.doc.lines.join(self.eol);
        if self.final_newline && !self.doc.lines.is_empty() {
            text.push_str(self.eol);
        }
        match std::fs::write(&self.path, text) {
            Ok(()) => {
                self.written = true;
                self.doc.modified = false;
                let open = match self.doc.unresolved() {
                    (0, 0) => String::new(),
                    _ => format!(" ({})", self.progress()),
                };
                self.msg(format!(
                    "\"{}\" {}L written{open}",
                    self.path.display(),
                    self.doc.lines.len()
                ));
                true
            }
            Err(e) => {
                self.err(format!("Cannot write \"{}\": {e}", self.path.display()));
                false
            }
        }
    }

    fn write_quit(&mut self, force: bool) {
        if !force && self.doc.unresolved() != (0, 0) {
            self.err(format!(
                "{} (:wq! writes anyway, n jumps to the next)",
                self.progress()
            ));
        } else if self.write() {
            self.quit = Some(0);
        }
    }

    /// Called when drawing: adopts the screen size and keeps the cursor in view.
    pub fn set_view(&mut self, height: usize, text_width: usize) {
        if (height, text_width) != (self.view_h, self.text_w) {
            self.view_h = height;
            self.text_w = text_width;
            self.follow_cursor();
            self.follow_hscroll();
        }
    }

    fn follow_hscroll(&mut self) {
        let col = display_col(self.line(self.cur.line), self.cur.col);
        let width = self.text_w.max(1);
        if col < self.hscroll {
            self.hscroll = col;
        } else if col >= self.hscroll + width {
            self.hscroll = col + 1 - width;
        }
    }

    // ----- scrolling -------------------------------------------------

    fn follow_cursor(&mut self) {
        let row = if self.nlines() == 0 {
            0
        } else {
            self.layout().row_of(self.pane, self.cur.line)
        };
        let h = self.view_h.max(1);
        if row < self.top {
            self.top = row;
        } else if row >= self.top + h {
            self.top = row + 1 - h;
        }
    }

    /// `^e` / `^y`: moves the view; the cursor only moves to stay visible.
    fn scroll_view(&mut self, down: bool, n: usize) {
        let l = self.layout();
        let h = self.view_h.max(1);
        self.top = if down {
            (self.top + n).min(l.rows.saturating_sub(1))
        } else {
            self.top.saturating_sub(n)
        };
        let row = l.row_of(self.pane, self.cur.line);
        if row < self.top {
            self.cur.line = l.line_toward(self.pane, self.top, true);
        } else if row >= self.top + h {
            self.cur.line = l.line_toward(self.pane, self.top + h - 1, false);
        }
        self.cur.col = self.want_col;
        self.free_scroll = true;
    }

    /// `^d` / `^u` / `^f` / `^b`: moves cursor and view together.
    fn scroll_page(&mut self, down: bool, amount: usize) {
        let l = self.layout();
        let last_row = l.rows.saturating_sub(1);
        let row = l.row_of(self.pane, self.cur.line).min(last_row);
        if down {
            self.cur.line = l.line_toward(self.pane, (row + amount).min(last_row), true);
            let max_top = l.rows.saturating_sub(self.view_h).max(self.top);
            self.top = (self.top + amount).min(max_top);
        } else {
            self.cur.line = l.line_toward(self.pane, row.saturating_sub(amount), false);
            self.top = self.top.saturating_sub(amount);
        }
        self.cur.col = self.want_col;
    }
}

fn flip(c: char) -> char {
    if c.is_uppercase() {
        c.to_lowercase().next().unwrap_or(c)
    } else {
        c.to_uppercase().next().unwrap_or(c)
    }
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
