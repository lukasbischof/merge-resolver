use super::*;
use crate::doc::{Doc, LEFT, OUT, RIGHT};

fn lines(s: &[&str]) -> Vec<String> {
    s.iter().map(|l| l.to_string()).collect()
}

fn merge(base: &[&str], left: &[&str], right: &[&str]) -> App {
    let name = format!(
        "merge-resolver-test-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    );
    App::new(
        Doc::new(lines(base), lines(left), lines(right), true),
        std::env::temp_dir().join(name),
    )
}

/// A result buffer with the given text and no conflicts.
fn edit(text: &[&str]) -> App {
    merge(text, text, text)
}

/// One conflict on line 1 and an applied right-only change on line 4.
fn sample() -> App {
    merge(
        &["a", "b", "c", "d", "e"],
        &["a", "L", "c", "d", "e"],
        &["a", "R", "c", "d", "E"],
    )
}

/// Conflicts on lines 1 and 3.
fn two_conflicts() -> App {
    merge(
        &["a", "b", "c", "d", "e"],
        &["a", "L", "c", "M", "e"],
        &["a", "R", "c", "S", "e"],
    )
}

/// Feeds keys; `<esc> <cr> <bs> <tab> <space> <lt> <c-x>` are special.
fn feed(a: &mut App, keys: &str) {
    let mut rest = keys;
    while let Some(c) = rest.chars().next() {
        let mut key = Key::Char(c);
        let mut len = c.len_utf8();
        if c == '<'
            && let Some(end) = rest.find('>')
        {
            len = end + 1;
            key = match &rest[1..end] {
                "esc" => Key::Esc,
                "cr" => Key::Enter,
                "bs" => Key::Backspace,
                "tab" => Key::Tab,
                "space" => Key::Char(' '),
                "lt" => Key::Char('<'),
                name => Key::Ctrl(name.strip_prefix("c-").unwrap().chars().next().unwrap()),
            };
        }
        a.handle_key(key);
        rest = &rest[len..];
    }
}

fn typed(text: &[&str], keys: &str) -> Vec<String> {
    let mut a = edit(text);
    feed(&mut a, keys);
    a.doc.lines.clone()
}

fn cursor(text: &[&str], keys: &str) -> (usize, usize) {
    let mut a = edit(text);
    feed(&mut a, keys);
    (a.cur.line, a.cur.col)
}

fn out(a: &App) -> String {
    a.doc.lines.join(" ")
}

fn is_error(a: &App) -> bool {
    matches!(a.message, Some((_, true)))
}

// ----- vim editing in the result pane ---------------------------------

#[test]
fn x_deletes_the_char_under_the_cursor() {
    assert_eq!(typed(&["hello"], "x"), ["ello"]);
    assert_eq!(typed(&["hello"], "3x"), ["lo"]);
}

#[test]
fn dw_deletes_to_the_start_of_the_next_word() {
    assert_eq!(typed(&["foo bar baz"], "dw"), ["bar baz"]);
    assert_eq!(typed(&["foo bar baz"], "2dw"), ["baz"]);
}

#[test]
fn dw_on_the_last_word_does_not_join_the_next_line() {
    assert_eq!(typed(&["foo", "bar"], "dw"), ["", "bar"]);
}

#[test]
fn cw_changes_the_word_but_keeps_the_following_space() {
    assert_eq!(typed(&["foo bar"], "cwxy<esc>"), ["xy bar"]);
    assert_eq!(typed(&["a b c"], "cwxy<esc>"), ["xy b c"]);
    assert_eq!(typed(&["a b c"], "2cwxy<esc>"), ["xy c"]);
}

#[test]
fn dd_deletes_lines_and_p_puts_them_back_below() {
    assert_eq!(typed(&["a", "b", "c"], "ddp"), ["b", "a", "c"]);
    assert_eq!(typed(&["a", "b", "c", "d"], "2dd"), ["c", "d"]);
    assert_eq!(typed(&["a", "b"], "yyP"), ["a", "a", "b"]);
}

#[test]
fn capital_d_deletes_to_the_end_of_the_line() {
    assert_eq!(typed(&["foo bar"], "wD"), ["foo "]);
    assert_eq!(typed(&["foo bar"], "wd$"), ["foo "]);
}

#[test]
fn insert_commands_start_typing_at_the_right_place() {
    assert_eq!(typed(&["ab"], "ix<esc>"), ["xab"]);
    assert_eq!(typed(&["ab"], "ax<esc>"), ["axb"]);
    assert_eq!(typed(&["ab"], "Ac<esc>"), ["abc"]);
    assert_eq!(typed(&["  ab"], "Ix<esc>"), ["  xab"]);
}

#[test]
fn leaving_insert_mode_moves_the_cursor_back_one_column() {
    assert_eq!(cursor(&["ab"], "Acd<esc>"), (0, 3));
}

#[test]
fn o_opens_an_indented_line_below_and_capital_o_above() {
    assert_eq!(typed(&["  a"], "ob<esc>"), ["  a", "  b"]);
    assert_eq!(typed(&["a"], "Ob<esc>"), ["b", "a"]);
}

#[test]
fn enter_splits_the_line_and_backspace_joins_it_again() {
    assert_eq!(typed(&["ab"], "li<cr><esc>"), ["a", "b"]);
    assert_eq!(typed(&["a", "b"], "ji<bs><esc>"), ["ab"]);
    assert_eq!(typed(&["abc"], "A<bs><bs><esc>"), ["a"]);
}

#[test]
fn u_undoes_one_command_at_a_time_and_ctrl_r_redoes() {
    assert_eq!(typed(&["abc"], "xxu"), ["bc"]);
    assert_eq!(typed(&["abc"], "xxuu"), ["abc"]);
    assert_eq!(typed(&["abc"], "xu<c-r>"), ["bc"]);
}

#[test]
fn an_insert_session_is_undone_as_one_step() {
    assert_eq!(typed(&["a"], "xAbcd<cr>e<esc>u"), [""]);
}

#[test]
fn undo_puts_the_cursor_back_where_the_change_started() {
    assert_eq!(cursor(&["a", "b", "c"], "jddggu"), (1, 0));
}

#[test]
fn dot_repeats_the_last_change() {
    assert_eq!(typed(&["a b c d"], "dw."), ["c d"]);
    assert_eq!(typed(&["one", "two"], "Ax<esc>j."), ["onex", "twox"]);
    assert_eq!(typed(&["a b", "c d"], "cwxy<esc>j0."), ["xy b", "xy d"]);
}

#[test]
fn text_objects_select_words_brackets_and_quotes() {
    assert_eq!(typed(&["foo bar"], "wciwx<esc>"), ["foo x"]);
    assert_eq!(typed(&["foo bar baz"], "wdaw"), ["foo baz"]);
    assert_eq!(typed(&["f(a, b)"], "fadi("), ["f()"]);
    assert_eq!(typed(&["f(a, b) c"], "fada("), ["f c"]);
    assert_eq!(typed(&["s = \"hi\""], "ci\"x<esc>"), ["s = \"x\""]);
}

#[test]
fn inner_block_spanning_lines_is_changed_linewise() {
    assert_eq!(
        typed(&["f {", "  a", "  b", "}"], "jci{x<esc>"),
        ["f {", "  x", "}"]
    );
}

#[test]
fn capital_j_joins_lines_with_a_single_space() {
    assert_eq!(typed(&["a", "  b"], "J"), ["a b"]);
    assert_eq!(typed(&["a", "b", "c"], "3J"), ["a b c"]);
}

#[test]
fn r_replaces_and_tilde_toggles_case() {
    assert_eq!(typed(&["abc"], "rx"), ["xbc"]);
    assert_eq!(typed(&["abc"], "~~"), ["ABc"]);
}

#[test]
fn visual_mode_operates_on_the_selection() {
    assert_eq!(typed(&["hello world"], "vld"), ["llo world"]);
    assert_eq!(typed(&["ab cd"], "veyP"), ["abab cd"]);
    assert_eq!(typed(&["a", "b", "c"], "Vjd"), ["c"]);
    assert_eq!(typed(&["foo bar"], "wviwcx<esc>"), ["foo x"]);
}

#[test]
fn angle_brackets_shift_lines_by_the_detected_indent() {
    assert_eq!(typed(&["a"], ">>"), ["  a"]);
    assert_eq!(typed(&["    a", "    b"], "<lt>j"), ["a", "b"]);
    assert_eq!(typed(&["a", "\tb"], ">>"), ["\ta", "\tb"]);
}

#[test]
fn word_and_line_motions_move_the_cursor() {
    let text = ["foo bar", "baz"];
    assert_eq!(cursor(&text, "w"), (0, 4));
    assert_eq!(cursor(&text, "e"), (0, 2));
    assert_eq!(cursor(&text, "$"), (0, 6));
    assert_eq!(cursor(&text, "ww"), (1, 0));
    assert_eq!(cursor(&text, "wwb"), (0, 4));
    assert_eq!(cursor(&text, "G"), (1, 0));
    assert_eq!(cursor(&text, "Ggg"), (0, 0));
    assert_eq!(cursor(&["  x"], "$^"), (0, 2));
}

#[test]
fn l_stops_at_the_last_char_of_the_line() {
    assert_eq!(cursor(&["abc"], "5l"), (0, 2));
}

#[test]
fn j_and_k_remember_the_column_across_short_lines() {
    assert_eq!(cursor(&["abcdef", "ab", "abcdef"], "4ljj"), (2, 4));
    assert_eq!(cursor(&["abc", "a", "abcdef"], "$jj"), (2, 5));
}

#[test]
fn find_motions_work_alone_and_with_operators() {
    assert_eq!(cursor(&["a,b,c"], "f,"), (0, 1));
    assert_eq!(cursor(&["a,b,c"], "f,;"), (0, 3));
    assert_eq!(cursor(&["a,b,c"], "$F,"), (0, 3));
    assert_eq!(typed(&["a,b,c"], "df,"), ["b,c"]);
    assert_eq!(typed(&["a,b,c"], "dt,"), [",b,c"]);
}

#[test]
fn percent_jumps_to_the_matching_bracket() {
    assert_eq!(cursor(&["(a(b))"], "%"), (0, 5));
    assert_eq!(cursor(&["(a(b))"], "$%"), (0, 0));
    assert_eq!(cursor(&["f {", "}"], "%"), (1, 0));
}

#[test]
fn linewise_motions_make_operators_linewise() {
    assert_eq!(typed(&["a", "b", "c"], "jdG"), ["a"]);
    assert_eq!(typed(&["a", "b", "c"], "jdgg"), ["c"]);
    assert_eq!(typed(&["a", "b", "c"], "dj"), ["c"]);
}

#[test]
fn cc_replaces_the_line_but_keeps_its_indent() {
    assert_eq!(typed(&["  foo", "bar"], "ccx<esc>"), ["  x", "bar"]);
    assert_eq!(typed(&["  foo", "bar"], "Sx<esc>"), ["  x", "bar"]);
}

#[test]
fn p_pastes_charwise_text_after_the_cursor() {
    assert_eq!(typed(&["abc"], "ylp"), ["aabc"]);
    assert_eq!(typed(&["ab"], "xp"), ["ba"]);
}

#[test]
fn insert_mode_ctrl_w_deletes_the_word_before_the_cursor() {
    assert_eq!(typed(&["foo bar"], "A<c-w>x<esc>"), ["foo x"]);
}

#[test]
fn deleting_every_line_leaves_an_editable_empty_buffer() {
    assert_eq!(typed(&["a", "b"], "dGix<esc>"), ["x"]);
    assert_eq!(typed(&["a"], "ddp"), ["a"]);
}

#[test]
fn search_jumps_to_the_next_match_and_gn_repeats_it() {
    let text = ["foo", "bar", "a foo"];
    assert_eq!(cursor(&text, "/foo<cr>"), (2, 2));
    assert_eq!(cursor(&text, "/foo<cr>gn"), (0, 0));
    assert_eq!(cursor(&text, "/foo<cr>gN"), (0, 0));
    assert_eq!(cursor(&text, "?bar<cr>"), (1, 0));
    assert_eq!(cursor(&text, "*"), (2, 2));
}

#[test]
fn search_is_case_insensitive_unless_the_pattern_has_uppercase() {
    assert_eq!(cursor(&["x", "Foo", "foo"], "/foo<cr>"), (1, 0));
    assert_eq!(cursor(&["x", "foo", "Foo"], "/Foo<cr>"), (2, 0));
}

#[test]
fn colon_number_goes_to_that_line() {
    assert_eq!(cursor(&["a", "b", "c"], ":2<cr>"), (1, 0));
}

// ----- conflicts ----------------------------------------------------

#[test]
fn starts_in_the_result_pane_on_the_first_conflict() {
    let a = sample();
    assert_eq!((a.pane, a.cur.line), (OUT, 1));
}

#[test]
fn capital_l_and_r_accept_a_side() {
    let mut a = sample();
    feed(&mut a, "L");
    assert_eq!(out(&a), "a L c d E");
    let mut a = sample();
    feed(&mut a, "R");
    assert_eq!(out(&a), "a R c d E");
}

#[test]
fn lr_and_rl_accept_both_sides_in_the_typed_order() {
    let mut a = sample();
    feed(&mut a, "LR");
    assert_eq!(out(&a), "a L R c d E");
    let mut a = sample();
    feed(&mut a, "RL");
    assert_eq!(out(&a), "a R L c d E");
}

#[test]
fn xl_ignores_left_so_that_r_takes_only_right() {
    let mut a = sample();
    feed(&mut a, "XL");
    assert_eq!(a.doc.unresolved(), (1, 0));
    feed(&mut a, "R");
    assert_eq!(out(&a), "a R c d E");
}

#[test]
fn ignoring_both_sides_resolves_and_keeps_the_result() {
    let mut a = sample();
    feed(&mut a, "XLXR");
    assert_eq!(out(&a), "a b c d E");
    assert_eq!(a.doc.unresolved(), (0, 0));
}

#[test]
fn u_undoes_an_accept_and_reopens_the_conflict() {
    let mut a = sample();
    feed(&mut a, "Lu");
    assert_eq!(out(&a), "a b c d E");
    assert_eq!(a.doc.unresolved(), (1, 0));
    feed(&mut a, "R");
    assert_eq!(out(&a), "a R c d E");
}

#[test]
fn conflict_keys_off_a_conflict_do_nothing_but_explain() {
    let mut a = sample();
    feed(&mut a, "ggL");
    assert_eq!(out(&a), "a b c d E");
    assert!(is_error(&a));
}

#[test]
fn accepting_a_deletion_keeps_the_conflict_active_for_the_other_side() {
    let mut a = merge(&["a", "b", "c"], &["a", "c"], &["a", "X", "c"]);
    feed(&mut a, "L");
    assert_eq!(out(&a), "a c");
    feed(&mut a, "R");
    assert_eq!(out(&a), "a X c");
}

#[test]
fn ctrl_h_and_ctrl_l_switch_panes_and_keep_the_row() {
    let mut a = sample();
    feed(&mut a, "<c-h>");
    assert_eq!((a.pane, a.cur.line), (LEFT, 1));
    feed(&mut a, "<c-l><c-l>");
    assert_eq!((a.pane, a.cur.line), (RIGHT, 1));
    feed(&mut a, "<c-w>h");
    assert_eq!(a.pane, OUT);
}

#[test]
fn pane_switch_lands_on_the_line_shown_on_the_same_row() {
    let mut a = merge(&["a", "b", "c"], &["a", "L1", "L2", "c"], &["a", "R", "c"]);
    feed(&mut a, "j<c-h>");
    assert_eq!((a.pane, a.cur.line), (LEFT, 3));
}

#[test]
fn plain_h_and_l_move_within_the_pane() {
    let mut a = merge(&["ab", "x"], &["ab", "L"], &["ab", "R"]);
    feed(&mut a, "ggl");
    assert_eq!((a.pane, a.cur.col), (OUT, 1));
    feed(&mut a, "h");
    assert_eq!((a.pane, a.cur.col), (OUT, 0));
}

#[test]
fn conflict_keys_work_from_the_side_panes() {
    let mut a = sample();
    feed(&mut a, "<c-h>L");
    assert_eq!(out(&a), "a L c d E");
    feed(&mut a, "<c-l><c-l>R");
    assert_eq!(out(&a), "a L R c d E");
}

#[test]
fn side_panes_reject_edits() {
    let mut a = sample();
    feed(&mut a, "<c-h>x");
    assert_eq!(a.doc.left, ["a", "L", "c", "d", "e"]);
    assert_eq!(out(&a), "a b c d E");
    assert!(is_error(&a));
    feed(&mut a, "ix<esc>");
    assert_eq!(a.doc.left, ["a", "L", "c", "d", "e"]);
    assert_eq!(a.mode, Mode::Normal);
}

#[test]
fn text_yanked_in_a_side_pane_can_be_pasted_into_the_result() {
    let mut a = sample();
    feed(&mut a, "<c-h>yy<c-l>p");
    assert_eq!(out(&a), "a b L c d E");
}

#[test]
fn n_jumps_between_unresolved_conflicts_and_wraps() {
    let mut a = two_conflicts();
    feed(&mut a, "n");
    assert_eq!(a.cur.line, 3);
    feed(&mut a, "n");
    assert_eq!(a.cur.line, 1);
    feed(&mut a, "N");
    assert_eq!(a.cur.line, 3);
}

#[test]
fn n_skips_resolved_conflicts() {
    let mut a = two_conflicts();
    feed(&mut a, "Lggn");
    assert_eq!(a.cur.line, 3);
    feed(&mut a, "n");
    assert_eq!(a.cur.line, 3);
}

#[test]
fn n_goes_to_the_conflict_start_in_the_current_pane() {
    let mut a = merge(
        &["a", "b", "c", "d", "e"],
        &["a", "L1", "L2", "c", "M", "e"],
        &["a", "R", "c", "S", "e"],
    );
    feed(&mut a, "<c-h>n");
    assert_eq!((a.pane, a.cur.line), (LEFT, 4));
}

#[test]
fn bracket_c_also_visits_resolved_changes() {
    let mut a = sample();
    feed(&mut a, "gg]c");
    assert_eq!(a.cur.line, 1);
    feed(&mut a, "]c");
    assert_eq!(a.cur.line, 4);
    feed(&mut a, "[c");
    assert_eq!(a.cur.line, 1);
}

#[test]
fn typing_in_a_conflict_then_accepting_replaces_the_typed_text() {
    let mut a = sample();
    feed(&mut a, "Ax<esc>L");
    assert_eq!(out(&a), "a L c d E");
}

// ----- leader menu --------------------------------------------------

#[test]
fn space_opens_the_leader_menu_and_escape_closes_it() {
    let mut a = sample();
    feed(&mut a, "<space>");
    assert_eq!(a.mode, Mode::Leader);
    feed(&mut a, "<esc>");
    assert_eq!(a.mode, Mode::Normal);
}

#[test]
fn leader_m_resolves_simple_conflicts() {
    let mut a = merge(&["f(a, b)"], &["f(a2, b)"], &["f(a, b2)"]);
    feed(&mut a, "<space>m");
    assert_eq!(a.doc.lines, ["f(a2, b2)"]);
    assert_eq!(a.mode, Mode::Normal);
}

#[test]
fn leader_u_resets_the_conflict_under_the_cursor() {
    let mut a = sample();
    feed(&mut a, "L");
    assert_eq!(out(&a), "a L c d E");
    feed(&mut a, "<space>u");
    assert_eq!(out(&a), "a b c d E");
    assert_eq!(a.doc.unresolved(), (1, 0));
}

#[test]
fn leader_capital_l_and_r_take_a_whole_file() {
    let mut a = sample();
    feed(&mut a, "<space>L");
    assert_eq!(out(&a), "a L c d e");
    feed(&mut a, "<space>R");
    assert_eq!(out(&a), "a R c d E");
    feed(&mut a, "u");
    assert_eq!(out(&a), "a L c d e");
}

#[test]
fn leader_a_applies_pending_non_conflicting_changes() {
    let mut a = sample();
    feed(&mut a, "Gk]c<space>u");
    assert_eq!(out(&a), "a b c d e");
    feed(&mut a, "<space>a");
    assert_eq!(out(&a), "a b c d E");
}

#[test]
fn leader_question_mark_shows_help_until_a_key_is_pressed() {
    let mut a = sample();
    feed(&mut a, "<space>?");
    assert_eq!(a.mode, Mode::Help);
    feed(&mut a, "x");
    assert_eq!(a.mode, Mode::Normal);
    assert_eq!(out(&a), "a b c d E");
}

// ----- saving and quitting ------------------------------------------

fn written(a: &App) -> String {
    std::fs::read_to_string(&a.path).unwrap()
}

#[test]
fn colon_w_writes_the_result_and_q_then_exits_successfully() {
    let mut a = sample();
    feed(&mut a, "L:w<cr>");
    assert_eq!(written(&a), "a\nL\nc\nd\nE\n");
    assert_eq!(a.quit, None);
    feed(&mut a, ":q<cr>");
    assert_eq!(a.quit, Some(0));
}

#[test]
fn quitting_without_ever_writing_exits_with_failure_and_writes_nothing() {
    let mut a = sample();
    let _ = std::fs::remove_file(&a.path);
    feed(&mut a, "L:q<cr>");
    assert_eq!(a.quit, Some(1));
    assert!(!a.path.exists());
    let mut a = sample();
    feed(&mut a, "ZQ");
    assert_eq!(a.quit, Some(1));
}

#[test]
fn q_after_w_keeps_the_last_written_state() {
    let mut a = sample();
    feed(&mut a, "L:w<cr>ggdd:q<cr>");
    assert_eq!(written(&a), "a\nL\nc\nd\nE\n");
    assert_eq!(a.quit, Some(0));
}

#[test]
fn zz_writes_and_quits_once_everything_is_resolved() {
    let mut a = sample();
    feed(&mut a, "RZZ");
    assert_eq!(written(&a), "a\nR\nc\nd\nE\n");
    assert_eq!(a.quit, Some(0));
}

#[test]
fn zz_refuses_to_quit_while_conflicts_are_unresolved() {
    let mut a = sample();
    let _ = std::fs::remove_file(&a.path);
    feed(&mut a, "ZZ");
    assert_eq!(a.quit, None);
    assert!(!a.path.exists());
    assert!(is_error(&a));
    feed(&mut a, ":wq<cr>");
    assert_eq!(a.quit, None);
    feed(&mut a, ":wq!<cr>");
    assert_eq!(a.quit, Some(0));
    assert_eq!(written(&a), "a\nb\nc\nd\nE\n");
}

#[test]
fn cq_exits_with_failure_even_after_writing() {
    let mut a = sample();
    feed(&mut a, "L:w<cr>:cq<cr>");
    assert_eq!(a.quit, Some(1));
}

#[test]
fn writing_preserves_line_endings_and_a_missing_final_newline() {
    let mut a = sample();
    a.eol = "\r\n";
    a.final_newline = false;
    feed(&mut a, "L:w<cr>");
    assert_eq!(written(&a), "a\r\nL\r\nc\r\nd\r\nE");
}

#[test]
fn unknown_commands_are_reported() {
    let mut a = sample();
    feed(&mut a, ":bogus<cr>");
    assert!(is_error(&a));
    assert_eq!(a.quit, None);
}

// ----- scrolling ----------------------------------------------------

fn tall() -> App {
    let mut a = edit(&["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"]);
    a.view_h = 4;
    a
}

#[test]
fn the_view_follows_the_cursor() {
    let mut a = tall();
    feed(&mut a, "G");
    assert_eq!(a.top, 6);
    feed(&mut a, "gg");
    assert_eq!(a.top, 0);
}

#[test]
fn ctrl_d_and_ctrl_u_move_half_a_screen() {
    let mut a = tall();
    feed(&mut a, "<c-d>");
    assert_eq!((a.cur.line, a.top), (2, 2));
    feed(&mut a, "<c-u>");
    assert_eq!((a.cur.line, a.top), (0, 0));
}

#[test]
fn ctrl_e_scrolls_the_view_and_drags_the_cursor_along() {
    let mut a = tall();
    feed(&mut a, "<c-e>");
    assert_eq!((a.cur.line, a.top), (1, 1));
    feed(&mut a, "jj<c-y>");
    assert_eq!((a.cur.line, a.top), (3, 0));
    feed(&mut a, "G<c-y>");
    assert_eq!((a.cur.line, a.top), (8, 5));
}

#[test]
fn zt_and_zz_reposition_the_view_around_the_cursor() {
    let mut a = tall();
    feed(&mut a, "5Gzt");
    assert_eq!(a.top, 4);
    feed(&mut a, "zz");
    assert_eq!(a.top, 2);
    feed(&mut a, "zb");
    assert_eq!(a.top, 1);
}

#[test]
fn scrolling_is_measured_in_aligned_rows_not_lines() {
    // The left side has three extra rows before line "c".
    let mut a = merge(
        &["a", "b", "c", "d"],
        &["a", "1", "2", "3", "4", "c", "d"],
        &["a", "R", "c", "d"],
    );
    a.view_h = 3;
    feed(&mut a, "G");
    assert_eq!((a.cur.line, a.top), (3, 4));
}

#[test]
fn long_lines_scroll_horizontally_with_the_cursor() {
    let mut a = edit(&["0123456789"]);
    a.text_w = 4;
    feed(&mut a, "$");
    assert_eq!(a.hscroll, 6);
    feed(&mut a, "0");
    assert_eq!(a.hscroll, 0);
}

#[test]
fn u_after_accepting_both_sides_undoes_only_the_second() {
    let mut a = sample();
    feed(&mut a, "LRu");
    assert_eq!(out(&a), "a L c d E");
    assert_eq!(a.doc.unresolved(), (0, 0));
}

#[test]
fn a_cancelled_leader_menu_does_not_disturb_undo() {
    let mut a = sample();
    feed(&mut a, "LR<space><esc>u");
    assert_eq!(out(&a), "a L c d E");
}

#[test]
fn visual_commands_on_an_empty_buffer_do_nothing() {
    assert_eq!(typed(&["a"], "ddVd"), [] as [&str; 0]);
    assert_eq!(typed(&["a"], "ddV>"), [] as [&str; 0]);
    assert_eq!(typed(&["a"], "ddvp"), ["a"]);
}
