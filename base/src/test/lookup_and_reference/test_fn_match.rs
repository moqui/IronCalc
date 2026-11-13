#![allow(clippy::unwrap_used)]

use crate::test::util::new_empty_model;

#[test]
fn test_match_array_row_vector() {
    let mut model = new_empty_model();
    model._set("A1", "=MATCH(69,{24,43.5,52.8,69,269,387,770},0)");
    model.evaluate();
    assert_eq!(model._get_text("A1"), "4");
}

#[test]
fn test_match_array_column_vector() {
    let mut model = new_empty_model();
    model._set("A1", "=MATCH(52.8,{24;43.5;52.8;69;269;387;770},0)");
    model.evaluate();
    assert_eq!(model._get_text("A1"), "3");
}

#[test]
fn test_match_array_exact_string() {
    let mut model = new_empty_model();
    model._set("A1", r#"=MATCH("banana",{"apple","banana","cherry"},0)"#);
    model.evaluate();
    assert_eq!(model._get_text("A1"), "2");
}

#[test]
fn test_match_array_not_found() {
    let mut model = new_empty_model();
    model._set("A1", "=MATCH(100,{24,43.5,52.8,69},0)");
    model.evaluate();
    assert_eq!(model._get_text("A1"), "#N/A");
}

#[test]
fn test_match_array_ascending_default() {
    let mut model = new_empty_model();
    // match_type 1 (default): largest value <= target in an ascending array
    model._set("A1", "=MATCH(70,{24,43.5,52.8,69,269,387,770})");
    model.evaluate();
    assert_eq!(model._get_text("A1"), "4");
}

#[test]
fn test_match_array_descending() {
    let mut model = new_empty_model();
    // match_type -1: smallest value >= target in a descending array.
    // Values >= 70 are 770, 387, 269; the smallest of those is 269 at position 3.
    model._set("A1", "=MATCH(70,{770,387,269,69,52.8,43.5,24},-1)");
    model.evaluate();
    assert_eq!(model._get_text("A1"), "3");
}

#[test]
fn test_match_array_wildcard() {
    let mut model = new_empty_model();
    model._set("A1", r#"=MATCH("ban*",{"apple","banana","cherry"},0)"#);
    model.evaluate();
    assert_eq!(model._get_text("A1"), "2");
}

#[test]
fn test_match_array_not_a_vector() {
    let mut model = new_empty_model();
    model._set("A1", "=MATCH(1,{1,2;3,4},0)");
    model.evaluate();
    assert_eq!(model._get_text("A1"), "#ERROR!");
}

#[test]
fn numbers_are_compared_to_15_digits_however_far_apart() {
    let mut model = new_empty_model();
    model._set("A1", "=0.1+0.2");
    model._set("A2", "0.5");
    model._set("A3", "1000000");
    model._set("A4", "1000001");
    model._set("A5", "=1E-20");
    model._set("A6", "1E+300");
    // Equal to 15 digits is equal, though the numbers differ
    model._set("B1", "=MATCH(0.3,A1:A6,0)");
    model._set("B2", "=MATCH(0.30000000000000004,A1:A6,0)");
    // Far apart, large or small
    model._set("B3", "=MATCH(0.5,A1:A6,0)");
    model._set("B4", "=MATCH(1E+300,A1:A6,0)");
    model._set("B5", "=MATCH(0.3000001,A1:A6,0)");
    model._set("B6", "=MATCH(-1E+300,A1:A6,0)");
    // Sorted ascending: the last one not larger
    model._set("B7", "=MATCH(999999,A1:A6,1)");
    model._set("B8", "=MATCH(2E+300,A1:A6,1)");
    model.evaluate();

    assert_eq!(model._get_text("B1"), *"1");
    assert_eq!(model._get_text("B2"), *"1");
    assert_eq!(model._get_text("B3"), *"2");
    assert_eq!(model._get_text("B4"), *"6");
    assert_eq!(model._get_text("B5"), *"#N/A");
    assert_eq!(model._get_text("B6"), *"#N/A");
    assert_eq!(model._get_text("B7"), *"2");
    assert_eq!(model._get_text("B8"), *"6");
}
