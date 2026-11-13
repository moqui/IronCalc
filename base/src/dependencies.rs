//! What each formula read when it last ran, and the other way round: which
//! formulas read a cell. With it an edit evaluates only the formulas that
//! read what changed, and what read those, instead of every formula of the
//! workbook (`Model::evaluate_changes`).
//!
//! The reads are recorded as the evaluation resolves a formula's references,
//! not as it reads cells: a reference is a cell, a range is one rectangle
//! however many cells a function then walks through it, and a reference that
//! is only known once evaluated (INDIRECT, OFFSET, a defined name, the range
//! operator) is recorded as what it came to. A function only ever reads
//! inside the references it is given, so the references cover the reads.
//! Recorded at run time, they are what the formula used this time: a formula
//! whose reference depends on a value reads that value too, and when the
//! value changes the formula runs again and records afresh.
//!
//! The index finds the readers of an area: single cells in a map, ranges up
//! to `NARROW` columns wide under each of their columns with their rows,
//! wider ones in a list.

use std::collections::{HashMap, HashSet};

use crate::evaluation::{CellKey, KeyBuild};

/// Ranges at most this many columns wide are indexed column by column.
const NARROW: i32 = 64;

/// Areas with at most this many cells are looked up cell by cell in the map
/// of single cells; larger ones have the map walked instead.
const PROBE_CELLS: i64 = 4096;

/// A range under one of its columns: `(first row, last row, formula)`.
type ColumnSpan = (i32, i32, CellKey);

/// A rectangle of one sheet, both corners included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Rect {
    pub(crate) sheet: u32,
    pub(crate) row1: i32,
    pub(crate) column1: i32,
    pub(crate) row2: i32,
    pub(crate) column2: i32,
}

impl Rect {
    pub(crate) fn cell((sheet, row, column): CellKey) -> Rect {
        Rect {
            sheet,
            row1: row,
            column1: column,
            row2: row,
            column2: column,
        }
    }

    pub(crate) fn contains(&self, (sheet, row, column): CellKey) -> bool {
        sheet == self.sheet
            && (self.row1..=self.row2).contains(&row)
            && (self.column1..=self.column2).contains(&column)
    }

    pub(crate) fn intersects(&self, other: &Rect) -> bool {
        self.sheet == other.sheet
            && self.row1 <= other.row2
            && other.row1 <= self.row2
            && self.column1 <= other.column2
            && other.column1 <= self.column2
    }

    fn is_cell(&self) -> bool {
        self.row1 == self.row2 && self.column1 == self.column2
    }

    fn cells(&self) -> i64 {
        (self.row2 as i64 - self.row1 as i64 + 1) * (self.column2 as i64 - self.column1 as i64 + 1)
    }
}

#[derive(Default)]
pub(crate) struct Dependencies {
    /// A full evaluation has recorded every formula since the workbook was
    /// built or last changed in a way that moves cells.
    pub(crate) ready: bool,
    /// What each formula read when it last ran.
    reads: HashMap<CellKey, Vec<Rect>, KeyBuild>,
    /// The formulas that called a volatile function when they last ran.
    pub(crate) volatile: HashSet<CellKey, KeyBuild>,
    /// Formulas that read a cell.
    by_cell: HashMap<CellKey, Vec<CellKey>, KeyBuild>,
    /// Formulas that read a range at most `NARROW` columns wide, under each
    /// of its columns: `(first row, last row, formula)`.
    by_column: HashMap<(u32, i32), Vec<ColumnSpan>, KeyBuild>,
    /// Formulas that read a wider range.
    wide: Vec<(Rect, CellKey)>,
}

impl Dependencies {
    /// Forgets everything: a full evaluation records it all again.
    pub(crate) fn clear(&mut self) {
        self.ready = false;
        self.reads.clear();
        self.volatile.clear();
        self.by_cell.clear();
        self.by_column.clear();
        self.wide.clear();
    }

    /// `formula` read `read`. Kept as it comes; `index` sorts it out.
    #[inline]
    pub(crate) fn record(&mut self, formula: CellKey, read: Rect) {
        self.reads.entry(formula).or_default().push(read);
    }

    pub(crate) fn record_volatile(&mut self, formula: CellKey) {
        self.volatile.insert(formula);
    }

    /// Puts what `formula` read in its last run in the index, once each.
    pub(crate) fn index(&mut self, formula: CellKey) {
        let Some(reads) = self.reads.get_mut(&formula) else {
            return;
        };
        reads.sort_unstable();
        reads.dedup();
        reads.shrink_to_fit();
        for read in reads.iter() {
            if read.is_cell() {
                self.by_cell
                    .entry((read.sheet, read.row1, read.column1))
                    .or_default()
                    .push(formula);
            } else if read.column2 - read.column1 < NARROW {
                for column in read.column1..=read.column2 {
                    self.by_column
                        .entry((read.sheet, column))
                        .or_default()
                        .push((read.row1, read.row2, formula));
                }
            } else {
                self.wide.push((*read, formula));
            }
        }
    }

    /// Indexes every formula recorded, after a full evaluation.
    pub(crate) fn index_all(&mut self) {
        let formulas: Vec<CellKey> = self.reads.keys().copied().collect();
        for formula in formulas {
            self.index(formula);
        }
        self.ready = true;
    }

    /// Takes what `formula` read out of the index, before it runs again or
    /// after it is gone.
    pub(crate) fn forget(&mut self, formula: CellKey) {
        self.volatile.remove(&formula);
        let Some(reads) = self.reads.remove(&formula) else {
            return;
        };
        for read in reads {
            if read.is_cell() {
                let key = (read.sheet, read.row1, read.column1);
                if let Some(readers) = self.by_cell.get_mut(&key) {
                    readers.retain(|f| *f != formula);
                    if readers.is_empty() {
                        self.by_cell.remove(&key);
                    }
                }
            } else if read.column2 - read.column1 < NARROW {
                for column in read.column1..=read.column2 {
                    let key = (read.sheet, column);
                    if let Some(spans) = self.by_column.get_mut(&key) {
                        spans.retain(|(_, _, f)| *f != formula);
                        if spans.is_empty() {
                            self.by_column.remove(&key);
                        }
                    }
                }
            } else {
                self.wide.retain(|(_, f)| *f != formula);
            }
        }
    }

    /// The formulas recorded in `area`: the formulas that sat there when
    /// they last ran.
    pub(crate) fn formulas_within(&self, area: &Rect) -> Vec<CellKey> {
        if area.cells() <= PROBE_CELLS {
            let mut found = Vec::new();
            for row in area.row1..=area.row2 {
                for column in area.column1..=area.column2 {
                    let key = (area.sheet, row, column);
                    if self.reads.contains_key(&key) || self.volatile.contains(&key) {
                        found.push(key);
                    }
                }
            }
            return found;
        }
        self.reads
            .keys()
            .chain(self.volatile.iter())
            .filter(|key| area.contains(**key))
            .copied()
            .collect::<HashSet<CellKey>>()
            .into_iter()
            .collect()
    }

    /// Adds to `out` the formulas that read a cell of `area`.
    pub(crate) fn readers_of(&self, area: &Rect, out: &mut Vec<CellKey>) {
        if area.cells() <= PROBE_CELLS {
            for row in area.row1..=area.row2 {
                for column in area.column1..=area.column2 {
                    if let Some(readers) = self.by_cell.get(&(area.sheet, row, column)) {
                        out.extend_from_slice(readers);
                    }
                }
            }
        } else {
            for (key, readers) in &self.by_cell {
                if area.contains(*key) {
                    out.extend_from_slice(readers);
                }
            }
        }
        let width = area.column2 as i64 - area.column1 as i64 + 1;
        let mut spans_of = |spans: &Vec<ColumnSpan>| {
            for (row1, row2, formula) in spans {
                if *row1 <= area.row2 && area.row1 <= *row2 {
                    out.push(*formula);
                }
            }
        };
        if width <= PROBE_CELLS {
            for column in area.column1..=area.column2 {
                if let Some(spans) = self.by_column.get(&(area.sheet, column)) {
                    spans_of(spans);
                }
            }
        } else {
            for ((sheet, column), spans) in &self.by_column {
                if *sheet == area.sheet && (area.column1..=area.column2).contains(column) {
                    spans_of(spans);
                }
            }
        }
        for (rect, formula) in &self.wide {
            if rect.intersects(area) {
                out.push(*formula);
            }
        }
    }

    /// The memory the index takes, in bytes: an estimate from the sizes and
    /// capacities of its parts.
    pub(crate) fn heap_size(&self) -> usize {
        use std::mem::size_of;
        // A hash map holds its entries and a byte of control for each slot.
        fn map<K, V>(capacity: usize) -> usize {
            capacity * (size_of::<(K, V)>() + 1)
        }
        map::<CellKey, Vec<Rect>>(self.reads.capacity())
            + self
                .reads
                .values()
                .map(|reads| reads.capacity() * size_of::<Rect>())
                .sum::<usize>()
            + map::<CellKey, ()>(self.volatile.capacity())
            + map::<CellKey, Vec<CellKey>>(self.by_cell.capacity())
            + self
                .by_cell
                .values()
                .map(|readers| readers.capacity() * size_of::<CellKey>())
                .sum::<usize>()
            + map::<(u32, i32), Vec<ColumnSpan>>(self.by_column.capacity())
            + self
                .by_column
                .values()
                .map(|spans| spans.capacity() * size_of::<ColumnSpan>())
                .sum::<usize>()
            + self.wide.capacity() * size_of::<(Rect, CellKey)>()
    }
}
