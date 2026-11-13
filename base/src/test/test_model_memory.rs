#![allow(clippy::unwrap_used)]

use crate::test::util::new_empty_model;

#[test]
fn memory_usage_counts_the_cells_of_each_sheet_and_the_texts() {
    let mut model = new_empty_model();
    let empty = model.memory_usage();
    assert_eq!(empty.sheets.len(), 1);
    model.add_sheet("Data").unwrap();
    for row in 1..=50 {
        model._set(&format!("Data!A{row}"), &format!("Customer {row}"));
        model._set(&format!("Data!B{row}"), "12.5");
    }
    model._set("A1", "=SUM(Data!B1:B50)");
    model.evaluate();
    let usage = model.memory_usage();
    assert_eq!(usage.sheets.len(), 2);
    assert!(usage.sheets[1] > usage.sheets[0]);
    // the 50 texts, each held once
    assert!(usage.shared_strings >= 50 * "Customer 10".len());
    assert_eq!(
        usage.total(),
        usage.sheets.iter().sum::<usize>() + usage.shared_strings
    );
}
