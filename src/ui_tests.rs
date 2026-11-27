use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::style::Color;

use super::*;
use crate::app::Key;
use crate::doc::Doc;
use crate::syntax::Token;

fn lines(s: &[&str]) -> Vec<String> {
    s.iter().map(|l| l.to_string()).collect()
}

fn merge(base: &[&str], left: &[&str], right: &[&str]) -> App {
    App::new(
        Doc::new(lines(base), lines(left), lines(right), true),
        "result.txt".into(),
    )
}

/// One conflict on line 1 and an applied right-only change on line 4.
fn sample() -> App {
    merge(
        &["a", "b", "c", "d", "e"],
        &["a", "L", "c", "d", "e"],
        &["a", "R", "c", "d", "E"],
    )
}

fn keys(app: &mut App, s: &str) {
    for c in s.chars() {
        app.handle_key(if c == '\x1b' { Key::Esc } else { Key::Char(c) });
    }
}

/// Draws on a 62x10 screen: three panes of 20 columns with a 6 column
/// gutter each, pane rows at y = 1..=7, status at y = 8, command line at y = 9.
fn render(app: &mut App) -> (Buffer, Position) {
    let mut t = Terminal::new(TestBackend::new(62, 10)).unwrap();
    t.draw(|f| draw(f, app)).unwrap();
    (t.backend().buffer().clone(), t.backend().cursor_position())
}

fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

/// The text of one pane (0, 1, 2) on a screen row.
fn pane(buf: &Buffer, p: usize, y: u16) -> String {
    row(buf, y).split('│').nth(p).unwrap().to_string()
}

fn screen(buf: &Buffer) -> String {
    (0..buf.area.height)
        .map(|y| row(buf, y))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Background of the first text cell of a pane.
fn bg(buf: &Buffer, p: u16, y: u16) -> Color {
    buf[(p * 21 + 6, y)].bg
}

#[test]
fn the_header_names_the_three_panes() {
    let (buf, _) = render(&mut sample());
    assert!(pane(&buf, 0, 0).contains("Local"));
    assert!(pane(&buf, 1, 0).contains("Result"));
    assert!(pane(&buf, 2, 0).contains("Incoming"));
}

#[test]
fn each_pane_shows_its_version_of_a_line_on_the_same_row() {
    let (buf, _) = render(&mut sample());
    assert!(
        pane(&buf, 0, 2).ends_with("2 L             "),
        "{:?}",
        pane(&buf, 0, 2)
    );
    assert!(pane(&buf, 1, 2).ends_with("2 b             "));
    assert!(pane(&buf, 2, 2).ends_with("2 R             "));
}

#[test]
fn a_shorter_side_is_padded_with_filler_rows() {
    let mut a = merge(&["a", "b", "c"], &["a", "L1", "L2", "c"], &["a", "R", "c"]);
    let (buf, _) = render(&mut a);
    assert!(pane(&buf, 0, 3).contains("L2"));
    assert!(pane(&buf, 1, 3).contains("╱╱╱"));
    assert!(pane(&buf, 1, 4).ends_with("3 c             "));
}

#[test]
fn pending_sides_show_arrows_and_handled_sides_a_mark() {
    let mut a = sample();
    let (buf, _) = render(&mut a);
    assert!(pane(&buf, 0, 2).starts_with('»'));
    assert!(pane(&buf, 2, 2).starts_with('«'));
    keys(&mut a, "LXR");
    let (buf, _) = render(&mut a);
    assert!(pane(&buf, 0, 2).starts_with('✓'));
    assert!(pane(&buf, 2, 2).starts_with('✕'));
}

#[test]
fn an_unresolved_conflict_is_tinted_in_every_pane() {
    let (buf, _) = render(&mut sample());
    for p in 0..3 {
        assert_ne!(bg(&buf, p, 2), Color::Reset, "pane {p}");
        assert_eq!(bg(&buf, p, 1), Color::Reset, "pane {p}");
    }
}

#[test]
fn the_conflict_under_the_cursor_is_highlighted_more_strongly() {
    let mut a = merge(
        &["a", "b", "c", "d", "e"],
        &["a", "L", "c", "M", "e"],
        &["a", "R", "c", "S", "e"],
    );
    let (buf, _) = render(&mut a);
    assert_ne!(bg(&buf, 1, 2), bg(&buf, 1, 4));
    keys(&mut a, "n");
    let (after, _) = render(&mut a);
    assert_eq!(bg(&after, 1, 4), bg(&buf, 1, 2));
    assert_eq!(bg(&after, 1, 2), bg(&buf, 1, 4));
}

#[test]
fn a_resolved_conflict_loses_its_tint_once_the_cursor_leaves() {
    let mut a = sample();
    keys(&mut a, "L");
    let (buf, _) = render(&mut a);
    assert_ne!(bg(&buf, 1, 2), Color::Reset);
    keys(&mut a, "gg");
    let (buf, _) = render(&mut a);
    for p in 0..3 {
        assert_eq!(bg(&buf, p, 2), Color::Reset, "pane {p}");
    }
}

#[test]
fn changed_words_stand_out_within_a_conflict_line() {
    let mut a = merge(&["x = foo(1)"], &["x = bar(1)"], &["y = foo(1)"]);
    let (buf, _) = render(&mut a);
    // Left pane text starts at x = 6: "x = bar(1)"; "bar" is at columns 4..7.
    assert_ne!(buf[(6 + 4, 1)].bg, buf[(6, 1)].bg);
    assert_eq!(buf[(6 + 8, 1)].bg, buf[(6, 1)].bg);
    keys(&mut a, " w");
    let (buf, _) = render(&mut a);
    assert_eq!(buf[(6 + 4, 1)].bg, buf[(6, 1)].bg);
}

#[test]
fn the_status_line_reports_mode_and_progress() {
    let mut a = sample();
    let (buf, _) = render(&mut a);
    assert!(row(&buf, 8).contains("NORMAL"));
    assert!(row(&buf, 8).contains("1 conflict unresolved"));
    keys(&mut a, "Li");
    let (buf, _) = render(&mut a);
    assert!(row(&buf, 8).contains("INSERT"));
    assert!(row(&buf, 8).contains("All conflicts resolved"));
}

#[test]
fn the_cursor_sits_on_its_line_in_the_focused_pane() {
    let mut a = sample();
    let (_, cursor) = render(&mut a);
    assert_eq!(cursor, Position::new(21 + 6, 2));
    a.handle_key(Key::Ctrl('l'));
    let (_, cursor) = render(&mut a);
    assert_eq!(cursor, Position::new(42 + 6, 2));
}

#[test]
fn drawing_tells_the_app_how_much_fits_on_screen() {
    let mut a = sample();
    render(&mut a);
    assert_eq!((a.view_h, a.text_w), (7, 14));
}

#[test]
fn the_view_starts_at_the_scrolled_row() {
    let mut a = sample();
    a.top = 3;
    a.handle_key(Key::Ctrl('e'));
    let (buf, _) = render(&mut a);
    assert!(pane(&buf, 1, 1).ends_with("5 E             "));
}

#[test]
fn long_lines_are_cut_at_the_pane_edge_and_scroll_with_the_cursor() {
    let mut a = merge(
        &["0123456789abcdefghij"],
        &["0123456789abcdefghij"],
        &["0123456789abcdefghij"],
    );
    let (buf, _) = render(&mut a);
    assert!(pane(&buf, 1, 1).ends_with("1 0123456789abcd"));
    assert!(pane(&buf, 2, 1).ends_with("1 0123456789abcd"));
    keys(&mut a, "$");
    let (buf, cursor) = render(&mut a);
    assert!(pane(&buf, 1, 1).ends_with("1 6789abcdefghij"));
    assert_eq!(cursor, Position::new(21 + 6 + 13, 1));
}

#[test]
fn tabs_are_drawn_as_spaces_up_to_the_next_tab_stop() {
    let mut a = merge(&["\tx"], &["\tx"], &["\tx"]);
    let (buf, _) = render(&mut a);
    assert!(pane(&buf, 1, 1).ends_with("1     x         "));
}

#[test]
fn the_leader_menu_lists_its_commands() {
    let mut a = sample();
    keys(&mut a, " ");
    let (buf, _) = render(&mut a);
    assert!(screen(&buf).contains("a  Apply all non-conflicting changes"));
    keys(&mut a, "\x1b");
    let (buf, _) = render(&mut a);
    assert!(!screen(&buf).contains("Apply all"));
}

#[test]
fn the_command_line_shows_what_is_being_typed() {
    let mut a = sample();
    keys(&mut a, ":wq");
    let (buf, cursor) = render(&mut a);
    assert!(row(&buf, 9).starts_with(":wq "));
    assert_eq!(cursor, Position::new(3, 9));
}

#[test]
fn messages_appear_on_the_bottom_line() {
    let mut a = sample();
    keys(&mut a, "ggL");
    let (buf, _) = render(&mut a);
    assert!(row(&buf, 9).contains("No conflict under the cursor"));
}

#[test]
fn the_visual_selection_is_highlighted() {
    let mut a = merge(&["abc"], &["abc"], &["abc"]);
    keys(&mut a, "vl");
    let (buf, _) = render(&mut a);
    assert_ne!(buf[(27, 1)].bg, Color::Reset);
    assert_ne!(buf[(28, 1)].bg, Color::Reset);
    assert_eq!(buf[(29, 1)].bg, Color::Reset);
}

#[test]
fn a_tiny_terminal_does_not_crash() {
    let mut a = sample();
    for (w, h) in [(0, 0), (1, 1), (5, 2), (12, 3), (20, 4)] {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| draw(f, &mut a)).unwrap();
    }
}

/// Background of a pane's last column, which is never emphasised text.
fn tint(buf: &Buffer, p: u16, y: u16) -> Color {
    buf[(p * 21 + 19, y)].bg
}

/// Like `merge`, but nothing is applied on load, so every change is pending.
fn unapplied(base: &[&str], left: &[&str], right: &[&str]) -> App {
    App::new(
        Doc::new(lines(base), lines(left), lines(right), false),
        "result.txt".into(),
    )
}

#[test]
fn local_and_incoming_changes_have_different_colors() {
    let (buf, _) = render(&mut sample());
    assert_ne!(tint(&buf, 0, 2), Color::Reset);
    assert_ne!(tint(&buf, 2, 2), Color::Reset);
    assert_ne!(tint(&buf, 0, 2), tint(&buf, 2, 2));
}

#[test]
fn a_conflict_lands_in_the_result_in_a_color_of_its_own() {
    let (buf, _) = render(&mut sample());
    assert_ne!(tint(&buf, 1, 2), Color::Reset);
    assert_ne!(tint(&buf, 1, 2), tint(&buf, 0, 2));
    assert_ne!(tint(&buf, 1, 2), tint(&buf, 2, 2));
}

#[test]
fn a_one_sided_change_lands_in_the_result_in_the_color_of_its_side() {
    let mut a = unapplied(
        &["a", "b", "c", "d", "e"],
        &["a", "L", "c", "d", "e"],
        &["a", "b", "c", "R", "e"],
    );
    keys(&mut a, "G");
    let (buf, _) = render(&mut a);
    assert_ne!(tint(&buf, 0, 2), tint(&buf, 2, 4));
    assert_eq!(tint(&buf, 1, 2), tint(&buf, 0, 2));
    assert_eq!(tint(&buf, 1, 4), tint(&buf, 2, 4));
}

#[test]
fn ignoring_one_side_leaves_the_landing_zone_in_the_other_sides_color() {
    let mut a = sample();
    let (before, _) = render(&mut a);
    keys(&mut a, "XL");
    let (buf, _) = render(&mut a);
    assert_eq!(tint(&buf, 2, 2), tint(&before, 2, 2));
    assert_eq!(tint(&buf, 1, 2), tint(&buf, 2, 2));
    assert_eq!(buf[(21, 2)].fg, buf[(42, 2)].fg);
    assert_ne!(tint(&buf, 0, 2), tint(&before, 0, 2));
}

/// Two conflicts: the one on row 2 is under the cursor, the one on row 4 is not.
fn two_conflicts() -> App {
    merge(
        &["a", "b", "c", "d", "e"],
        &["a", "L", "c", "M", "e"],
        &["a", "R", "c", "S", "e"],
    )
}

#[test]
fn side_headings_have_the_background_of_their_changes() {
    let (buf, _) = render(&mut two_conflicts());
    assert_eq!(tint(&buf, 0, 0), tint(&buf, 0, 4));
    assert_eq!(tint(&buf, 2, 0), tint(&buf, 2, 4));
    assert_eq!(tint(&buf, 1, 0), Color::Reset);
}

#[test]
fn the_focused_side_heading_has_the_background_of_the_highlighted_change() {
    let mut a = two_conflicts();
    a.handle_key(Key::Ctrl('h'));
    let (buf, _) = render(&mut a);
    assert_eq!(tint(&buf, 0, 0), tint(&buf, 0, 2));
    assert_eq!(tint(&buf, 2, 0), tint(&buf, 2, 4));
}

/// Foreground of the text cell at column `col` of a pane.
fn fg(buf: &Buffer, p: u16, y: u16, col: u16) -> Color {
    buf[(p * 21 + 6 + col, y)].fg
}

fn color_of(token: Token) -> Color {
    syntax_color(token).expect("a colored token").into()
}

/// The same file on all three sides, named so that it has a language.
fn code(file: &str, text: &[&str]) -> App {
    let doc = Doc::new(lines(text), lines(text), lines(text), false);
    App::new(doc, file.into())
}

#[test]
fn code_is_colored_in_every_pane() {
    let (buf, _) = render(&mut code("a.rb", &["# note", "x = 1"]));
    for p in 0..3 {
        assert_eq!(fg(&buf, p, 1, 0), color_of(Token::Comment), "pane {p}");
        assert_eq!(fg(&buf, p, 2, 0), Color::Reset, "pane {p}");
        assert_eq!(fg(&buf, p, 2, 4), color_of(Token::Constant), "pane {p}");
    }
}

#[test]
fn a_file_without_language_is_not_colored() {
    let (buf, _) = render(&mut code("a.txt", &["# note"]));
    assert_eq!(fg(&buf, 1, 1, 0), Color::Reset);
}

#[test]
fn colors_follow_the_text_past_wide_chars_and_tabs() {
    let (buf, _) = render(&mut code("a.rb", &["\tä = '日' # c"]));
    assert_eq!(fg(&buf, 1, 1, 8), color_of(Token::Str));
    assert_eq!(fg(&buf, 1, 1, 9), color_of(Token::Str));
    assert_eq!(fg(&buf, 1, 1, 13), color_of(Token::Comment));
}

#[test]
fn a_changed_line_keeps_the_colors_of_its_code() {
    let mut app = App::new(
        Doc::new(
            lines(&["x = 1"]),
            lines(&["x = 2"]),
            lines(&["x = 3"]),
            false,
        ),
        "a.rb".into(),
    );
    let (buf, _) = render(&mut app);
    for p in 0..3 {
        assert_eq!(fg(&buf, p, 1, 4), color_of(Token::Constant), "pane {p}");
        assert_ne!(tint(&buf, p, 1), Color::Reset, "pane {p}");
    }
}

#[test]
fn the_result_is_recolored_as_it_is_edited() {
    let mut app = code("a.rb", &["x = 1", "y = 2"]);
    render(&mut app);
    keys(&mut app, "I# \x1b");
    let (buf, _) = render(&mut app);
    assert_eq!(fg(&buf, 1, 1, 0), color_of(Token::Comment));
    assert_eq!(fg(&buf, 1, 1, 6), color_of(Token::Comment));
    assert_eq!(fg(&buf, 0, 1, 4), color_of(Token::Constant));
    assert_eq!(fg(&buf, 1, 2, 4), color_of(Token::Constant));
}

#[test]
fn a_changed_word_in_a_comment_is_not_drawn_in_the_faint_comment_color() {
    let mut app = App::new(
        Doc::new(
            lines(&["# foo"]),
            lines(&["# bar"]),
            lines(&["# foo"]),
            false,
        ),
        "a.rb".into(),
    );
    let (buf, _) = render(&mut app);
    assert_eq!(fg(&buf, 0, 1, 0), color_of(Token::Comment));
    assert_eq!(fg(&buf, 0, 1, 2), Color::Reset);
}

#[test]
fn drawing_again_colors_the_lines_below_the_screen() {
    let text: Vec<String> = (0..3000).map(|i| format!("x = {i} # note")).collect();
    let mut app = App::new(
        Doc::new(text.clone(), text.clone(), text, false),
        "a.rb".into(),
    );
    let mut draws = 0;
    render(&mut app);
    while app.highlighting {
        render(&mut app);
        draws += 1;
        assert!(draws < 10_000, "coloring never finishes");
    }
    for p in 0..3 {
        assert!(!app.syntax[p].spans(2999).is_empty(), "pane {p}");
    }
}

#[test]
fn a_file_without_language_leaves_nothing_to_color() {
    let mut app = code("a.txt", &["x"]);
    render(&mut app);
    assert!(!app.highlighting);
}
