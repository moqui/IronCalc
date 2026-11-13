#![allow(clippy::unwrap_used)]

//! The stored form of a workbook reads what was stored before it had a
//! version, and refuses what a later version stored.

use crate::Model;

/// Stored by the model before the stored form had a version: two sheets,
/// texts, numbers, formulas across them, a defined name, styles, a merge,
/// frozen rows, a column width and a row height.
const STORED_V0: &[u8] = include_bytes!("stored/workbook_v0.bin");

fn check(model: &mut Model) {
    model.evaluate();
    assert_eq!(model._get_text("Sheet1!A3"), *"Item 3");
    assert_eq!(model._get_text("Sheet1!B7"), *"200.00");
    assert_eq!(model._get_formula("Sheet1!B7"), *"=SUM(B2:B6)");
    assert_eq!(model._get_text("Sheet2!A1"), *"400");
    assert_eq!(model._get_text("Sheet2!A2"), *"200");
    assert_eq!(model._get_text("Sheet2!A3"), *"TRUE");
    let style = model.get_style_for_cell(0, 1, 1).unwrap();
    assert!(style.font.b);
    let worksheet = model.workbook.worksheet(0).unwrap();
    assert_eq!(worksheet.frozen_rows, 1);
    assert_eq!(worksheet.merged_cells.len(), 1);
    assert_eq!(model.get_row_height(0, 3).unwrap(), 30.0);
}

#[test]
fn a_workbook_stored_before_versions_is_read() {
    let mut model = Model::from_bytes(STORED_V0, "en").unwrap();
    check(&mut model);
    // Stored again, it has a version, and reads the same
    let bytes = model.to_bytes();
    assert!(bytes.starts_with(b"\0ICW"));
    assert_eq!(&bytes[4..6], &1u16.to_le_bytes());
    let mut again = Model::from_bytes(&bytes, "en").unwrap();
    check(&mut again);
    assert_eq!(again.to_bytes(), bytes);
}

#[test]
fn a_later_version_or_a_broken_form_is_refused() {
    let model = Model::from_bytes(STORED_V0, "en").unwrap();
    let mut bytes = model.to_bytes();
    bytes[4] = 9;
    let error = Model::from_bytes(&bytes, "en").err().unwrap();
    assert!(error.contains("later version"), "{error}");
    assert!(Model::from_bytes(b"\0ICW", "en").is_err());
    assert!(Model::from_bytes(b"\0ICW\x01\x00garbage", "en").is_err());
}
