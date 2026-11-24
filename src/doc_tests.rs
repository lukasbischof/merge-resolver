use super::*;

fn words(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

fn doc(base: &str, left: &str, right: &str, auto: bool) -> Doc {
    Doc::new(words(base), words(left), words(right), auto)
}

/// One conflict on line 1 (b -> L / R) and one right-only change on line 4.
fn sample(auto: bool) -> Doc {
    doc("a b c d e", "a L c d e", "a R c d E", auto)
}

fn text(d: &Doc) -> String {
    d.lines.join(" ")
}

#[test]
fn non_conflicting_changes_are_applied_on_load() {
    let d = sample(true);
    assert_eq!(text(&d), "a b c d E");
    assert_eq!(d.unresolved(), (1, 0));
}

#[test]
fn without_auto_apply_the_result_starts_as_the_base() {
    let d = sample(false);
    assert_eq!(text(&d), "a b c d e");
    assert_eq!(d.unresolved(), (1, 1));
}

#[test]
fn accepting_left_replaces_the_conflict_and_resolves_it() {
    let mut d = sample(true);
    d.accept(0, Side::Left).unwrap();
    assert_eq!(text(&d), "a L c d E");
    assert_eq!(d.unresolved(), (0, 0));
}

#[test]
fn accepting_right_after_left_appends_it() {
    let mut d = sample(true);
    d.accept(0, Side::Left).unwrap();
    d.accept(0, Side::Right).unwrap();
    assert_eq!(text(&d), "a L R c d E");
}

#[test]
fn accepting_left_after_right_appends_it() {
    let mut d = sample(true);
    d.accept(0, Side::Right).unwrap();
    d.accept(0, Side::Left).unwrap();
    assert_eq!(text(&d), "a R L c d E");
}

#[test]
fn accepting_the_same_side_twice_is_rejected() {
    let mut d = sample(true);
    d.accept(0, Side::Left).unwrap();
    assert!(d.accept(0, Side::Left).is_err());
    assert_eq!(text(&d), "a L c d E");
}

#[test]
fn ignoring_one_side_leaves_the_conflict_unresolved() {
    let mut d = sample(true);
    d.ignore(0, Side::Left).unwrap();
    assert_eq!(d.unresolved(), (1, 0));
    assert_eq!(text(&d), "a b c d E");
}

#[test]
fn ignoring_both_sides_resolves_without_changing_the_result() {
    let mut d = sample(true);
    d.ignore(0, Side::Left).unwrap();
    d.ignore(0, Side::Right).unwrap();
    assert_eq!(d.unresolved(), (0, 0));
    assert_eq!(text(&d), "a b c d E");
}

#[test]
fn accepting_right_after_ignoring_left_takes_only_right() {
    let mut d = sample(true);
    d.ignore(0, Side::Left).unwrap();
    d.accept(0, Side::Right).unwrap();
    assert_eq!(text(&d), "a R c d E");
    assert_eq!(d.unresolved(), (0, 0));
}

#[test]
fn ignoring_an_accepted_side_is_rejected() {
    let mut d = sample(true);
    d.accept(0, Side::Left).unwrap();
    assert!(d.ignore(0, Side::Left).is_err());
}

#[test]
fn accepting_a_side_without_a_change_is_rejected() {
    let mut d = sample(false);
    assert!(d.accept(1, Side::Left).is_err());
    assert_eq!(text(&d), "a b c d e");
}

#[test]
fn undo_restores_text_and_conflict_state_and_redo_reapplies() {
    let mut d = sample(true);
    d.begin(Pos { line: 1, col: 0 });
    d.accept(0, Side::Left).unwrap();
    d.commit(Pos { line: 1, col: 0 });
    assert_eq!(d.undo(), Some(Pos { line: 1, col: 0 }));
    assert_eq!(text(&d), "a b c d E");
    assert_eq!(d.unresolved(), (1, 0));
    assert!(d.redo().is_some());
    assert_eq!(text(&d), "a L c d E");
    assert_eq!(d.unresolved(), (0, 0));
}

#[test]
fn undo_with_nothing_to_undo_returns_none() {
    let mut d = sample(true);
    assert_eq!(d.undo(), None);
    assert_eq!(text(&d), "a b c d E");
}

#[test]
fn a_new_change_discards_the_redo_history() {
    let mut d = sample(true);
    d.begin(Pos::default());
    d.accept(0, Side::Left).unwrap();
    d.commit(Pos::default());
    d.undo();
    d.begin(Pos::default());
    d.accept(0, Side::Right).unwrap();
    d.commit(Pos::default());
    assert_eq!(d.redo(), None);
    assert_eq!(text(&d), "a R c d E");
}

#[test]
fn later_conflicts_track_their_lines_when_earlier_ones_grow() {
    let mut d = sample(true);
    d.accept(0, Side::Left).unwrap();
    d.accept(0, Side::Right).unwrap();
    assert_eq!(d.chunks[1].out, 5..6);
    assert_eq!(d.chunk_at(OUT, 5), Some(1));
}

#[test]
fn manual_edits_inside_a_conflict_are_replaced_by_a_later_accept() {
    let mut d = sample(true);
    d.splice(1, 2, words("x y"));
    assert_eq!(text(&d), "a x y c d E");
    d.accept(0, Side::Left).unwrap();
    assert_eq!(text(&d), "a L c d E");
}

#[test]
fn lines_inserted_before_a_conflict_stay_outside_it() {
    let mut d = sample(true);
    d.splice(1, 1, words("new"));
    d.accept(0, Side::Left).unwrap();
    assert_eq!(text(&d), "a new L c d E");
}

#[test]
fn deleting_across_two_conflicts_keeps_them_apart() {
    // Conflicts on b and d; delete lines b..=d (1..4).
    let mut d = doc("a b c d e", "a L c M e", "a R c S e", true);
    d.splice(1, 4, vec![]);
    assert_eq!(text(&d), "a e");
    d.accept(0, Side::Left).unwrap();
    d.accept(1, Side::Right).unwrap();
    assert_eq!(text(&d), "a L S e");
}

#[test]
fn chunk_at_finds_the_conflict_covering_a_line() {
    let d = sample(true);
    assert_eq!(d.chunk_at(OUT, 1), Some(0));
    assert_eq!(d.chunk_at(OUT, 0), None);
    assert_eq!(d.chunk_at(RIGHT, 4), Some(1));
}

#[test]
fn a_side_that_deleted_lines_activates_the_conflict_on_the_following_line() {
    let d = doc("a b c", "a c", "a X c", true);
    assert_eq!(d.chunk_at(LEFT, 1), Some(0));
    assert_eq!(d.chunk_at(LEFT, 0), None);
}

#[test]
fn a_deletion_at_the_end_activates_on_the_last_line() {
    let d = doc("a b", "a", "a X", true);
    assert_eq!(d.chunk_at(LEFT, 0), Some(0));
}

#[test]
fn reset_restores_the_base_and_reopens_the_conflict() {
    let mut d = sample(true);
    d.accept(0, Side::Left).unwrap();
    d.reset(0);
    assert_eq!(text(&d), "a b c d E");
    assert_eq!(d.unresolved(), (1, 0));
}

#[test]
fn apply_non_conflicting_can_be_limited_to_one_side() {
    let mut d = doc("a b c d e", "A b c d e", "a b c d E", false);
    assert_eq!(d.apply_non_conflicting(Some(Side::Right)), 1);
    assert_eq!(text(&d), "a b c d E");
    assert_eq!(d.apply_non_conflicting(None), 1);
    assert_eq!(text(&d), "A b c d E");
}

#[test]
fn an_identical_change_on_both_sides_is_applied_once() {
    let mut d = doc("a b", "a X", "a X", false);
    d.accept(0, Side::Left).unwrap();
    assert!(d.accept(0, Side::Right).is_err());
    assert_eq!(text(&d), "a X");
}

#[test]
fn simple_conflicts_merge_at_word_level() {
    let mut d = Doc::new(
        vec!["x = foo(a, b)".into()],
        vec!["x = foo(a2, b)".into()],
        vec!["x = foo(a, b2)".into()],
        true,
    );
    d.resolve_simple(0).unwrap();
    assert_eq!(d.lines, vec!["x = foo(a2, b2)"]);
    assert_eq!(d.unresolved(), (0, 0));
}

#[test]
fn conflicts_touching_the_same_word_are_not_simple() {
    let mut d = Doc::new(
        vec!["x = 0".into()],
        vec!["x = 1".into()],
        vec!["x = 2".into()],
        true,
    );
    assert!(d.resolve_simple(0).is_err());
    assert_eq!(d.lines, vec!["x = 0"]);
    assert_eq!(d.unresolved(), (1, 0));
}

#[test]
fn resolve_all_simple_reports_how_many_it_resolved() {
    let mut d = Doc::new(
        vec!["f(a, b)".into(), "keep".into(), "x = 0".into()],
        vec!["f(a2, b)".into(), "keep".into(), "x = 1".into()],
        vec!["f(a, b2)".into(), "keep".into(), "x = 2".into()],
        true,
    );
    assert_eq!(d.resolve_all_simple(), 1);
    assert_eq!(d.lines, vec!["f(a2, b2)", "keep", "x = 0"]);
}

#[test]
fn accepting_a_whole_file_makes_the_result_equal_to_that_side() {
    let mut d = sample(true);
    d.splice(0, 1, words("edited"));
    d.accept_file(Side::Left);
    assert_eq!(text(&d), "a L c d e");
    assert_eq!(d.unresolved(), (0, 0));
}

#[test]
fn typing_on_one_line_undoes_as_a_single_step() {
    let mut d = sample(true);
    d.begin(Pos::default());
    d.splice(0, 1, words("ax"));
    d.splice(0, 1, words("axy"));
    d.commit(Pos::default());
    d.undo();
    assert_eq!(text(&d), "a b c d E");
}

#[test]
fn ignoring_all_pending_sides_dismisses_a_non_conflicting_change() {
    let mut d = sample(false);
    d.ignore_pending(1).unwrap();
    assert_eq!(text(&d), "a b c d e");
    assert_eq!(d.unresolved(), (1, 0));
}

#[test]
fn ignoring_all_pending_sides_leaves_an_accepted_side_alone() {
    let mut d = sample(true);
    d.accept(0, Side::Left).unwrap();
    d.ignore_pending(0).unwrap();
    assert_eq!(text(&d), "a L c d E");
    assert_eq!(d.chunks[0].ls, State::Accepted);
    assert_eq!(d.chunks[0].rs, State::Ignored);
    assert!(d.ignore_pending(0).is_err());
}
