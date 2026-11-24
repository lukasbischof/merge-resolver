//! Keys and the normal-mode command grammar:
//! `[count] command`, `[count] operator [count] (motion | text object)`.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Esc,
    Enter,
    Backspace,
    Tab,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Motion {
    Left,
    Right,
    Down,
    Up,
    Bol,
    FirstNonBlank,
    Eol,
    /// `w` / `W`
    Word(bool),
    /// `b` / `B`
    Back(bool),
    /// `e` / `E`
    EndWord(bool),
    FileStart,
    FileEnd,
    Find {
        c: char,
        fwd: bool,
        till: bool,
    },
    /// `;` / `,` (reversed)
    RepeatFind(bool),
    ParaFwd,
    ParaBack,
    Pair,
    ScreenTop,
    ScreenMid,
    NextLine,
    PrevLine,
    CurLine,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Delete,
    Change,
    Yank,
    Indent,
    Dedent,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Obj {
    Word {
        big: bool,
        around: bool,
    },
    Pair {
        open: char,
        close: char,
        around: bool,
    },
    Quote {
        q: char,
        around: bool,
    },
    Para {
        around: bool,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Motion(Motion),
    /// The doubled operator (`dd`); also "the selection" in visual mode.
    Lines,
    Obj(Obj),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    InsertBefore,
    Append,
    InsertBol,
    AppendEol,
    OpenBelow,
    OpenAbove,
    DelChar,
    Subst,
    SubstLine,
    ChangeEol,
    DeleteEol,
    YankLine,
    Join,
    PasteAfter,
    PasteBefore,
    Undo,
    Redo,
    Repeat,
    ToggleCase,
    Replace(char),
    Visual,
    VisualLine,
    CmdLine,
    /// `/` (true) or `?`
    Search(bool),
    /// `gn` (same direction) / `gN`
    SearchNext(bool),
    /// `*` (forward) / `#`
    SearchWord(bool),
    /// `n` (forward) / `N`: unresolved conflicts
    NextConflict(bool),
    /// `]c` (forward) / `[c`: every change
    NextChunk(bool),
    AcceptLeft,
    AcceptRight,
    IgnoreLeft,
    IgnoreRight,
    Leader,
    WriteQuit,
    QuitNoSave,
    HalfDown,
    HalfUp,
    PageDown,
    PageUp,
    LineDown,
    LineUp,
    Center,
    ScrollTop,
    ScrollBottom,
    PaneLeft,
    PaneRight,
    PaneNext,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    Move(Motion),
    Operate(Op, Target),
    /// Visual mode only: select a text object.
    Select(Obj),
    Act(Act),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Parse {
    Incomplete,
    Invalid,
    Done(Option<usize>, Cmd),
}

struct Reader<'a> {
    keys: &'a [Key],
    pos: usize,
}

impl Reader<'_> {
    /// The next key, or `None` if more input is needed.
    fn next(&mut self) -> Option<Key> {
        let k = self.keys.get(self.pos).copied();
        self.pos += 1;
        k
    }

    fn count(&mut self) -> Option<usize> {
        let mut n: Option<usize> = None;
        while let Some(Key::Char(c)) = self.keys.get(self.pos) {
            let Some(d) = c.to_digit(10) else { break };
            if d == 0 && n.is_none() {
                break;
            }
            n = Some((n.unwrap_or(0) * 10 + d as usize).min(99_999));
            self.pos += 1;
        }
        n
    }
}

/// Outer `None`: more keys needed. Inner `None`: not a motion.
fn motion(k: Key, rd: &mut Reader) -> Option<Option<Motion>> {
    use Motion::*;
    Some(Some(match k {
        Key::Char('h') | Key::Left | Key::Backspace => Left,
        Key::Char('l') | Key::Right => Right,
        Key::Char('j') | Key::Down | Key::Ctrl('j') | Key::Ctrl('n') => Down,
        Key::Char('k') | Key::Up | Key::Ctrl('p') => Up,
        Key::Char('0') | Key::Home => Bol,
        Key::Char('^') => FirstNonBlank,
        Key::Char('$') | Key::End => Eol,
        Key::Char('w') => Word(false),
        Key::Char('W') => Word(true),
        Key::Char('b') => Back(false),
        Key::Char('B') => Back(true),
        Key::Char('e') => EndWord(false),
        Key::Char('E') => EndWord(true),
        Key::Char('G') => FileEnd,
        Key::Char('g') => match rd.next()? {
            Key::Char('g') => FileStart,
            _ => return Some(None),
        },
        Key::Char(f @ ('f' | 'F' | 't' | 'T')) => match rd.next()? {
            Key::Char(c) => Find {
                c,
                fwd: f.is_lowercase(),
                till: f.eq_ignore_ascii_case(&'t'),
            },
            _ => return Some(None),
        },
        Key::Char(';') => RepeatFind(false),
        Key::Char(',') => RepeatFind(true),
        Key::Char('}') => ParaFwd,
        Key::Char('{') => ParaBack,
        Key::Char('%') => Pair,
        Key::Char('H') => ScreenTop,
        Key::Char('M') => ScreenMid,
        Key::Char('+') | Key::Enter => NextLine,
        Key::Char('-') => PrevLine,
        Key::Char('_') => CurLine,
        _ => return Some(None),
    }))
}

/// Outer `None`: more keys needed. Inner `None`: not a text object.
fn object(k: Key, rd: &mut Reader) -> Option<Option<Obj>> {
    let around = match k {
        Key::Char('i') => false,
        Key::Char('a') => true,
        _ => return Some(None),
    };
    let Key::Char(c) = rd.next()? else {
        return Some(None);
    };
    let pair = |open, close| Obj::Pair {
        open,
        close,
        around,
    };
    Some(Some(match c {
        'w' => Obj::Word { big: false, around },
        'W' => Obj::Word { big: true, around },
        '(' | ')' | 'b' => pair('(', ')'),
        '[' | ']' => pair('[', ']'),
        '{' | '}' | 'B' => pair('{', '}'),
        '<' | '>' => pair('<', '>'),
        '"' | '\'' | '`' => Obj::Quote { q: c, around },
        'p' => Obj::Para { around },
        _ => return Some(None),
    }))
}

fn mul(a: Option<usize>, b: Option<usize>) -> Option<usize> {
    match (a, b) {
        (None, None) => None,
        _ => Some(a.unwrap_or(1) * b.unwrap_or(1)),
    }
}

fn parse_inner(rd: &mut Reader, visual: bool) -> Option<Parse> {
    use Act::*;
    let count = rd.count();
    let k = rd.next()?;
    let done = |count, cmd| Some(Parse::Done(count, cmd));

    let op = match k {
        Key::Char('d') => Some(Op::Delete),
        Key::Char('c') => Some(Op::Change),
        Key::Char('y') => Some(Op::Yank),
        Key::Char('>') => Some(Op::Indent),
        Key::Char('<') => Some(Op::Dedent),
        _ => None,
    };
    if let Some(op) = op {
        if visual {
            return done(count, Cmd::Operate(op, Target::Lines));
        }
        let count = mul(count, rd.count());
        let k2 = rd.next()?;
        if k2 == k {
            return done(count, Cmd::Operate(op, Target::Lines));
        }
        if let Some(o) = object(k2, rd)? {
            return done(count, Cmd::Operate(op, Target::Obj(o)));
        }
        return match motion(k2, rd)? {
            Some(m) => done(count, Cmd::Operate(op, Target::Motion(m))),
            None => Some(Parse::Invalid),
        };
    }
    if visual && matches!(k, Key::Char('i' | 'a')) {
        return match object(k, rd)? {
            Some(o) => done(count, Cmd::Select(o)),
            None => Some(Parse::Invalid),
        };
    }

    let act = match k {
        Key::Char('g') => match rd.next()? {
            Key::Char('g') => return done(count, Cmd::Move(Motion::FileStart)),
            Key::Char('n') => SearchNext(true),
            Key::Char('N') => SearchNext(false),
            _ => return Some(Parse::Invalid),
        },
        Key::Char('z') => match rd.next()? {
            Key::Char('z') => Center,
            Key::Char('t') | Key::Enter => ScrollTop,
            Key::Char('b') => ScrollBottom,
            _ => return Some(Parse::Invalid),
        },
        Key::Char('Z') => match rd.next()? {
            Key::Char('Z') => WriteQuit,
            Key::Char('Q') => QuitNoSave,
            _ => return Some(Parse::Invalid),
        },
        Key::Char('X') => match rd.next()? {
            Key::Char('L') => IgnoreLeft,
            Key::Char('R') => IgnoreRight,
            _ => return Some(Parse::Invalid),
        },
        Key::Ctrl('w') => match rd.next()? {
            Key::Char('h') | Key::Ctrl('h') | Key::Left => PaneLeft,
            Key::Char('l') | Key::Ctrl('l') | Key::Right => PaneRight,
            Key::Char('w') | Key::Ctrl('w') => PaneNext,
            _ => return Some(Parse::Invalid),
        },
        Key::Char(']') => match rd.next()? {
            Key::Char('c') => NextChunk(true),
            _ => return Some(Parse::Invalid),
        },
        Key::Char('[') => match rd.next()? {
            Key::Char('c') => NextChunk(false),
            _ => return Some(Parse::Invalid),
        },
        Key::Char('r') => match rd.next()? {
            Key::Char(c) => Replace(c),
            _ => return Some(Parse::Invalid),
        },
        Key::Char('i') => InsertBefore,
        Key::Char('a') => Append,
        Key::Char('I') => InsertBol,
        Key::Char('A') => AppendEol,
        Key::Char('o') => OpenBelow,
        Key::Char('O') => OpenAbove,
        Key::Char('x') | Key::Delete => DelChar,
        Key::Char('s') => Subst,
        Key::Char('S') => SubstLine,
        Key::Char('C') => ChangeEol,
        Key::Char('D') => DeleteEol,
        Key::Char('Y') => YankLine,
        Key::Char('J') => Join,
        Key::Char('p') => PasteAfter,
        Key::Char('P') => PasteBefore,
        Key::Char('u') => Undo,
        Key::Ctrl('r') => Redo,
        Key::Char('.') => Repeat,
        Key::Char('~') => ToggleCase,
        Key::Char('v') => Visual,
        Key::Char('V') => VisualLine,
        Key::Char(':') => CmdLine,
        Key::Char('/') => Search(true),
        Key::Char('?') => Search(false),
        Key::Char('*') => SearchWord(true),
        Key::Char('#') => SearchWord(false),
        Key::Char('n') => NextConflict(true),
        Key::Char('N') => NextConflict(false),
        Key::Char('L') => AcceptLeft,
        Key::Char('R') => AcceptRight,
        Key::Char(' ') => Leader,
        Key::Ctrl('d') => HalfDown,
        Key::Ctrl('u') => HalfUp,
        Key::Ctrl('f') | Key::PageDown => PageDown,
        Key::Ctrl('b') | Key::PageUp => PageUp,
        Key::Ctrl('e') => LineDown,
        Key::Ctrl('y') => LineUp,
        Key::Ctrl('h') => PaneLeft,
        Key::Ctrl('l') => PaneRight,
        _ => {
            return match motion(k, rd)? {
                Some(m) => done(count, Cmd::Move(m)),
                None => Some(Parse::Invalid),
            };
        }
    };
    done(count, Cmd::Act(act))
}

/// Parses the keys typed so far in normal or visual mode.
pub fn parse(keys: &[Key], visual: bool) -> Parse {
    parse_inner(&mut Reader { keys, pos: 0 }, visual).unwrap_or(Parse::Incomplete)
}

/// Translates a terminal key event. Alt+key arrives as Escape followed
/// by the key, which is what terminals send for a quick `<esc>key`.
pub fn from_event(k: KeyEvent) -> Vec<Key> {
    if k.kind == KeyEventKind::Release {
        return Vec::new();
    }
    let key = match k.code {
        KeyCode::Char(c) if k.modifiers.contains(KeyModifiers::CONTROL) => {
            Key::Ctrl(c.to_ascii_lowercase())
        }
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Esc => Key::Esc,
        KeyCode::Enter => Key::Enter,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Tab => Key::Tab,
        KeyCode::Delete => Key::Delete,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        _ => return Vec::new(),
    };
    if k.modifiers.contains(KeyModifiers::ALT) {
        vec![Key::Esc, key]
    } else {
        vec![key]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode, modifiers: KeyModifiers) -> Vec<Key> {
        from_event(KeyEvent::new(code, modifiers))
    }

    #[test]
    fn plain_and_shifted_chars_are_passed_through() {
        assert_eq!(
            press(KeyCode::Char('x'), KeyModifiers::NONE),
            [Key::Char('x')]
        );
        assert_eq!(
            press(KeyCode::Char('L'), KeyModifiers::SHIFT),
            [Key::Char('L')]
        );
    }

    #[test]
    fn control_chars_are_lowercased() {
        assert_eq!(
            press(KeyCode::Char('H'), KeyModifiers::CONTROL),
            [Key::Ctrl('h')]
        );
        assert_eq!(
            press(KeyCode::Char('d'), KeyModifiers::CONTROL),
            [Key::Ctrl('d')]
        );
    }

    #[test]
    fn alt_is_escape_followed_by_the_key() {
        assert_eq!(
            press(KeyCode::Char('u'), KeyModifiers::ALT),
            [Key::Esc, Key::Char('u')]
        );
    }

    #[test]
    fn named_keys_are_mapped() {
        assert_eq!(press(KeyCode::Esc, KeyModifiers::NONE), [Key::Esc]);
        assert_eq!(press(KeyCode::Enter, KeyModifiers::NONE), [Key::Enter]);
        assert_eq!(
            press(KeyCode::Backspace, KeyModifiers::NONE),
            [Key::Backspace]
        );
        assert_eq!(press(KeyCode::F(5), KeyModifiers::NONE), []);
    }

    #[test]
    fn key_releases_are_ignored() {
        let mut k = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        k.kind = KeyEventKind::Release;
        assert_eq!(from_event(k), []);
    }
}
