#![allow(clippy::unwrap_used)]

//! The shared strings: found without a second copy of each text, and
//! dropped when no cell shows them.

use crate::shared_strings::SharedStringIndex;
use crate::test::util::new_empty_model;
use crate::types::Cell;
use crate::UserModel;

#[test]
fn the_index_finds_each_text_and_the_first_of_two_equal_ones() {
    let strings: Vec<String> = ["a", "b", "", "a", "long text with spaces"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut index = SharedStringIndex::of(&strings);
    assert_eq!(index.get(&strings, "a"), Some(0));
    assert_eq!(index.get(&strings, "b"), Some(1));
    assert_eq!(index.get(&strings, ""), Some(2));
    assert_eq!(index.get(&strings, "long text with spaces"), Some(4));
    assert_eq!(index.get(&strings, "A"), None);
    assert_eq!(index.get(&strings, "c"), None);
    // Two places for one hash: the one that holds the text is found
    let mut more = strings.clone();
    more.push("c".to_string());
    index.insert("c", 5);
    index.insert("c", 1);
    assert_eq!(index.get(&more, "c"), Some(5));
    let many: Vec<String> = (0..10_000).map(|i| format!("text {i}")).collect();
    let index = SharedStringIndex::of(&many);
    assert!((0..10_000).all(|i| index.get(&many, &format!("text {i}")) == Some(i)));
}

#[test]
fn texts_no_cell_shows_are_dropped_and_the_cells_show_the_same() {
    let mut model = new_empty_model();
    for row in 1..=30 {
        model._set(&format!("A{row}"), &format!("name {}", row % 10));
        model._set(&format!("B{row}"), &format!("note {row}"));
    }
    // A row of constants, as data is loaded
    let t = model.intern_shared_string("loaded");
    model
        .replace_rows(
            0,
            40,
            0,
            vec![vec![(1, Cell::SharedString { si: t, s: 0 })]],
        )
        .unwrap();
    model._set("C1", "=COUNTIF(A1:A30,\"name 3\")");
    model.evaluate();
    let before: Vec<String> = (1..=40)
        .flat_map(|row| {
            [
                model._get_text(&format!("A{row}")),
                model._get_text(&format!("B{row}")),
            ]
        })
        .collect();
    assert_eq!(model.workbook.shared_strings.len(), 41);

    // Nothing to drop
    assert_eq!(model.compact_shared_strings(), 0);
    // The notes go
    for row in 1..=30 {
        model._set(&format!("B{row}"), "");
    }
    for row in 1..=30 {
        model
            .workbook
            .worksheet_mut(0)
            .unwrap()
            .sheet_data
            .remove_cell(row, 2);
    }
    let usage = model.memory_usage().shared_strings;
    assert_eq!(model.compact_shared_strings(), 30);
    assert_eq!(model.workbook.shared_strings.len(), 11);
    assert!(model.memory_usage().shared_strings < usage);
    let after: Vec<String> = (1..=40)
        .flat_map(|row| {
            [
                model._get_text(&format!("A{row}")),
                model._get_text(&format!("B{row}")),
            ]
        })
        .collect();
    let expected: Vec<String> = before
        .iter()
        .enumerate()
        .map(|(i, text)| {
            if i % 2 == 1 && i < 60 {
                String::new()
            } else {
                text.clone()
            }
        })
        .collect();
    assert_eq!(after, expected);
    assert_eq!(model._get_text("A40"), *"loaded");

    // Typing a text finds it at its new place, and formulas read the same
    model._set("D1", "name 3");
    let si = |model: &crate::Model, reference: &str| match model._get_cell(reference) {
        Cell::SharedString { si, .. } => si,
        _ => -1,
    };
    assert_eq!(si(&model, "D1"), si(&model, "A3"));
    model.evaluate();
    assert_eq!(model._get_text("C1"), *"3");
    // And it survives saving
    let read = crate::Model::from_bytes(&model.to_bytes(), "en").unwrap();
    assert_eq!(read._get_text("A13"), *"name 3");
}

#[test]
fn a_user_model_forgets_its_history_when_texts_are_dropped() {
    let mut model = UserModel::from_model(new_empty_model());
    model.set_user_input(0, 1, 1, "old").unwrap();
    model.set_user_input(0, 1, 1, "new").unwrap();
    assert!(model.can_undo());
    assert_eq!(model.compact_shared_strings(), 1);
    assert!(!model.can_undo());
    assert_eq!(model.get_formatted_cell_value(0, 1, 1).unwrap(), "new");
}
