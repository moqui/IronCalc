#![allow(clippy::unwrap_used)]

//! The functions that read a range of numbers read it row by row, a row of
//! numbers at once and the constants of any other row straight from the
//! sheet. They see what reading cell by cell would have shown them.

use crate::test::util::new_empty_model;

#[test]
fn a_range_of_everything_a_sheet_can_hold() {
    let mut model = new_empty_model();
    // A row of numbers alone
    for column in ["A", "B", "C", "D"] {
        model._set(&format!("{column}1"), "2");
    }
    // A row of numbers with a gap, a text, a boolean and a formula
    model._set("A2", "1");
    model._set("C2", "text");
    model._set("D2", "TRUE");
    model._set("A3", "=A1*10");
    model._set("B3", "=\"5\"");
    model._set("C3", "3");
    // A dynamic array spilling into the range, from outside its rows and
    // columns
    model._set("F12", "=SEQUENCE(1,2,100)");
    model._set("A4", "=F12:G12");
    // Empty rows below, and a row past them
    model._set("A9", "0.5");

    let functions = [
        "SUM", "AVERAGE", "MAX", "MIN", "COUNT", "STDEV.S", "VAR.P", "MEDIAN", "SUMSQ", "DEVSQ",
    ];
    for (row, function) in functions.iter().enumerate() {
        let row = row + 20;
        model._set(&format!("H{row}"), &format!("={function}(A1:D9)"));
        model._set(&format!("I{row}"), &format!("={function}(A:D)"));
        model._set(&format!("J{row}"), &format!("={function}(1:9)"));
    }
    model.evaluate();

    let values: Vec<String> = (20..30)
        .map(|row| model._get_text(&format!("H{row}")))
        .collect();
    assert_eq!(
        values,
        [
            "233.5",
            "23.35",
            "101",
            "0.5",
            "10",
            "41.06231714",
            "1517.5025",
            "2",
            "20627.25",
            "15175.025"
        ]
    );
    for row in 20..30 {
        let in_range = model._get_text(&format!("H{row}"));
        assert_eq!(model._get_text(&format!("I{row}")), in_range, "row {row}");
        assert_eq!(model._get_text(&format!("J{row}")), in_range, "row {row}");
    }
}

#[test]
fn the_first_error_stops_the_read() {
    let mut model = new_empty_model();
    model._set("A1", "=SUM(B1:B3)");
    model._set("B1", "1");
    model._set("B2", "=1/0");
    // Read only if the sum went past the error: it would close a loop
    model._set("B3", "=A1");
    model._set("C1", "=MAX(D1:D3)");
    model._set("D1", "#N/A");
    model._set("D3", "=C1");
    model._set("E1", "=AVERAGE(F1:G2)");
    model._set("F1", "=NA()");
    model._set("G1", "=E1");
    // COUNT does not stop
    model._set("H1", "=COUNT(B1:B2,D1:D3)");
    model.evaluate();

    assert_eq!(model._get_text("A1"), *"#DIV/0!");
    assert_eq!(model._get_text("B3"), *"#DIV/0!");
    assert_eq!(model._get_text("C1"), *"#N/A");
    assert_eq!(model._get_text("E1"), *"#N/A");
    assert_eq!(model._get_text("H1"), *"1");
}
