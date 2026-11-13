//! Times the evaluation of formulas over a sheet of data: a data sheet of
//! many rows that hold no formula, as a table loaded from a database or a
//! CSV file does, and a second sheet whose formulas read whole columns of it.
//!
//! The data sheet has 20 columns: an id, six columns of text and thirteen of
//! numbers. A row of it is a row of cells. A third sheet holds the same
//! thirteen columns of numbers alone, which the sheet stores as numbers.
//!
//! The memory the workbook takes is printed first. Then every formula is
//! evaluated alone, a few times, and the best time is printed with the
//! allocations of one evaluation and the most memory it held on top of the
//! workbook. Then a set of formulas is evaluated together.
//!
//! Usage:
//!   cargo run --release --example data_sheet -- [rows] [mix] [only] [times]
//!
//! Defaults: 100,000 rows, 300 formulas in the mix. With `only`, the name of
//! one of the formulas, that formula is evaluated `times` times (20) and
//! nothing else is, which is what a profiler wants.
//!
//! With `RELOAD` set in the environment the workbook is saved as bytes and
//! read back before anything is evaluated, as a stored workbook is opened.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use ironcalc_base::types::Cell;
use ironcalc_base::Model;

struct Counting;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
/// Bytes allocated and not yet freed, and the most there have been.
static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);

fn grew(bytes: usize) {
    let live = LIVE.fetch_add(bytes as u64, Ordering::Relaxed) + bytes as u64;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

// SAFETY: every call is passed on to the system allocator unchanged.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        grew(layout.size());
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
        System.dealloc(pointer, layout)
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
        grew(size);
        System.realloc(pointer, layout, size)
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

const STATUSES: [&str; 6] = [
    "open",
    "placed",
    "approved",
    "sent",
    "completed",
    "cancelled",
];
const REGIONS: [&str; 8] = [
    "north", "south", "east", "west", "centre", "coast", "islands", "abroad",
];
const CHANNELS: [&str; 4] = ["web", "phone", "store", "partner"];

/// A small generator, so that the sheet is the same on every run.
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

fn build(rows: i32) -> Result<Model<'static>, String> {
    let mut model = Model::new_empty("data-sheet", "en", "UTC", "en")?;
    model.add_sheet("Data")?;
    model.add_sheet("Numbers")?;
    let mut workbook = model.workbook;
    let mut strings = std::mem::take(&mut workbook.shared_strings);
    let mut text = |value: String| -> i32 {
        strings.push(value);
        (strings.len() - 1) as i32
    };
    let customers: Vec<i32> = (0..2000).map(|i| text(format!("Customer {i}"))).collect();
    let products: Vec<i32> = (0..200).map(|i| text(format!("Product {i}"))).collect();
    let notes: Vec<i32> = (0..50).map(|i| text(format!("Note {i}"))).collect();
    let statuses: Vec<i32> = STATUSES.iter().map(|s| text(s.to_string())).collect();
    let regions: Vec<i32> = REGIONS.iter().map(|s| text(s.to_string())).collect();
    let channels: Vec<i32> = CHANNELS.iter().map(|s| text(s.to_string())).collect();
    // A row of the data sheet; the generator makes the same rows again
    // when started again
    let row_cells = |row: i32, random: &mut Random| -> Vec<(i32, Cell)> {
        let pick = |from: &[i32], random: &mut Random| Cell::SharedString {
            si: from[random.below(from.len() as u64) as usize],
            s: 0,
        };
        let number = |v: f64| Cell::NumberCell { v, s: 0 };
        let quantity = (1 + random.below(20)) as f64;
        let price = (100 + random.below(9900)) as f64 / 100.0;
        let mut cells = Vec::with_capacity(20);
        cells.push((1, number((row - 1) as f64)));
        cells.push((2, pick(&customers, random)));
        cells.push((3, pick(&statuses, random)));
        cells.push((4, pick(&regions, random)));
        cells.push((5, pick(&products, random)));
        cells.push((6, pick(&channels, random)));
        cells.push((7, pick(&notes, random)));
        cells.push((8, number(quantity)));
        cells.push((9, number(price)));
        cells.push((10, number(quantity * price)));
        for column in 11..=20 {
            cells.push((column, number(random.below(100_000) as f64 / 100.0)));
        }
        cells
    };
    // One sheet after the other, each row made at its size, as a table
    // read from a database is loaded
    let seed = 0x9E37_79B9_7F4A_7C15;
    let mut random = Random(seed);
    for row in 2..=rows + 1 {
        let cells = row_cells(row, &mut random);
        workbook.worksheets[1].sheet_data.set_row(row, cells);
    }
    let mut random = Random(seed);
    for row in 2..=rows + 1 {
        let numbers: Vec<(i32, Cell)> = row_cells(row, &mut random)
            .into_iter()
            .skip(7)
            .enumerate()
            .map(|(index, (_, cell))| (index as i32 + 1, cell))
            .collect();
        workbook.worksheets[2].sheet_data.set_row(row, numbers);
    }
    workbook.shared_strings = strings;
    Model::from_workbook(workbook, "en")
}

/// The formulas, by name. `{n}` is the last row of the data and `{k}` a
/// column letter that the mix varies.
fn formulas() -> Vec<(&'static str, &'static str)> {
    vec![
        ("nothing", "=1+1"),
        ("sum", "=SUM(Data!{k}2:{k}{n})"),
        ("sum-numbers", "=SUM(Numbers!C2:C{n})"),
        ("sum-block", "=SUM(Data!H2:T{n})"),
        ("sum-block-numbers", "=SUM(Numbers!A2:M{n})"),
        ("average", "=AVERAGE(Data!{k}2:{k}{n})"),
        ("max", "=MAX(Data!{k}2:{k}{n})"),
        ("count", "=COUNT(Data!{k}2:{k}{n})"),
        ("counta-text", "=COUNTA(Data!B2:B{n})"),
        ("stdev", "=STDEV(Data!{k}2:{k}{n})"),
        ("median", "=MEDIAN(Data!{k}2:{k}{n})"),
        ("sumif", "=SUMIF(Data!C2:C{n},\"open\",Data!{k}2:{k}{n})"),
        (
            "sumif-number",
            "=SUMIF(Data!H2:H{n},\">10\",Data!{k}2:{k}{n})",
        ),
        (
            "sumifs",
            "=SUMIFS(Data!{k}2:{k}{n},Data!C2:C{n},\"open\",Data!D2:D{n},\"west\")",
        ),
        ("countif-text", "=COUNTIF(Data!C2:C{n},\"open\")"),
        ("countif-number", "=COUNTIF(Data!{k}2:{k}{n},\">10\")"),
        (
            "countifs",
            "=COUNTIFS(Data!C2:C{n},\"open\",Data!H2:H{n},\">10\")",
        ),
        (
            "averageifs",
            "=AVERAGEIFS(Data!{k}2:{k}{n},Data!C2:C{n},\"open\")",
        ),
        ("maxifs", "=MAXIFS(Data!{k}2:{k}{n},Data!C2:C{n},\"open\")"),
        ("sumproduct", "=SUMPRODUCT(Data!H2:H{n},Data!{k}2:{k}{n})"),
        (
            "sumproduct-numbers",
            "=SUMPRODUCT(Numbers!A2:A{n},Numbers!B2:B{n})",
        ),
        ("vlookup-last", "=VLOOKUP({n}-1,Data!A2:J{n},10,FALSE)"),
        ("match-last", "=MATCH({n}-1,Data!A2:A{n},0)"),
        (
            "xlookup-last",
            "=XLOOKUP({n}-1,Data!A2:A{n},Data!{k}2:{k}{n})",
        ),
        ("index", "=INDEX(Data!{k}2:{k}{n},{n}/2)"),
        ("whole-column", "=SUM(Data!J:J)"),
    ]
}

/// The formulas the mix is made of, in turn.
const MIX: [&str; 10] = [
    "sum",
    "average",
    "max",
    "count",
    "sumif",
    "sumifs",
    "countif-text",
    "countifs",
    "sumproduct",
    "vlookup-last",
];

fn text_of(formula: &str, rows: i32, column: usize) -> String {
    const COLUMNS: [&str; 10] = ["J", "I", "H", "K", "L", "M", "N", "O", "P", "Q"];
    formula
        .replace("{n}", &(rows + 1).to_string())
        .replace("{k}", COLUMNS[column % COLUMNS.len()])
}

/// One measurement: the best time of the evaluations, and of the last of
/// them the allocations and the most memory it held beyond what was held
/// before it started.
struct Measured {
    best: Duration,
    allocations: u64,
    peak: u64,
}

impl Measured {
    fn print(&self, name: &str, value: &str) {
        println!(
            "{name:<20} {:>10.3} {:>12} {:>9.2}  {value}",
            self.best.as_secs_f64() * 1000.0,
            self.allocations,
            megabytes(self.peak),
        );
    }
}

fn evaluate(model: &mut Model, runs: usize) -> Measured {
    let mut measured = Measured {
        best: Duration::MAX,
        allocations: 0,
        peak: 0,
    };
    for _ in 0..runs {
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        let live = LIVE.load(Ordering::Relaxed);
        PEAK.store(live, Ordering::Relaxed);
        let start = Instant::now();
        model.evaluate();
        measured.best = measured.best.min(start.elapsed());
        measured.allocations = ALLOCATIONS.load(Ordering::Relaxed) - before;
        measured.peak = PEAK.load(Ordering::Relaxed).saturating_sub(live);
    }
    measured
}

fn megabytes(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let rows: i32 = match args.next() {
        Some(value) => value.parse()?,
        None => 100_000,
    };
    let mix: usize = match args.next() {
        Some(value) => value.parse()?,
        None => 300,
    };
    let only = args.next();
    let times: usize = match args.next() {
        Some(value) => value.parse()?,
        None => 20,
    };
    if !(1..=1_000_000).contains(&rows) {
        return Err("rows must be between 1 and 1,000,000".into());
    }

    let empty = LIVE.load(Ordering::Relaxed);
    let start = Instant::now();
    let mut model = build(rows)?;
    println!(
        "built {rows} rows of 20 and of 13 columns in {:?}",
        start.elapsed()
    );
    let usage = model.memory_usage();
    println!(
        "in memory {:.1} MB ({:.1} MB by memory_usage), as bytes {:.1} MB",
        megabytes(LIVE.load(Ordering::Relaxed) - empty),
        megabytes(usage.total() as u64),
        megabytes(model.to_bytes().len() as u64)
    );
    if std::env::var("RELOAD").is_ok() {
        model = Model::from_bytes(&model.to_bytes(), "en")?;
    }
    let all = formulas();

    if let Some(name) = only {
        let Some((_, formula)) = all.iter().find(|(n, _)| *n == name) else {
            return Err(format!("no formula named '{name}'").into());
        };
        model.set_user_input(0, 1, 1, text_of(formula, rows, 0))?;
        let start = Instant::now();
        for _ in 0..times {
            model.evaluate();
        }
        println!("{name}: {times} evaluations in {:?}", start.elapsed());
        println!("A1 = {}", model.get_formatted_cell_value(0, 1, 1)?);
        return Ok(());
    }

    println!(
        "{:<20} {:>10} {:>12} {:>9}  value",
        "formula", "ms", "allocations", "peak MB"
    );
    for (name, formula) in &all {
        model.set_user_input(0, 1, 1, text_of(formula, rows, 0))?;
        let measured = evaluate(&mut model, 5);
        measured.print(name, &model.get_formatted_cell_value(0, 1, 1)?);
    }

    for index in 0..mix {
        let name = MIX[index % MIX.len()];
        let Some((_, formula)) = all.iter().find(|(n, _)| *n == name) else {
            continue;
        };
        let row = index as i32 + 1;
        model.set_user_input(0, row, 1, text_of(formula, rows, index / MIX.len()))?;
    }
    evaluate(&mut model, 3).print(&format!("mix of {mix}"), "");
    Ok(())
}
