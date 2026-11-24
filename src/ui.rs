//! Drawing: three aligned panes, status line, command line, popups.

use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::app::{App, LEADER_MENU, Mode};
use crate::doc::{Chunk, Kind, LEFT, RIGHT, Side, State};
use crate::layout::RowRef;
use crate::text::{char_len, char_width, display_col};

static TRUECOLOR: AtomicBool = AtomicBool::new(true);

/// Falls back to the 256-color palette on terminals without 24-bit color.
pub fn set_truecolor(on: bool) {
    TRUECOLOR.store(on, Ordering::Relaxed);
}

/// A color as 24-bit RGB plus its nearest 256-palette index.
#[derive(Clone, Copy)]
struct C(u8, u8, u8, u8);

impl From<C> for Color {
    fn from(c: C) -> Color {
        if TRUECOLOR.load(Ordering::Relaxed) {
            Color::Rgb(c.0, c.1, c.2)
        } else {
            Color::Indexed(c.3)
        }
    }
}

/// Colors of one kind of change: idle and under-the-cursor variants.
struct Palette {
    bg: [C; 2],
    emph: [C; 2],
    sign: C,
}

const CONFLICT: Palette = Palette {
    bg: [C(64, 28, 32, 52), C(98, 36, 42, 88)],
    emph: [C(104, 42, 48, 88), C(150, 58, 66, 124)],
    sign: C(240, 100, 100, 203),
};
const MODIFIED: Palette = Palette {
    bg: [C(28, 44, 72, 17), C(40, 62, 104, 18)],
    emph: [C(46, 72, 120, 24), C(64, 100, 165, 25)],
    sign: C(100, 160, 255, 75),
};
const INSERTED: Palette = Palette {
    bg: [C(28, 56, 34, 22), C(38, 82, 48, 28)],
    emph: [C(44, 94, 56, 28), C(60, 125, 74, 34)],
    sign: C(110, 200, 120, 114),
};
const DELETED: Palette = Palette {
    bg: [C(50, 50, 56, 237), C(74, 74, 82, 240)],
    emph: [C(50, 50, 56, 237), C(74, 74, 82, 240)],
    sign: C(150, 150, 160, 247),
};
/// A handled change, shown only while the cursor is on it.
const HANDLED: Palette = Palette {
    bg: [C(44, 44, 50, 236), C(44, 44, 50, 236)],
    emph: [C(44, 44, 50, 236), C(44, 44, 50, 236)],
    sign: C(120, 120, 132, 244),
};

const DIM: C = C(105, 105, 118, 243);
const FAINT: C = C(72, 72, 82, 239);
const BRIGHT: C = C(228, 228, 232, 254);
const SELECTION: C = C(58, 74, 112, 60);
const BAR_BG: C = C(44, 46, 58, 236);
const BAR_FG: C = C(200, 200, 210, 252);
const ERROR: C = C(255, 110, 110, 203);

const HELP: &[&str] = &[
    "Conflicts (any pane, on the highlighted conflict)",
    "  n  N        next / previous unresolved conflict",
    "  ]c  [c      next / previous change, resolved or not",
    "  L  R        accept left / right; LR or RL takes both",
    "  XL  XR      ignore left / right",
    "  XX          ignore both and keep the result as it is",
    "  u  ^r       undo / redo",
    "",
    "Panes",
    "  ^h  ^l      pane to the left / right (also ^w h, ^w l, ^w w)",
    "",
    "Editing the result (middle pane) works like vim",
    "  i a I A o O   d c y > < with motions and text objects",
    "  x s r ~ J p P .   v V   dd cc yy   counts",
    "",
    "Moving",
    "  h j k l  w b e  0 ^ $  gg G  f t ; ,  % { }  H M",
    "  ^d ^u ^f ^b ^e ^y  zz zt zb   /text ?text gn gN * #",
    "",
    "Saving",
    "  :w  write    :wq  ZZ  write and quit    :q  ZQ  quit",
    "  :wq!  write and quit with open conflicts    :cq  abort",
    "",
    "<space> opens the menu with the remaining commands",
];

/// Picks the colors for a chunk in one pane, or none if it is not drawn.
fn palette(c: &Chunk, pane: usize, active: bool) -> Option<&'static Palette> {
    let untouched = matches!((c.kind, pane), (Kind::Left, RIGHT) | (Kind::Right, LEFT));
    if c.resolved() || untouched {
        return active.then_some(&HANDLED);
    }
    let side = if c.kind == Kind::Right {
        &c.right
    } else {
        &c.left
    };
    Some(match c.kind {
        Kind::Conflict => &CONFLICT,
        _ if c.base.is_empty() => &INSERTED,
        _ if side.is_empty() => &DELETED,
        _ => &MODIFIED,
    })
}

/// The gutter mark on a chunk's first row.
fn sign(c: &Chunk, pane: usize) -> char {
    let state = match pane {
        LEFT => c.state(Side::Left),
        RIGHT => c.state(Side::Right),
        _ => return if c.resolved() { ' ' } else { '●' },
    };
    match state {
        State::Pending if pane == LEFT => '»',
        State::Pending => '«',
        State::Accepted => '✓',
        State::Ignored => '✕',
        State::Na => ' ',
    }
}

struct TextStyle<'a> {
    base: Style,
    emph: Option<(&'a [Range<usize>], Color)>,
    /// Selected char columns.
    selected: Option<Range<usize>>,
}

/// Draws one line of text into `area`, scrolled left by `hscroll` cells.
fn draw_text(buf: &mut Buffer, area: Rect, line: &str, hscroll: usize, style: &TextStyle) {
    let mut col = 0;
    for (i, c) in line.chars().enumerate() {
        let width = char_width(c, col);
        let start = col;
        col += width;
        if start < hscroll || width == 0 {
            continue;
        }
        let x = start - hscroll;
        if x + width > area.width as usize {
            break;
        }
        let mut s = style.base;
        if let Some((ranges, color)) = style.emph
            && ranges.iter().any(|r| r.contains(&i))
        {
            s = s.bg(color);
        }
        if style.selected.as_ref().is_some_and(|r| r.contains(&i)) {
            s = s.bg(SELECTION.into());
        }
        let x = area.x + x as u16;
        if c == '\t' {
            buf.set_style(Rect::new(x, area.y, width as u16, 1), s);
        } else {
            buf.set_stringn(x, area.y, c.encode_utf8(&mut [0; 4]), width, s);
        }
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let longest = app
        .doc
        .left
        .len()
        .max(app.doc.lines.len())
        .max(app.doc.right.len());
    let num_w = (longest.max(1).ilog10() as u16 + 1).max(3);
    let gutter = num_w + 3;
    let pane_w = area.width.saturating_sub(2) / 3;
    if area.height < 4 || pane_w <= gutter {
        if area.height > 0 {
            f.buffer_mut().set_stringn(
                0,
                0,
                "Terminal too small",
                area.width as usize,
                Style::new(),
            );
        }
        return;
    }
    let xs = [0, pane_w + 1, 2 * pane_w + 2];
    let widths = [pane_w, pane_w, area.width - xs[2]];
    let view_h = area.height - 3;
    app.set_view(view_h as usize, (pane_w - gutter) as usize);

    let layout = app.layout();
    let active = app.active_chunk();
    let selection = app.selection();
    let buf = f.buffer_mut();

    // Header and pane separators.
    for p in 0..3 {
        let style = if p == app.pane {
            Style::new()
                .fg(BRIGHT.into())
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::new().fg(DIM.into())
        };
        buf.set_stringn(
            xs[p] + 1,
            0,
            &app.titles[p],
            widths[p].saturating_sub(2) as usize,
            style,
        );
    }
    for y in 0..=view_h {
        for x in [xs[1] - 1, xs[2] - 1] {
            buf.set_string(x, y, "│", Style::new().fg(FAINT.into()));
        }
    }

    for vy in 0..view_h {
        let row = app.top + vy as usize;
        let y = 1 + vy;
        let Some(seg) = layout.seg_at(row) else {
            continue;
        };
        let chunk = seg.chunk.map(|i| &app.doc.chunks[i]);
        let is_active = seg.chunk.is_some() && seg.chunk == active;
        for p in 0..3 {
            let (x, w) = (xs[p], widths[p]);
            let pal = chunk.and_then(|c| palette(c, p, is_active));
            if let Some(pal) = pal {
                buf.set_style(
                    Rect::new(x, y, w, 1),
                    Style::new().bg(pal.bg[is_active as usize].into()),
                );
            }
            if let Some(c) = chunk {
                // The first row carries the side's state; a bar marks the rest.
                let bar = if pal.is_some() { '▎' } else { ' ' };
                let mark = if row == seg.row {
                    Some(sign(c, p)).filter(|s| *s != ' ').unwrap_or(bar)
                } else {
                    bar
                };
                let color = pal.map_or(HANDLED.sign, |pal| pal.sign);
                buf.set_string(
                    x,
                    y,
                    mark.to_string(),
                    Style::new().fg(color.into()).add_modifier(Modifier::BOLD),
                );
            }
            let text_area = Rect::new(x + gutter, y, w - gutter, 1);
            match layout.row_ref(p, row) {
                RowRef::Line(n) => {
                    let on_cursor = p == app.pane && n == app.cur.line;
                    let number = Style::new().fg(if on_cursor { BRIGHT } else { DIM }.into());
                    buf.set_string(x + 2, y, format!("{:>1$}", n + 1, num_w as usize), number);
                    let line = &app.doc.pane_lines(p)[n];
                    let emph = match (pal, p, app.word_hl) {
                        (Some(pal), LEFT | RIGHT, true) => Some((
                            app.emph[p / 2][n].as_slice(),
                            pal.emph[is_active as usize].into(),
                        )),
                        _ => None,
                    };
                    let selected = selection
                        .filter(|(s, e)| p == app.pane && (s.line..=e.line).contains(&n))
                        .map(|(s, e)| {
                            let from = if n == s.line { s.col } else { 0 };
                            let to = if n == e.line {
                                e.col.saturating_add(1)
                            } else {
                                usize::MAX
                            };
                            // An empty line still shows that it is selected.
                            from..to.max(from + 1).min(char_len(line).max(from + 1))
                        });
                    if let Some(sel) = &selected
                        && line.is_empty()
                        && sel.start == 0
                    {
                        buf.set_style(
                            Rect {
                                width: 1,
                                ..text_area
                            },
                            Style::new().bg(SELECTION.into()),
                        );
                    }
                    draw_text(
                        buf,
                        text_area,
                        line,
                        app.hscroll,
                        &TextStyle {
                            base: Style::new(),
                            emph,
                            selected,
                        },
                    );
                }
                RowRef::Filler => {
                    let fill = "╱".repeat(text_area.width as usize);
                    buf.set_string(text_area.x, y, fill, Style::new().fg(FAINT.into()));
                }
                RowRef::None => {}
            }
        }
    }

    draw_status(
        buf,
        app,
        Rect::new(0, area.height - 2, area.width, 1),
        active,
    );
    let bottom = area.height - 1;
    let mut cursor = None;
    match app.mode {
        Mode::Cmd | Mode::Search(_) => {
            let prefix = match app.mode {
                Mode::Cmd => ':',
                Mode::Search(true) => '/',
                _ => '?',
            };
            buf.set_stringn(
                0,
                bottom,
                format!("{prefix}{}", app.input),
                area.width as usize,
                Style::new(),
            );
            cursor = Some((
                1 + display_col(&app.input, char_len(&app.input)) as u16,
                bottom,
            ));
        }
        _ => {
            if let Some((text, error)) = &app.message {
                let style = if *error {
                    Style::new().fg(ERROR.into())
                } else {
                    Style::new()
                };
                buf.set_stringn(
                    1,
                    bottom,
                    text,
                    area.width.saturating_sub(1) as usize,
                    style,
                );
            }
            // The cursor is only shown where its line is on screen.
            let row = if app.doc.pane_lines(app.pane).is_empty() {
                0
            } else {
                layout.row_of(app.pane, app.cur.line)
            };
            let line = app
                .doc
                .pane_lines(app.pane)
                .get(app.cur.line)
                .map_or("", String::as_str);
            let col = display_col(line, app.cur.col);
            let text_w = (widths[app.pane] - gutter) as usize;
            if (app.top..app.top + view_h as usize).contains(&row)
                && (app.hscroll..app.hscroll + text_w).contains(&col)
            {
                cursor = Some((
                    xs[app.pane] + gutter + (col - app.hscroll) as u16,
                    1 + (row - app.top) as u16,
                ));
            }
        }
    }

    match app.mode {
        Mode::Leader => {
            let lines: Vec<String> = LEADER_MENU
                .iter()
                .map(|(k, text)| format!(" {k}  {text} "))
                .collect();
            popup(f, " <space> ", &lines, false);
        }
        Mode::Help => {
            let lines: Vec<String> = HELP.iter().map(|l| format!(" {l} ")).collect();
            popup(f, " Keys (any key closes) ", &lines, true);
        }
        _ => {
            if let Some(pos) = cursor {
                f.set_cursor_position(pos);
            }
        }
    }
}

fn draw_status(buf: &mut Buffer, app: &App, area: Rect, active: Option<usize>) {
    buf.set_style(area, Style::new().bg(BAR_BG.into()).fg(BAR_FG.into()));
    let (name, color) = match app.mode {
        Mode::Insert => ("INSERT", C(120, 200, 120, 114)),
        Mode::Visual => ("VISUAL", C(200, 140, 240, 177)),
        Mode::VisualLine => ("V-LINE", C(200, 140, 240, 177)),
        Mode::Cmd | Mode::Search(_) => ("COMMAND", C(230, 180, 90, 179)),
        _ => ("NORMAL", C(100, 150, 240, 69)),
    };
    let badge = Style::new()
        .bg(color.into())
        .fg(Color::Black)
        .add_modifier(Modifier::BOLD);
    let (x, _) = buf.set_stringn(
        area.x,
        area.y,
        format!(" {name} "),
        area.width as usize,
        badge,
    );
    let left = format!(
        " {}{}",
        app.progress(),
        if app.doc.modified { "  [+]" } else { "" }
    );
    let (x, _) = buf.set_stringn(x, area.y, left, (area.right() - x) as usize, Style::new());

    let keys: String = app.keys.iter().map(key_name).collect();
    let on = active.map_or(String::new(), |i| {
        let c = &app.doc.chunks[i];
        let what = match c.kind {
            Kind::Conflict => "conflict",
            Kind::Left => "change from the left",
            Kind::Right => "change from the right",
            Kind::Both => "same change on both sides",
        };
        format!("{what} {}/{}", i + 1, app.doc.chunks.len())
    });
    let right = format!("{keys}  {on}  {}:{} ", app.cur.line + 1, app.cur.col + 1);
    let width = char_len(&right) as u16;
    if x + width < area.right() {
        buf.set_string(area.right() - width, area.y, right, Style::new());
    }
}

fn key_name(key: &crate::app::Key) -> String {
    use crate::app::Key;
    match key {
        Key::Char(' ') => "<space>".into(),
        Key::Char(c) => c.to_string(),
        Key::Ctrl(c) => format!("^{c}"),
        _ => String::new(),
    }
}

/// A bordered box: centered, or sitting above the status line on the right.
fn popup(f: &mut Frame, title: &str, lines: &[String], centered: bool) {
    let area = f.area();
    let inner = lines
        .iter()
        .map(|l| char_len(l))
        .max()
        .unwrap_or(0)
        .max(char_len(title)) as u16;
    let w = (inner + 2).min(area.width);
    let h = (lines.len() as u16 + 2).min(area.height);
    let (x, y) = if centered {
        ((area.width - w) / 2, (area.height - h) / 2)
    } else {
        (
            area.width - w,
            area.height.saturating_sub(2).saturating_sub(h),
        )
    };
    let rect = Rect::new(x, y, w, h);
    let text: Vec<Line> = lines.iter().map(|l| Line::raw(l.as_str())).collect();
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(text).block(Block::bordered().title(title)),
        rect,
    );
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;
