#![allow(clippy::unwrap_used)]

//! Evaluating only what changed (`Model::evaluate_changes`): which formulas
//! run again, what is reported changed, when it falls back to a full
//! evaluation, and that it always ends where a full evaluation would.

use crate::expressions::types::Area;
use crate::test::util::new_empty_model;
use crate::{DefinedNameEntry, EvaluationChanges, Model};

fn cell(sheet: u32, row: i32, column: i32) -> Area {
    Area {
        sheet,
        row,
        column,
        width: 1,
        height: 1,
    }
}

/// Sets a cell and evaluates what changed.
fn edit(model: &mut Model, reference: &str, value: &str) -> EvaluationChanges {
    model._set(reference, value);
    model.evaluate_changes()
}

#[test]
fn the_first_evaluation_is_full_and_an_edit_nothing_reads_runs_nothing() {
    let mut model = new_empty_model();
    model._set("A1", "1");
    model._set("B1", "=A1*2");
    let first = model.evaluate_changes();
    assert!(first.full);
    assert_eq!(first.formulas_run, 1);

    let changes = edit(&mut model, "C5", "7");
    assert!(!changes.full);
    assert_eq!(changes.formulas_run, 0);
    assert_eq!(changes.areas, vec![cell(0, 5, 3)]);
    assert_eq!(model._get_text("B1"), *"2");
}

#[test]
fn a_chain_runs_from_the_edit_down_and_reports_what_changed() {
    let mut model = new_empty_model();
    model._set("A1", "1");
    for row in 2..=20 {
        model._set(&format!("A{row}"), &format!("=A{}+1", row - 1));
    }
    model._set("B1", "=5");
    model.evaluate();

    let changes = edit(&mut model, "A1", "100");
    assert!(!changes.full);
    assert_eq!(changes.formulas_run, 19);
    assert_eq!(model._get_text("A20"), *"119");
    let changes = edit(&mut model, "A15", "0");
    assert!(!changes.full);
    assert_eq!(changes.formulas_run, 5);
    assert_eq!(model._get_text("A20"), *"5");
    assert_eq!(model._get_text("A14"), *"113");
    let changes = edit(&mut model, "A18", "100");
    assert_eq!(changes.formulas_run, 2);
    assert_eq!(model._get_text("A20"), *"102");
    let mut expected = vec![cell(0, 18, 1)];
    expected.extend((19..=20).map(|row| cell(0, row, 1)));
    assert_eq!(changes.areas, expected);
}

#[test]
fn when_most_of_many_formulas_run_all_of_them_do_at_once() {
    let mut model = new_empty_model();
    model._set("A1", "1");
    for row in 2..=3000 {
        model._set(&format!("A{row}"), &format!("=A{}+1", row - 1));
    }
    model.evaluate();
    // 2,999 of 2,999: a full evaluation
    let changes = edit(&mut model, "A1", "2");
    assert!(changes.full);
    assert_eq!(model._get_text("A3000"), *"3001");
    // 1,500 of 2,999: just over half, but the half below runs alone
    let changes = edit(&mut model, "A1500", "0");
    assert!(changes.full);
    let changes = edit(&mut model, "A2000", "0");
    assert!(!changes.full);
    assert_eq!(changes.formulas_run, 1000);
    assert_eq!(model._get_text("A3000"), *"1000");
}

#[test]
fn a_formula_whose_value_stays_is_run_but_not_reported() {
    let mut model = new_empty_model();
    model._set("A1", "1");
    model._set("B1", "=IF(A1>0,\"positive\",\"not\")");
    model.evaluate();
    let changes = edit(&mut model, "A1", "2");
    assert_eq!(changes.formulas_run, 1);
    assert_eq!(changes.areas, vec![cell(0, 1, 1)]);
}

#[test]
fn what_reads_a_formula_whose_value_stayed_does_not_run() {
    let mut model = new_empty_model();
    model._set("A1", "1");
    model._set("B1", "=IF(A1>0,\"positive\",\"not\")");
    model._set("C1", "=B1&\"!\"");
    model._set("D1", "=C1&\"?\"");
    model._set("E1", "=A1*2");
    model.evaluate();

    // B1 and E1 read A1; B1 stays, so C1 and D1 do not run
    let changes = edit(&mut model, "A1", "2");
    assert_eq!(changes.formulas_run, 2);
    assert_eq!(changes.areas, vec![cell(0, 1, 1), cell(0, 1, 5)]);
    assert_eq!(model._get_text("D1"), *"positive!?");
    // Again, with what the ones that did not run read put back
    let changes = edit(&mut model, "A1", "3");
    assert_eq!(changes.formulas_run, 2);
    // B1 changes: everything down from it runs
    let changes = edit(&mut model, "A1", "-1");
    assert_eq!(changes.formulas_run, 4);
    assert_eq!(model._get_text("D1"), *"not!?");
    let changes = edit(&mut model, "C1", "=B1&\"#\"");
    assert_eq!(changes.formulas_run, 2);
    assert_eq!(model._get_text("D1"), *"not#?");
}

#[test]
fn ranges_whole_columns_and_criteria() {
    let mut model = new_empty_model();
    for row in 1..=20 {
        model._set(&format!("A{row}"), &format!("{row}"));
        model._set(
            &format!("B{row}"),
            if row % 2 == 0 { "even" } else { "odd" },
        );
    }
    model._set("D1", "=SUM(A1:A20)");
    model._set("D2", "=SUM(A:A)");
    model._set("D3", "=SUMIF(B1:B20,\"even\",A1:A20)");
    model._set("D4", "=COUNTIF(B:B,\"odd\")");
    model._set("D5", "=SUM(A1:A5)");
    model.evaluate();

    // Inside the column, below the ranges, and in the criteria
    let changes = edit(&mut model, "A10", "0");
    assert_eq!(changes.formulas_run, 3);
    assert_eq!(model._get_text("D1"), *"200");
    assert_eq!(model._get_text("D2"), *"200");
    assert_eq!(model._get_text("D3"), *"100");

    let changes = edit(&mut model, "A500", "1000");
    assert_eq!(changes.formulas_run, 1);
    assert_eq!(model._get_text("D2"), *"1200");

    let changes = edit(&mut model, "B1", "even");
    assert_eq!(changes.formulas_run, 2);
    assert_eq!(model._get_text("D3"), *"101");
    assert_eq!(model._get_text("D4"), *"9");
}

#[test]
fn references_made_by_evaluating_follow_what_they_came_to() {
    let mut model = new_empty_model();
    model._set("A1", "1");
    model._set("A2", "2");
    model._set("C1", "A1");
    model._set("D1", "=INDIRECT(C1)");
    model._set("D2", "=OFFSET(A1,C2,0)");
    model._set("C2", "0");
    model.evaluate();
    assert_eq!(model._get_text("D1"), *"1");

    let changes = edit(&mut model, "A1", "10");
    assert_eq!(changes.formulas_run, 2);
    assert_eq!(model._get_text("D1"), *"10");
    assert_eq!(model._get_text("D2"), *"10");

    // The references move; what they read moves with them
    edit(&mut model, "C1", "A2");
    edit(&mut model, "C2", "1");
    assert_eq!(model._get_text("D1"), *"2");
    assert_eq!(model._get_text("D2"), *"2");
    // OFFSET's base is a reference it is given, and counts as read though
    // only where it is matters: D2 runs, D1 does not
    let changes = edit(&mut model, "A1", "11");
    assert_eq!(changes.formulas_run, 1);
    assert_eq!(model._get_text("D2"), *"2");
    let changes = edit(&mut model, "A2", "20");
    assert_eq!(changes.formulas_run, 2);
    assert_eq!(model._get_text("D1"), *"20");
    assert_eq!(model._get_text("D2"), *"20");
}

#[test]
fn defined_names_and_other_sheets() {
    let mut model = new_empty_model();
    model.add_sheet("Data").unwrap();
    model._set("Data!A1", "3");
    model._set("Data!A2", "4");
    model
        .new_defined_name(DefinedNameEntry::new("amounts", "=Data!$A$1:$A$2"))
        .unwrap();
    model._set("B1", "=SUM(amounts)");
    model._set("B2", "=Data!A1*10");
    model.evaluate();

    let changes = edit(&mut model, "Data!A2", "6");
    assert!(!changes.full);
    assert_eq!(changes.formulas_run, 1);
    assert_eq!(model._get_text("B1"), *"9");
    let changes = edit(&mut model, "Data!A1", "1");
    assert_eq!(changes.formulas_run, 2);
    assert_eq!(model._get_text("B2"), *"10");
}

#[test]
fn formulas_written_and_formulas_gone() {
    let mut model = new_empty_model();
    model._set("A1", "1");
    model._set("B1", "=A1+1");
    model._set("C1", "=B1+1");
    model.evaluate();

    // A new formula runs, and so does what reads its cell
    let changes = edit(&mut model, "B1", "=A1*100");
    assert_eq!(changes.formulas_run, 2);
    assert_eq!(model._get_text("C1"), *"101");

    // A formula replaced by a constant reads nothing any more
    edit(&mut model, "B1", "5");
    assert_eq!(model._get_text("C1"), *"6");
    let changes = edit(&mut model, "A1", "2");
    assert_eq!(changes.formulas_run, 0);
}

#[test]
fn volatile_formulas_run_every_time() {
    let mut model = new_empty_model();
    model._set("A1", "=RAND()");
    model._set("A2", "=A1*0+1");
    model._set("A3", "=5");
    model.evaluate();
    let changes = edit(&mut model, "C1", "1");
    assert!(!changes.full);
    assert_eq!(changes.formulas_run, 2);
}

#[test]
fn moving_cells_hiding_rows_cycles_and_arrays_evaluate_everything() {
    let mut model = new_empty_model();
    model._set("A1", "1");
    model._set("A2", "2");
    model._set("B1", "=SUM(A1:A2)");
    model._set("B2", "=SUBTOTAL(109,A1:A2)");
    model.evaluate();

    model.insert_rows(0, 1, 1).unwrap();
    assert!(model.evaluate_changes().full);
    assert_eq!(model._get_text("B2"), *"3");

    model
        .workbook
        .worksheet_mut(0)
        .unwrap()
        .set_row_hidden(2, true)
        .unwrap();
    assert!(model.evaluate_changes().full);
    assert_eq!(model._get_text("B3"), *"2");

    // A cycle
    let changes = edit(&mut model, "A2", "=B2");
    assert!(changes.full);
    assert_eq!(model._get_text("B2"), *"#CIRC!");
    let changes = edit(&mut model, "A2", "1");
    assert!(changes.full);
    assert_eq!(model._get_text("B2"), *"3");

    // A dynamic array, and what blocks it
    let changes = edit(&mut model, "D1", "=SEQUENCE(3)");
    assert!(changes.full);
    let changes = edit(&mut model, "D3", "x");
    assert!(changes.full);
    assert_eq!(model._get_text("D1"), *"#SPILL!");
    let changes = edit(&mut model, "D3", "");
    assert!(changes.full);
    assert_eq!(model._get_text("D3"), *"3");
    // An edit away from it is not
    let changes = edit(&mut model, "F9", "1");
    assert!(!changes.full);
}

/// A small generator, so that a failing sequence can be run again.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const SIZE: i32 = 6;

fn random_reference(random: &mut Random) -> String {
    let sheet = if random.below(4) == 0 { "Sheet2!" } else { "" };
    let column = (b'A' + random.below(SIZE as u64) as u8) as char;
    format!("{sheet}{column}{}", 1 + random.below(SIZE as u64))
}

fn random_input(random: &mut Random) -> String {
    let a = random_reference(random);
    let b = random_reference(random);
    match random.below(16) {
        0..=4 => format!("{}", random.below(10)),
        5 => String::new(),
        6 => "text".to_string(),
        7 => format!("={a}+{b}"),
        8 => format!("=SUM(A1:{})", b.trim_start_matches("Sheet2!")),
        9 => format!("=SUMIF(A1:A{SIZE},\">4\",B1:B{SIZE})"),
        10 => format!("=INDIRECT(\"A\"&MAX(1,MIN({SIZE},{a})))"),
        11 => format!("=OFFSET(A1,MOD({a},{SIZE}),1)"),
        12 => format!("=IF({a}>4,{b},0)"),
        13 => format!("=COUNTIF(Sheet2!A:A,{a})"),
        14 => format!("=VLOOKUP({a},A1:B{SIZE},2,FALSE)"),
        _ => format!("=SUM(amounts)+{a}"),
    }
}

/// Every cell of both sheets, as shown.
fn snapshot(model: &Model) -> Vec<String> {
    let mut values = Vec::new();
    for sheet in 0..2 {
        for row in 1..=SIZE {
            for column in 1..=SIZE {
                values.push(model._get_text_at(sheet, row, column));
            }
        }
    }
    values
}

#[test]
fn random_edits_end_where_a_full_evaluation_ends() {
    let mut incremental_steps = 0;
    let mut steps = 0;
    for seed in 1..=150u64 {
        let mut random = Random(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut model = new_empty_model();
        let mut full = new_empty_model();
        for model in [&mut model, &mut full] {
            model.add_sheet("Sheet2").unwrap();
            model
                .new_defined_name(DefinedNameEntry::new("amounts", "=Sheet2!$B$1:$B$3"))
                .unwrap();
        }
        for _ in 0..40 {
            let sheet = if random.below(3) == 0 { "Sheet2!" } else { "" };
            let column = (b'A' + random.below(SIZE as u64) as u8) as char;
            let reference = format!("{sheet}{column}{}", 1 + random.below(SIZE as u64));
            let input = random_input(&mut random);
            model._set(&reference, &input);
            full._set(&reference, &input);
            let changes = model.evaluate_changes();
            full.evaluate();
            steps += 1;
            if !changes.full {
                incremental_steps += 1;
            }
            assert_eq!(
                snapshot(&model),
                snapshot(&full),
                "seed {seed}: {reference} = {input}"
            );
        }
    }
    // Most steps take the incremental way: the fallback is not what passes
    // (about 70%; the others meet a cycle or an array, or are the first)
    assert!(
        incremental_steps * 10 > steps * 6,
        "{incremental_steps} of {steps} steps incremental"
    );
}
