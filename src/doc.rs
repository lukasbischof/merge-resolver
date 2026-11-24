//! The merge document: the three inputs, the editable result, the
//! conflicts tracked through edits, and undo history.

use std::ops::Range;

pub use crate::merge::Kind;
use crate::merge::diff3;
use crate::text::tokens;

pub const LEFT: usize = 0;
pub const OUT: usize = 1;
pub const RIGHT: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Pending,
    Accepted,
    Ignored,
    /// This side did not change the region.
    Na,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn name(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, PartialOrd, Ord)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Chunk {
    pub kind: Kind,
    pub base: Range<usize>,
    pub left: Range<usize>,
    pub right: Range<usize>,
    /// Current line range in the result.
    pub out: Range<usize>,
    pub ls: State,
    pub rs: State,
}

impl Chunk {
    pub fn range(&self, pane: usize) -> &Range<usize> {
        match pane {
            LEFT => &self.left,
            OUT => &self.out,
            _ => &self.right,
        }
    }

    pub fn state(&self, side: Side) -> State {
        match side {
            Side::Left => self.ls,
            Side::Right => self.rs,
        }
    }

    fn set_state(&mut self, side: Side, state: State) {
        // An identical change on both sides is one change: keep them in sync.
        if self.kind == Kind::Both {
            self.ls = state;
            self.rs = state;
        } else if side == Side::Left {
            self.ls = state;
        } else {
            self.rs = state;
        }
    }

    pub fn resolved(&self) -> bool {
        match self.kind {
            Kind::Conflict => {
                self.ls == State::Accepted
                    || self.rs == State::Accepted
                    || (self.ls == State::Ignored && self.rs == State::Ignored)
            }
            Kind::Right => self.rs != State::Pending,
            Kind::Left | Kind::Both => self.ls != State::Pending,
        }
    }
}

struct Splice {
    at: usize,
    removed: Vec<String>,
    inserted: Vec<String>,
}

struct Group {
    splices: Vec<Splice>,
    chunks_before: Vec<Chunk>,
    chunks_after: Vec<Chunk>,
    cursor_before: Pos,
    cursor_after: Pos,
}

pub struct Doc {
    pub base: Vec<String>,
    pub left: Vec<String>,
    pub right: Vec<String>,
    pub lines: Vec<String>,
    pub chunks: Vec<Chunk>,
    /// Whether the result differs from what was last written.
    pub modified: bool,
    undo: Vec<Group>,
    redo: Vec<Group>,
    open: Option<Group>,
}

/// Moves a tracked line range across the replacement of lines `s..e`
/// by `n` new lines. Edits inside the range resize it; lines inserted
/// exactly at its start or end stay outside.
fn adjust(r: &mut Range<usize>, s: usize, e: usize, n: usize) {
    let shift = |p: usize| p + n - (e - s);
    let a = if r.start >= e {
        shift(r.start)
    } else if r.start <= s {
        r.start
    } else {
        s + n
    };
    let b = if r.end <= s {
        r.end
    } else if r.end >= e {
        shift(r.end)
    } else {
        s + n
    };
    *r = a..b.max(a);
}

impl Doc {
    pub fn new(base: Vec<String>, left: Vec<String>, right: Vec<String>, auto_apply: bool) -> Doc {
        let chunks = diff3(&base, &left, &right)
            .into_iter()
            .map(|h| Chunk {
                kind: h.kind,
                out: h.base.clone(),
                base: h.base,
                left: h.left,
                right: h.right,
                ls: if h.kind == Kind::Right {
                    State::Na
                } else {
                    State::Pending
                },
                rs: if h.kind == Kind::Left {
                    State::Na
                } else {
                    State::Pending
                },
            })
            .collect();
        let mut doc = Doc {
            lines: base.clone(),
            base,
            left,
            right,
            chunks,
            modified: true,
            undo: Vec::new(),
            redo: Vec::new(),
            open: None,
        };
        if auto_apply {
            doc.apply_non_conflicting(None);
            doc.open = None;
        }
        doc
    }

    pub fn pane_lines(&self, pane: usize) -> &[String] {
        match pane {
            LEFT => &self.left,
            OUT => &self.lines,
            _ => &self.right,
        }
    }

    fn side_lines(&self, side: Side) -> &[String] {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    /// The chunk the given line of a pane belongs to. A chunk that has
    /// no lines in this pane is found on the line following the gap.
    pub fn chunk_at(&self, pane: usize, line: usize) -> Option<usize> {
        let len = self.pane_lines(pane).len();
        self.chunks
            .iter()
            .position(|c| c.range(pane).contains(&line))
            .or_else(|| {
                self.chunks.iter().position(|c| {
                    let r = c.range(pane);
                    r.is_empty() && (r.start == line || (r.start >= len && line + 1 == len))
                })
            })
    }

    /// Number of unresolved (conflicts, non-conflicting changes).
    pub fn unresolved(&self) -> (usize, usize) {
        let open = self.chunks.iter().filter(|c| !c.resolved());
        let conflicts = open.clone().filter(|c| c.kind == Kind::Conflict).count();
        (conflicts, open.count() - conflicts)
    }

    // ----- editing and history -------------------------------------

    /// Opens an undo step (no-op if one is open). `cursor` is restored on undo.
    pub fn begin(&mut self, cursor: Pos) {
        if self.open.is_none() {
            self.open = Some(Group {
                splices: Vec::new(),
                chunks_before: self.chunks.clone(),
                chunks_after: Vec::new(),
                cursor_before: cursor,
                cursor_after: cursor,
            });
        }
    }

    /// Closes the open undo step, recording it if anything changed.
    pub fn commit(&mut self, cursor: Pos) {
        let Some(mut g) = self.open.take() else {
            return;
        };
        if g.splices.is_empty() && g.chunks_before == self.chunks {
            return;
        }
        g.chunks_after = self.chunks.clone();
        g.cursor_after = cursor;
        self.undo.push(g);
        self.redo.clear();
        self.modified = true;
    }

    /// Replaces lines `start..end` of the result. Every edit goes
    /// through here so that conflicts keep tracking their lines.
    pub fn splice(&mut self, start: usize, end: usize, new: Vec<String>) {
        self.begin(Pos::default());
        let removed: Vec<String> = self.lines.splice(start..end, new.iter().cloned()).collect();
        for c in &mut self.chunks {
            adjust(&mut c.out, start, end, new.len());
        }
        let g = self.open.as_mut().unwrap();
        // Repeated edits of one line (typing) collapse into one record.
        if let Some(last) = g.splices.last_mut()
            && last.at == start
            && last.inserted.len() == 1
            && removed.len() == 1
            && new.len() == 1
        {
            last.inserted = new;
            return;
        }
        g.splices.push(Splice {
            at: start,
            removed,
            inserted: new,
        });
    }

    pub fn undo(&mut self) -> Option<Pos> {
        self.commit(Pos::default());
        let g = self.undo.pop()?;
        for s in g.splices.iter().rev() {
            self.lines
                .splice(s.at..s.at + s.inserted.len(), s.removed.iter().cloned());
        }
        self.chunks = g.chunks_before.clone();
        let cursor = g.cursor_before;
        self.redo.push(g);
        self.modified = true;
        Some(cursor)
    }

    pub fn redo(&mut self) -> Option<Pos> {
        self.commit(Pos::default());
        let g = self.redo.pop()?;
        for s in &g.splices {
            self.lines
                .splice(s.at..s.at + s.removed.len(), s.inserted.iter().cloned());
        }
        self.chunks = g.chunks_after.clone();
        let cursor = g.cursor_after;
        self.undo.push(g);
        self.modified = true;
        Some(cursor)
    }

    // ----- resolving -----------------------------------------------

    fn replace_out(&mut self, idx: usize, lines: Vec<String>) {
        let out = self.chunks[idx].out.clone();
        let n = lines.len();
        self.splice(out.start, out.end, lines);
        self.chunks[idx].out = out.start..out.start + n;
    }

    /// Takes one side's version into the result. If the other side of a
    /// conflict was accepted before, this side is appended after it.
    pub fn accept(&mut self, idx: usize, side: Side) -> Result<(), String> {
        let c = self.chunks[idx].clone();
        match c.state(side) {
            State::Na => return Err(format!("No change on the {} side here", side.name())),
            State::Accepted if c.kind == Kind::Both => {
                return Err("Identical change is already applied".into());
            }
            State::Accepted => {
                return Err(format!("{} is already accepted (u to undo)", side.name()));
            }
            _ => {}
        }
        let range = if side == Side::Left {
            c.left.clone()
        } else {
            c.right.clone()
        };
        let lines = self.side_lines(side)[range].to_vec();
        let other = if side == Side::Left { c.rs } else { c.ls };
        if c.kind == Kind::Conflict && other == State::Accepted {
            let n = lines.len();
            self.splice(c.out.end, c.out.end, lines);
            self.chunks[idx].out = c.out.start..c.out.end + n;
        } else {
            self.replace_out(idx, lines);
        }
        self.chunks[idx].set_state(side, State::Accepted);
        Ok(())
    }

    /// Dismisses one side's version without touching the result.
    pub fn ignore(&mut self, idx: usize, side: Side) -> Result<(), String> {
        match self.chunks[idx].state(side) {
            State::Na => Err(format!("No change on the {} side here", side.name())),
            State::Accepted => Err(format!("{} is already accepted (u to undo)", side.name())),
            State::Ignored => Err(format!("{} is already ignored", side.name())),
            State::Pending => {
                self.begin(Pos::default());
                self.chunks[idx].set_state(side, State::Ignored);
                Ok(())
            }
        }
    }

    /// Dismisses every side that is still pending.
    pub fn ignore_pending(&mut self, idx: usize) -> Result<(), String> {
        let pending: Vec<Side> = [Side::Left, Side::Right]
            .into_iter()
            .filter(|&s| self.chunks[idx].state(s) == State::Pending)
            .collect();
        if pending.is_empty() {
            return Err("Nothing left to ignore here".into());
        }
        self.begin(Pos::default());
        for side in pending {
            self.chunks[idx].set_state(side, State::Ignored);
        }
        Ok(())
    }

    /// Puts the base version back and reopens the chunk.
    pub fn reset(&mut self, idx: usize) {
        let lines = self.base[self.chunks[idx].base.clone()].to_vec();
        self.replace_out(idx, lines);
        let c = &mut self.chunks[idx];
        if c.ls != State::Na {
            c.ls = State::Pending;
        }
        if c.rs != State::Na {
            c.rs = State::Pending;
        }
    }

    /// Accepts all pending non-conflicting changes, optionally from one
    /// side only. Returns how many were applied.
    pub fn apply_non_conflicting(&mut self, side: Option<Side>) -> usize {
        let mut applied = 0;
        for i in 0..self.chunks.len() {
            let c = &self.chunks[i];
            let from = match c.kind {
                Kind::Conflict => continue,
                Kind::Right => Side::Right,
                Kind::Left => Side::Left,
                Kind::Both => side.unwrap_or(Side::Left),
            };
            if c.resolved() || side.is_some_and(|s| s != from) {
                continue;
            }
            if self.accept(i, from).is_ok() {
                applied += 1;
            }
        }
        applied
    }

    /// Resolves a conflict whose sides changed different words.
    pub fn resolve_simple(&mut self, idx: usize) -> Result<(), String> {
        let c = self.chunks[idx].clone();
        if c.kind != Kind::Conflict || c.resolved() {
            return Err("No unresolved conflict here".into());
        }
        if c.base.is_empty() || c.left.is_empty() || c.right.is_empty() {
            return Err("Not a simple conflict".into());
        }
        let b = self.base[c.base].join("\n");
        let l = self.left[c.left].join("\n");
        let r = self.right[c.right].join("\n");
        let split = |s: &'_ str| {
            tokens(s)
                .into_iter()
                .map(|t| s[t].to_string())
                .collect::<Vec<_>>()
        };
        let (bt, lt, rt) = (split(&b), split(&l), split(&r));
        let hunks = diff3(&bt, &lt, &rt);
        if hunks.iter().any(|h| h.kind == Kind::Conflict) {
            return Err("Not a simple conflict".into());
        }
        let mut merged = String::new();
        let mut pos = 0;
        for h in hunks {
            merged.extend(bt[pos..h.base.start].iter().map(String::as_str));
            match h.kind {
                Kind::Right => merged.extend(rt[h.right].iter().map(String::as_str)),
                _ => merged.extend(lt[h.left].iter().map(String::as_str)),
            }
            pos = h.base.end;
        }
        merged.extend(bt[pos..].iter().map(String::as_str));
        self.replace_out(idx, merged.split('\n').map(String::from).collect());
        self.chunks[idx].ls = State::Accepted;
        self.chunks[idx].rs = State::Accepted;
        Ok(())
    }

    pub fn resolve_all_simple(&mut self) -> usize {
        (0..self.chunks.len())
            .filter(|&i| self.resolve_simple(i).is_ok())
            .count()
    }

    /// Makes the result identical to one side's file.
    pub fn accept_file(&mut self, side: Side) {
        let lines = self.side_lines(side).to_vec();
        self.splice(0, self.lines.len(), lines);
        for c in &mut self.chunks {
            c.out = if side == Side::Left {
                c.left.clone()
            } else {
                c.right.clone()
            };
            let (mine, theirs) = if side == Side::Left {
                (&mut c.ls, &mut c.rs)
            } else {
                (&mut c.rs, &mut c.ls)
            };
            if *mine != State::Na {
                *mine = State::Accepted;
            }
            if *theirs != State::Na {
                *theirs = if c.kind == Kind::Both {
                    State::Accepted
                } else {
                    State::Ignored
                };
            }
        }
    }
}

#[cfg(test)]
#[path = "doc_tests.rs"]
mod tests;
