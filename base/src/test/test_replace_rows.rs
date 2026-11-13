#![allow(clippy::unwrap_used)]

//! Loading the rows of a table again (`Model::replace_rows`).

use crate::test::util::new_empty_model;
use crate::types::Cell;
use crate::{Model, UserModel};

fn number(v: f64) -> Cell {
    Cell::NumberCell { v, s: 0 }
}

/// Rows of one number in column A and one text in column B.
fn rows(model: &mut Model, values: &[f64]) -> Vec<Vec<(i32, Cell)>> {
    values
        .iter()
        .map(|v| {
            let si = model.intern_shared_string(&format!("item {v}"));
            vec![(1, number(*v)), (2, Cell::SharedString { si, s: 0 })]
        })
        .collect()
}

#[test]
fn ranges_over_the_rows_follow_them_and_what_is_below_moves() {
    let mut model = new_empty_model();
    model._set("A1", "amount");
    let data = rows(&mut model, &[1.0, 2.0, 3.0]);
    model.replace_rows(0, 2, 0, data).unwrap();
    // Below the table, and over it
    model._set("A6", "=SUM(A2:A4)");
    model._set("A7", "=COUNTA(B2:B4)");
    model._set("A8", "=A2");
    model.evaluate();
    assert_eq!(model._get_text("A6"), *"6");
    assert_eq!(model._get_text("B3"), *"item 2");

    // More rows: the ranges stretch, the formulas move down
    let data = rows(&mut model, &[10.0, 20.0, 30.0, 40.0, 50.0]);
    model.replace_rows(0, 2, 3, data).unwrap();
    model.evaluate();
    assert_eq!(model._get_formula("A8"), *"=SUM(A2:A6)");
    assert_eq!(model._get_text("A8"), *"150");
    assert_eq!(model._get_text("A9"), *"5");
    assert_eq!(model._get_text("A10"), *"10");

    // Fewer: they shrink and move up
    let data = rows(&mut model, &[7.0, 8.0]);
    model.replace_rows(0, 2, 5, data).unwrap();
    model.evaluate();
    assert_eq!(model._get_formula("A5"), *"=SUM(A2:A3)");
    assert_eq!(model._get_text("A5"), *"15");
    assert_eq!(model._get_text("A6"), *"2");

    // One row: two are kept, the second empty, so the ranges can grow again
    let data = rows(&mut model, &[4.0]);
    model.replace_rows(0, 2, 2, data).unwrap();
    model.evaluate();
    assert_eq!(model._get_formula("A5"), *"=SUM(A2:A3)");
    assert_eq!(model._get_text("A5"), *"4");
    assert_eq!(model._get_text("A3"), *"");
    let data = rows(&mut model, &[1.0, 1.0, 1.0, 1.0]);
    model.replace_rows(0, 2, 2, data).unwrap();
    model.evaluate();
    assert_eq!(model._get_formula("A7"), *"=SUM(A2:A5)");
    assert_eq!(model._get_text("A7"), *"4");
}

#[test]
fn the_same_number_of_rows_evaluates_only_what_reads_them() {
    let mut model = new_empty_model();
    let data = rows(&mut model, &[1.0, 2.0, 3.0]);
    model.replace_rows(0, 2, 0, data).unwrap();
    // The rows are replaced whole: the formulas are above them
    model._set("D1", "=SUM(A2:A4)");
    model._set("E1", "=F1*2");
    model.evaluate();

    let data = rows(&mut model, &[5.0, 5.0, 5.0]);
    model.replace_rows(0, 2, 3, data).unwrap();
    let changes = model.evaluate_changes();
    assert!(!changes.full);
    assert_eq!(changes.formulas_run, 1);
    assert_eq!(model._get_text("D1"), *"15");

    // A different number moves rows: everything
    let data = rows(&mut model, &[5.0, 5.0, 5.0, 5.0]);
    model.replace_rows(0, 2, 3, data).unwrap();
    assert!(model.evaluate_changes().full);
    assert_eq!(model._get_text("D1"), *"20");
}

#[test]
fn rows_outside_the_sheet_and_unknown_sheets_are_refused() {
    let mut model = new_empty_model();
    assert!(model.replace_rows(3, 1, 0, Vec::new()).is_err());
    assert!(model.replace_rows(0, 0, 0, Vec::new()).is_err());
    assert!(model
        .replace_rows(0, crate::constants::LAST_ROW, 0, vec![vec![], vec![]])
        .is_err());
}

#[test]
fn a_user_model_forgets_its_history_and_evaluates() {
    let mut model = new_empty_model();
    let data = rows(&mut model, &[1.0, 1.0]);
    model.replace_rows(0, 2, 0, data).unwrap();
    let mut model = UserModel::from_model(model);
    model.set_user_input(0, 1, 3, "=SUM(A2:A3)").unwrap();
    assert_eq!(model.get_formatted_cell_value(0, 1, 3).unwrap(), "2");
    assert!(model.can_undo());
    let a = model.intern_shared_string("a");
    model
        .replace_rows(
            0,
            2,
            2,
            vec![
                vec![(1, number(2.0)), (2, Cell::SharedString { si: a, s: 0 })],
                vec![(1, number(3.0))],
            ],
        )
        .unwrap();
    assert!(!model.can_undo());
    assert_eq!(model.get_formatted_cell_value(0, 1, 3).unwrap(), "5");
    assert_eq!(model.get_formatted_cell_value(0, 2, 2).unwrap(), "a");
    assert!(!model.last_changes().full);
}
