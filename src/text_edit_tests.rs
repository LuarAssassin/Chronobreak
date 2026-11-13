use super::*;

fn assert_state(model: &TextEditModel, text: &str, anchor: usize, focus: usize) {
    assert_eq!(model.text(), text);
    assert_eq!(model.selection(), Selection { anchor, focus });
    assert_valid(model);
}

fn assert_valid(model: &TextEditModel) {
    for endpoint in [model.selection().anchor(), model.selection().focus()] {
        assert!(endpoint <= model.text().len());
        assert!(model.text().is_char_boundary(endpoint));
        assert!(
            endpoint == model.text().len()
                || model
                    .text()
                    .grapheme_indices(true)
                    .any(|(start, _)| start == endpoint)
        );
    }
    assert!(std::str::from_utf8(model.text().as_bytes()).is_ok());
}

#[test]
fn initial_state_and_selection_direction() {
    let mut model = TextEditModel::new("a中文");
    assert_state(&model, "a中文", 7, 7);
    assert_eq!(model.revision(), 0);
    model.select_all();
    assert_state(&model, "a中文", 0, 7);
    model.reverse_selection();
    assert!(model.selection().is_backward());
    assert_eq!(model.selection().range(), 0..7);
    assert_eq!(model.selected_text(), "a中文");
    model.reverse_selection();
    assert!(!model.selection().is_backward());
    assert_eq!(model.revision(), 0);
    assert_eq!(model.history_stats(), HistoryStats::default());
}

#[test]
fn invalid_offsets_are_rejected_atomically() {
    let mut model = TextEditModel::new("e\u{301}中👩🏽‍💻🇨🇳");
    let original = model.selection();
    for offset in 0..=model.text().len() + 1 {
        let valid = model.is_boundary(offset);
        let result = model.set_selection(0, offset);
        if valid {
            assert!(result.is_ok());
        } else {
            assert_eq!(result, Err(InvalidTextPosition { offset }));
            assert_eq!(model.selection(), original);
        }
        model.selection = original;
        assert_valid(&model);
    }
    assert_eq!(
        model.set_selection(usize::MAX, 0),
        Err(InvalidTextPosition { offset: usize::MAX })
    );
    assert_eq!(model.selection(), original);
    assert!(model.set_selection(0, 1).is_err());
    assert!(model.set_selection(0, 2).is_err());
}

#[test]
fn grapheme_movement_visits_exact_clusters_in_both_directions() {
    let clusters = ["a", "e\u{301}", "中", "👩🏽‍💻", "🇨🇳", "👍🏿", "\r\n", "ב"];
    let text = clusters.concat();
    let mut model = TextEditModel::new(text.clone());
    let mut offset = text.len();
    for cluster in clusters.iter().rev() {
        model.move_cursor(Direction::Backward, EditUnit::Grapheme, true);
        offset -= cluster.len();
        assert_state(&model, &text, text.len(), offset);
    }
    model.move_cursor(Direction::Backward, EditUnit::Grapheme, true);
    assert_state(&model, &text, text.len(), 0);
    model.set_selection(0, 0).unwrap();
    for cluster in clusters {
        model.move_cursor(Direction::Forward, EditUnit::Grapheme, false);
        offset += cluster.len();
        assert_state(&model, &text, offset, offset);
    }
    model.move_cursor(Direction::Forward, EditUnit::Grapheme, false);
    assert_state(&model, &text, text.len(), text.len());
}

#[test]
fn extension_crosses_anchor_and_collapse_does_not_step_again() {
    let mut model = TextEditModel::new("a中b");
    model.set_selection(1, 1).unwrap();
    model.move_cursor(Direction::Forward, EditUnit::Grapheme, true);
    assert_state(&model, "a中b", 1, 4);
    model.move_cursor(Direction::Backward, EditUnit::Grapheme, true);
    model.move_cursor(Direction::Backward, EditUnit::Grapheme, true);
    assert_state(&model, "a中b", 1, 0);
    model.move_cursor(Direction::Forward, EditUnit::Word, false);
    assert_state(&model, "a中b", 1, 1);
    model.set_selection(4, 1).unwrap();
    model.move_cursor(Direction::Backward, EditUnit::Grapheme, false);
    assert_state(&model, "a中b", 1, 1);
}

#[test]
fn insert_replace_and_directed_selection_undo_trace() {
    let mut model = TextEditModel::new("a中👩🏽‍💻z");
    model.set_selection("a中👩🏽‍💻".len(), 1).unwrap();
    let before = model.selection();
    assert!(model.replace_selection("e\u{301}"));
    assert_state(&model, "ae\u{301}z", 4, 4);
    assert_eq!(model.revision(), 1);
    assert!(model.undo());
    assert_eq!(model.selection(), before);
    assert_eq!(model.text(), "a中👩🏽‍💻z");
    assert_eq!(model.revision(), 2);
    assert!(model.redo());
    assert_state(&model, "ae\u{301}z", 4, 4);
    assert_eq!(model.revision(), 3);
    assert!(!model.redo());
}

#[test]
fn grapheme_deletion_restores_original_caret_and_clusters() {
    for direction in [Direction::Backward, Direction::Forward] {
        let text = "e\u{301}中👩🏽‍💻🇨🇳👍🏿";
        let mut model = TextEditModel::new(text);
        if direction == Direction::Forward {
            model.set_selection(0, 0).unwrap();
        }
        let mut remaining = text.to_string();
        for _ in 0..5 {
            let original_selection = model.selection();
            let original_text = model.text().to_string();
            assert!(model.delete(direction, EditUnit::Grapheme));
            let range = match direction {
                Direction::Forward => 0..remaining.graphemes(true).next().unwrap().len(),
                Direction::Backward => {
                    remaining.grapheme_indices(true).next_back().unwrap().0..remaining.len()
                }
            };
            remaining.replace_range(range, "");
            assert_eq!(model.text(), remaining);
            assert_valid(&model);
            assert!(model.undo());
            assert_eq!(model.text(), original_text);
            assert_eq!(model.selection(), original_selection);
            assert!(model.redo());
        }
        let revision = model.revision();
        assert!(!model.delete(direction, EditUnit::Grapheme));
        assert_eq!(model.revision(), revision);
        assert_state(&model, "", 0, 0);
    }
}

#[test]
fn deletion_uses_selection_before_direction_or_unit() {
    let mut model = TextEditModel::new("a中b");
    model.set_selection(4, 1).unwrap();
    assert!(model.delete(Direction::Forward, EditUnit::Word));
    assert_state(&model, "ab", 1, 1);
    assert!(model.undo());
    assert_state(&model, "a中b", 4, 1);
}

#[test]
fn word_policy_has_exact_whitespace_punctuation_and_unicode_targets() {
    let text = "hi,  世界!\tcan't 42\r\n";
    let mut model = TextEditModel::new(text);
    model.set_selection(0, 0).unwrap();
    for target in [2, 3, 8, 11, 12, 18, 21, 23] {
        model.move_cursor(Direction::Forward, EditUnit::Word, false);
        assert_state(&model, text, target, target);
    }
    for target in [19, 13, 11, 8, 5, 2, 0] {
        model.move_cursor(Direction::Backward, EditUnit::Word, false);
        assert_state(&model, text, target, target);
    }
    model.set_selection(1, 1).unwrap();
    model.move_cursor(Direction::Forward, EditUnit::Word, false);
    assert_state(&model, text, 2, 2);
    model.set_selection(17, 17).unwrap();
    model.move_cursor(Direction::Backward, EditUnit::Word, false);
    assert_state(&model, text, 13, 13);
}

#[test]
fn word_deletion_has_exact_string_and_undo_selection() {
    let mut model = TextEditModel::new("hi,  世界!\tcan't 42\r\n");
    assert!(model.delete(Direction::Backward, EditUnit::Word));
    assert_state(&model, "hi,  世界!\tcan't ", 19, 19);
    assert!(model.undo());
    assert_state(&model, "hi,  世界!\tcan't 42\r\n", 23, 23);
    model.set_selection(3, 3).unwrap();
    assert!(model.delete(Direction::Forward, EditUnit::Word));
    assert_state(&model, "hi,界!\tcan't 42\r\n", 3, 3);
    assert!(model.undo());
    assert_state(&model, "hi,  世界!\tcan't 42\r\n", 3, 3);
}

#[test]
fn whitespace_only_word_movement_and_deletion_reach_edges() {
    let mut model = TextEditModel::new(" \t\r\n\u{2003}");
    model.move_cursor(Direction::Backward, EditUnit::Word, false);
    assert_eq!(model.selection().focus(), 0);
    model.move_cursor(Direction::Forward, EditUnit::Word, false);
    assert_eq!(model.selection().focus(), model.text().len());
    assert!(model.delete(Direction::Backward, EditUnit::Word));
    assert_state(&model, "", 0, 0);
}

#[test]
fn word_steps_keep_emoji_zwj_flags_and_skin_modifiers_intact() {
    let text = "👩🏽‍💻, 👍🏿 🇨🇳";
    let mut model = TextEditModel::new(text);
    model.set_selection(0, 0).unwrap();
    for target in [15, 16, 25, 34] {
        model.move_cursor(Direction::Forward, EditUnit::Word, false);
        assert_state(&model, text, target, target);
    }
    for target in [26, 17, 15, 0] {
        model.move_cursor(Direction::Backward, EditUnit::Word, false);
        assert_state(&model, text, target, target);
    }
    assert!(model.delete(Direction::Forward, EditUnit::Word));
    assert_state(&model, ", 👍🏿 🇨🇳", 0, 0);
    assert!(model.undo());
    assert_state(&model, text, 0, 0);
}

#[test]
fn unicode_boundary_insertions_resegment_and_roundtrip_exactly() {
    let samples = ["", "e\u{301}中", "👩🏽‍💻👍🏿", "🇨🇳🇺🇸🇦", "\r\nב", "क्‍ष"];
    let insertions = ["\u{301}", "\u{200d}", "🏿", "🇨", "👩🏽‍💻", "\r", "中"];
    for text in samples {
        let boundaries = text
            .grapheme_indices(true)
            .map(|(start, _)| start)
            .chain(std::iter::once(text.len()));
        for offset in boundaries {
            for inserted in insertions {
                let mut model = TextEditModel::new(text);
                model.set_selection(offset, offset).unwrap();
                let mut expected = text.to_string();
                expected.insert_str(offset, inserted);
                assert!(model.replace_selection(inserted));
                assert_eq!(model.text(), expected);
                assert_valid(&model);
                let after = model.selection();
                assert!(model.undo());
                assert_state(&model, text, offset, offset);
                assert!(model.redo());
                assert_eq!(model.text(), expected);
                assert_eq!(model.selection(), after);
                assert_valid(&model);
            }
        }
    }
}

#[test]
fn insertion_resegments_combining_marks_zwj_and_regional_indicators() {
    for (initial, caret, inserted, expected, expected_caret) in [
        ("e", 1, "\u{301}", "e\u{301}", 3),
        ("👩💻", 4, "\u{200d}", "👩‍💻", 11),
        ("🇨", 0, "🇳", "🇳🇨", 8),
        ("👍", 4, "🏿", "👍🏿", 8),
        ("\n", 0, "\r", "\r\n", 2),
    ] {
        let mut model = TextEditModel::new(initial);
        model.set_selection(caret, caret).unwrap();
        assert!(model.replace_selection(inserted));
        assert_state(&model, expected, expected_caret, expected_caret);
        assert!(model.undo());
        assert_state(&model, initial, caret, caret);
        assert!(model.redo());
        assert_state(&model, expected, expected_caret, expected_caret);
    }
}

#[test]
fn deletion_resegments_newly_adjacent_clusters() {
    let mut model = TextEditModel::new("🇨x🇳");
    model.set_selection(4, 5).unwrap();
    model.replace_selection("");
    assert_state(&model, "🇨🇳", 8, 8);
    assert!(model.undo());
    assert_state(&model, "🇨x🇳", 4, 5);
}

#[test]
fn nested_groups_coalesce_and_capture_first_edit_selection() {
    let mut model = TextEditModel::new("a");
    let output = model.edit_group(|model| {
        model.set_selection(0, 0).unwrap();
        model.replace_selection("中");
        model.edit_group(|model| {
            model.replace_selection("e\u{301}");
            assert!(!model.undo());
            assert!(!model.redo());
        });
        model.set_selection(6, 0).unwrap();
        42
    });
    assert_eq!(output, 42);
    assert_state(&model, "中e\u{301}a", 6, 0);
    assert_eq!(model.history_stats().undo_entries, 1);
    assert!(model.undo());
    assert_state(&model, "a", 0, 0);
    assert!(model.redo());
    assert_state(&model, "中e\u{301}a", 6, 0);
}

#[test]
fn grouped_deletion_records_original_caret() {
    let mut model = TextEditModel::new("a中");
    model.edit_group(|model| {
        model.delete(Direction::Backward, EditUnit::Grapheme);
        model.delete(Direction::Backward, EditUnit::Grapheme);
    });
    assert_state(&model, "", 0, 0);
    assert!(model.undo());
    assert_state(&model, "a中", 4, 4);
}

#[test]
fn distinct_calls_do_not_implicitly_coalesce() {
    let mut model = TextEditModel::new("");
    model.replace_selection("a");
    model.replace_selection("b");
    assert_eq!(model.history_stats().undo_entries, 2);
    assert!(model.undo());
    assert_state(&model, "a", 1, 1);
    assert!(model.undo());
    assert_state(&model, "", 0, 0);
}

#[test]
fn no_op_edits_and_net_zero_groups_preserve_redo() {
    let mut model = TextEditModel::new("a");
    model.replace_selection("b");
    model.undo();
    let history = model.history_stats();
    let revision = model.revision();
    model.edit_group(|_| {});
    assert!(!model.replace_selection(""));
    model.select_all();
    assert!(!model.replace_selection("a"));
    assert!(!model.replace_programmatic("a"));
    assert!(!model.delete(Direction::Forward, EditUnit::Grapheme));
    assert_eq!(model.revision(), revision);
    model.edit_group(|model| {
        model.replace_selection("x");
        model.delete(Direction::Backward, EditUnit::Grapheme);
    });
    assert_eq!(model.history_stats(), history);
    assert_eq!(model.revision(), revision + 2);
    assert!(model.redo());
    assert_state(&model, "ab", 2, 2);
}

#[test]
fn branch_after_undo_clears_redo_only_on_text_change() {
    let mut model = TextEditModel::new("");
    model.replace_selection("a");
    model.replace_selection("b");
    model.undo();
    model.move_cursor(Direction::Backward, EditUnit::Grapheme, false);
    assert!(model.can_redo());
    model.replace_selection("中");
    assert_state(&model, "中a", 3, 3);
    assert!(!model.can_redo());
    assert!(!model.redo());
    model.undo();
    assert_state(&model, "a", 0, 0);
}

#[test]
fn history_count_budget_evicts_oldest_and_is_shared_with_redo() {
    let mut model = TextEditModel::with_history_limits(
        "",
        HistoryLimits {
            max_entries: 2,
            max_bytes: 100,
        },
    );
    for inserted in ["a", "b", "c"] {
        model.replace_selection(inserted);
    }
    assert_eq!(
        model.history_stats(),
        HistoryStats {
            undo_entries: 2,
            redo_entries: 0,
            text_bytes: 8
        }
    );
    assert!(model.undo());
    assert!(model.undo());
    assert!(!model.undo());
    assert_state(&model, "a", 1, 1);
    assert_eq!(
        model.history_stats(),
        HistoryStats {
            undo_entries: 0,
            redo_entries: 2,
            text_bytes: 8
        }
    );
    assert!(model.redo());
    assert!(model.redo());
    assert_state(&model, "abc", 3, 3);
}

#[test]
fn history_byte_budget_and_oversized_steps_do_not_retain_unbounded_text() {
    let mut model = TextEditModel::with_history_limits(
        "",
        HistoryLimits {
            max_entries: 10,
            max_bytes: 10,
        },
    );
    model.replace_selection("中");
    assert_eq!(model.history_stats().text_bytes, 3);
    model.replace_selection("文");
    assert_eq!(
        model.history_stats(),
        HistoryStats {
            undo_entries: 1,
            redo_entries: 0,
            text_bytes: 9
        }
    );
    model.replace_selection("👩🏽‍💻");
    assert_eq!(model.history_stats(), HistoryStats::default());
    assert!(!model.undo());
    assert_eq!(model.text(), "中文👩🏽‍💻");
}

#[test]
fn zero_history_budgets_disable_retention_including_groups() {
    for limits in [
        HistoryLimits {
            max_entries: 0,
            max_bytes: 100,
        },
        HistoryLimits {
            max_entries: 100,
            max_bytes: 0,
        },
    ] {
        let mut model = TextEditModel::with_history_limits("", limits);
        model.edit_group(|model| {
            model.replace_selection("中");
            model.replace_selection("文");
        });
        assert_eq!(model.history_stats(), HistoryStats::default());
        assert!(!model.undo());
        assert_state(&model, "中文", 6, 6);
    }
}

#[test]
fn programmatic_equal_echo_preserves_history_selection_and_revision() {
    let mut model = TextEditModel::new("中");
    model.replace_selection("a");
    model.undo();
    model.select_all();
    model.reverse_selection();
    let history = model.history_stats();
    let revision = model.revision();
    assert!(!model.replace_programmatic("中"));
    assert_state(&model, "中", 3, 0);
    assert_eq!(model.history_stats(), history);
    assert_eq!(model.revision(), revision);
    assert!(model.redo());
}

#[test]
fn programmatic_replace_clamps_invalidated_offsets_and_resets_history() {
    let mut model = TextEditModel::new("abcdefghij");
    model.replace_selection("k");
    model.undo();
    model.set_selection(9, 2).unwrap();
    assert!(model.replace_programmatic("e\u{301}中"));
    assert_state(&model, "e\u{301}中", 6, 0);
    assert_eq!(model.revision(), 3);
    assert_eq!(model.history_stats(), HistoryStats::default());
    assert!(!model.undo());
    assert!(!model.redo());
    assert!(model.replace_programmatic(""));
    assert_state(&model, "", 0, 0);
}

#[test]
fn programmatic_replacement_is_a_barrier_inside_nested_group() {
    let mut model = TextEditModel::new("a");
    model.edit_group(|model| {
        model.replace_selection("b");
        model.edit_group(|model| {
            model.replace_programmatic("中");
            model.replace_selection("x");
        });
    });
    assert_state(&model, "x中", 1, 1);
    assert_eq!(model.history_stats().undo_entries, 1);
    assert!(model.undo());
    assert_state(&model, "中", 0, 0);
    assert!(!model.undo());
}

#[test]
fn group_unwind_retains_edits_and_restores_grouping_state() {
    let mut model = TextEditModel::new("a");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        model.edit_group(|model| {
            model.replace_selection("中");
            model.edit_group(|model| {
                model.replace_selection("文");
                panic!("fixture");
            });
        });
    }));
    assert!(result.is_err());
    assert_state(&model, "a中文", 7, 7);
    assert_eq!(model.history_stats().undo_entries, 1);
    assert!(model.undo());
    assert_state(&model, "a", 1, 1);
    model.replace_selection("b");
    assert_state(&model, "ab", 2, 2);
}

#[derive(Clone, Copy)]
enum Operation {
    Move(Direction, EditUnit, bool),
    Insert(&'static str),
    Delete(Direction, EditUnit),
    SelectAll,
    Reverse,
    Undo,
    Redo,
    External(&'static str),
}

fn replay(model: &mut TextEditModel, operation: Operation) {
    match operation {
        Operation::Move(direction, unit, extend) => model.move_cursor(direction, unit, extend),
        Operation::Insert(text) => {
            model.replace_selection(text);
        }
        Operation::Delete(direction, unit) => {
            model.delete(direction, unit);
        }
        Operation::SelectAll => model.select_all(),
        Operation::Reverse => model.reverse_selection(),
        Operation::Undo => {
            model.undo();
        }
        Operation::Redo => {
            model.redo();
        }
        Operation::External(text) => {
            model.replace_programmatic(text);
        }
    }
}

#[test]
fn deterministic_operation_replay_has_exact_trace() {
    let operations = [
        Operation::Move(Direction::Backward, EditUnit::Grapheme, true),
        Operation::Insert("🇨🇳"),
        Operation::Delete(Direction::Backward, EditUnit::Word),
        Operation::Undo,
        Operation::Undo,
        Operation::Redo,
    ];
    let expected = [
        ("a e\u{301} 👩🏽‍💻", 21, 6, 0),
        ("a e\u{301} 🇨🇳", 14, 14, 1),
        ("a e\u{301} ", 6, 6, 2),
        ("a e\u{301} 🇨🇳", 14, 14, 3),
        ("a e\u{301} 👩🏽‍💻", 21, 6, 4),
        ("a e\u{301} 🇨🇳", 14, 14, 5),
    ];
    for _ in 0..2 {
        let mut model = TextEditModel::new("a e\u{301} 👩🏽‍💻");
        for (operation, (text, anchor, focus, revision)) in operations.into_iter().zip(expected) {
            replay(&mut model, operation);
            assert_state(&model, text, anchor, focus);
            assert_eq!(model.revision(), revision);
        }
    }
}

#[test]
fn unicode_operation_loops_keep_utf8_graphemes_and_history_budgets_valid() {
    let samples = [
        "",
        "a中ב",
        "e\u{301}\u{308}",
        "👨‍👩‍👧‍👦",
        "🇨🇳🇺🇸🇦",
        "👍🏿👩🏽‍💻",
        "\r\n\t\u{2003}",
        "क्‍ष",
    ];
    let operations = [
        Operation::Move(Direction::Backward, EditUnit::Grapheme, true),
        Operation::Move(Direction::Forward, EditUnit::Word, true),
        Operation::Move(Direction::Backward, EditUnit::Word, false),
        Operation::Move(Direction::Forward, EditUnit::Grapheme, false),
        Operation::Insert("\u{301}"),
        Operation::Insert("👩🏽‍💻"),
        Operation::Insert("🇨🇳"),
        Operation::Insert("\u{200d}"),
        Operation::Delete(Direction::Backward, EditUnit::Grapheme),
        Operation::Delete(Direction::Forward, EditUnit::Grapheme),
        Operation::Delete(Direction::Backward, EditUnit::Word),
        Operation::Delete(Direction::Forward, EditUnit::Word),
        Operation::SelectAll,
        Operation::Reverse,
        Operation::Undo,
        Operation::Redo,
        Operation::External("e\u{301}中🇨🇳"),
    ];
    for (seed, text) in samples.into_iter().enumerate() {
        let limits = HistoryLimits {
            max_entries: 7,
            max_bytes: 256,
        };
        let mut model = TextEditModel::with_history_limits(text, limits);
        let mut state = seed as u64 + 1;
        for _ in 0..512 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let operation = operations[(state >> 32) as usize % operations.len()];
            replay(&mut model, operation);
            assert_valid(&model);
            let history = model.history_stats();
            assert!(history.undo_entries + history.redo_entries <= limits.max_entries);
            assert!(history.text_bytes <= limits.max_bytes);
        }
    }
}
