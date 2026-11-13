//! A row of cells, and how it holds them: as cells, or as nothing but the
//! numbers when that is all there is.

use std::borrow::Cow;

use crate::constants::LAST_COLUMN;
use crate::expressions::token::Error;
use crate::types::Cell;

/// Rows this short are searched from the left; longer ones by bisection.
const LINEAR_SEARCH_LIMIT: usize = 8;

/// The most gaps a row of constants holds for each of its cells. A gap costs
/// as much as a constant, a sixteenth of what a cell costs in a row of
/// cells, so a row of constants with this many is still the smaller.
const GAPS_PER_CONSTANT: usize = 2;

/// A cell that holds a value and no formula, in the 16 bytes that takes: a
/// cell of any kind takes 48, for the kinds that hold a formula.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Constant {
    /// No cell: a gap in the row.
    Absent,
    Empty {
        s: i32,
    },
    Number {
        v: f64,
        s: i32,
    },
    Boolean {
        v: bool,
        s: i32,
    },
    Text {
        si: i32,
        s: i32,
    },
    Error {
        ei: Error,
        s: i32,
    },
}

impl Constant {
    /// The constant for a cell, if the cell holds no formula.
    fn of(cell: &Cell) -> Option<Constant> {
        Some(match cell {
            Cell::EmptyCell { s } => Constant::Empty { s: *s },
            Cell::NumberCell { v, s } => Constant::Number { v: *v, s: *s },
            Cell::BooleanCell { v, s } => Constant::Boolean { v: *v, s: *s },
            Cell::SharedString { si, s } => Constant::Text { si: *si, s: *s },
            Cell::ErrorCell { ei, s } => Constant::Error {
                ei: ei.clone(),
                s: *s,
            },
            Cell::CellFormula { .. } | Cell::ArrayFormula { .. } | Cell::SpillCell { .. } => {
                return None
            }
        })
    }

    /// The cell, `None` for a gap.
    fn to_cell(&self) -> Option<Cell> {
        Some(match self {
            Constant::Absent => return None,
            Constant::Empty { s } => Cell::EmptyCell { s: *s },
            Constant::Number { v, s } => Cell::NumberCell { v: *v, s: *s },
            Constant::Boolean { v, s } => Cell::BooleanCell { v: *v, s: *s },
            Constant::Text { si, s } => Cell::SharedString { si: *si, s: *s },
            Constant::Error { ei, s } => Cell::ErrorCell {
                ei: ei.clone(),
                s: *s,
            },
        })
    }
}

/// What `SheetData::peek` finds at a position.
pub(crate) enum Peek<'a> {
    /// A number of a row of numbers.
    Number(f64),
    /// A cell of a row of constants (never `Absent`).
    Constant(&'a Constant),
    /// Any cell, as the row holds it.
    Cell(&'a Cell),
    Nothing,
}

/// Where a column is in a run of `length` places from `first_column`.
#[inline]
fn place(first_column: i32, length: usize, column: i32) -> Option<usize> {
    let offset = column as i64 - first_column as i64;
    if (0..length as i64).contains(&offset) {
        Some(offset as usize)
    } else {
        None
    }
}

/// The cells of a row.
#[derive(Debug, Clone)]
pub(super) enum Row {
    /// Any cells, with their column, sorted by column.
    Cells(Vec<(i32, Cell)>),
    /// Numbers in consecutive columns, the first of them in `first_column`.
    Numbers {
        first_column: i32,
        /// The style of the numbers, all but those in `other_styles`.
        style: i32,
        values: Vec<f64>,
        /// The numbers of another style: where each is among `values`, and
        /// its style, in the order of `values`. A sheet of data has a number
        /// here and there shown in its own way, and is no less a sheet of
        /// numbers for it.
        other_styles: Box<[(u32, i32)]>,
    },
    /// Cells that hold no formula, by column from `first_column` on, with a
    /// gap held as `Constant::Absent`; neither the first nor the last is a
    /// gap. A sheet of data with texts in it is made of these.
    Constants {
        first_column: i32,
        cells: Vec<Constant>,
    },
}

/// The style of the number at `offset` in a row of numbers.
#[inline]
fn style_at(style: i32, other_styles: &[(u32, i32)], offset: usize) -> i32 {
    if other_styles.is_empty() {
        return style;
    }
    match other_styles.binary_search_by_key(&(offset as u32), |(at, _)| *at) {
        Ok(index) => other_styles[index].1,
        Err(_) => style,
    }
}

impl Default for Row {
    fn default() -> Self {
        Row::Cells(Vec::new())
    }
}

/// Where the column is among the cells, or where it would go.
#[inline]
fn position(cells: &[(i32, Cell)], column: i32) -> Result<usize, usize> {
    // In a row filled from the first column on with no gaps, the cell of a
    // column is at that column's place: one look, where a search takes a few,
    // each one somewhere else in memory.
    let guess = (column as usize).wrapping_sub(1);
    if let Some((c, _)) = cells.get(guess) {
        if *c == column {
            return Ok(guess);
        }
    }
    if cells.len() <= LINEAR_SEARCH_LIMIT {
        for (index, (c, _)) in cells.iter().enumerate() {
            if *c >= column {
                return if *c == column { Ok(index) } else { Err(index) };
            }
        }
        Err(cells.len())
    } else {
        cells.binary_search_by_key(&column, |(c, _)| *c)
    }
}

/// The cells of a row from some column on, in ascending order of column.
pub(super) enum RowIter<'a> {
    Cells(std::slice::Iter<'a, (i32, Cell)>),
    Numbers {
        /// The column of the next value, and where it is in the row.
        column: i32,
        offset: u32,
        style: i32,
        values: std::slice::Iter<'a, f64>,
        /// The numbers of another style from here on.
        other_styles: &'a [(u32, i32)],
    },
    Constants {
        /// The column of the next one.
        column: i32,
        cells: std::slice::Iter<'a, Constant>,
    },
}

impl<'a> Iterator for RowIter<'a> {
    type Item = (i32, Cow<'a, Cell>);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            RowIter::Cells(cells) => cells
                .next()
                .map(|(column, cell)| (*column, Cow::Borrowed(cell))),
            RowIter::Numbers {
                column,
                offset,
                style,
                values,
                other_styles,
            } => {
                let v = *values.next()?;
                let s = match other_styles.split_first() {
                    Some(((at, other), rest)) if at == offset => {
                        *other_styles = rest;
                        *other
                    }
                    _ => *style,
                };
                let cell_column = *column;
                *column += 1;
                *offset += 1;
                Some((cell_column, Cow::Owned(Cell::NumberCell { v, s })))
            }
            RowIter::Constants { column, cells } => loop {
                let constant = cells.next()?;
                let cell_column = *column;
                *column += 1;
                if let Some(cell) = constant.to_cell() {
                    return Some((cell_column, Cow::Owned(cell)));
                }
            },
        }
    }
}

impl Row {
    /// The row for these cells, which are sorted by column with no column
    /// given twice: the numbers alone if that is all there is to them.
    pub(super) fn from_cells(cells: Vec<(i32, Cell)>) -> Row {
        if let Some(row) = Row::numbers_of(&cells) {
            return row;
        }
        if let Some(row) = Row::constants_of(&cells) {
            return row;
        }
        Row::Cells(cells)
    }

    /// The row of constants for these cells, sorted by column with no column
    /// given twice, if none holds a formula and the gaps between them are
    /// few enough.
    fn constants_of(cells: &[(i32, Cell)]) -> Option<Row> {
        let first_column = cells.first()?.0;
        let span = (cells.last()?.0 as i64 - first_column as i64 + 1) as usize;
        if span > cells.len() * (1 + GAPS_PER_CONSTANT) {
            return None;
        }
        let mut constants = vec![Constant::Absent; span];
        for (column, cell) in cells {
            constants[(*column - first_column) as usize] = Constant::of(cell)?;
        }
        Some(Row::Constants {
            first_column,
            cells: constants,
        })
    }

    /// The row of numbers for these cells, sorted by column with no column
    /// given twice, if that is all there is to them.
    fn numbers_of(cells: &[(i32, Cell)]) -> Option<Row> {
        let first_column = match cells.first() {
            Some((column, Cell::NumberCell { .. })) => *column,
            _ => return None,
        };
        // The style most of them have, if most of them have one: the one
        // left standing when each is set against one that differs.
        let mut style = 0;
        let mut lead = 0;
        let mut values = Vec::with_capacity(cells.len());
        for (index, (column, cell)) in cells.iter().enumerate() {
            match cell {
                Cell::NumberCell { v, s }
                    if *column as i64 == first_column as i64 + index as i64 =>
                {
                    values.push(*v);
                    if lead == 0 {
                        style = *s;
                    }
                    lead += if *s == style { 1 } else { -1 };
                }
                _ => return None,
            }
        }
        let other_styles = cells
            .iter()
            .enumerate()
            .filter_map(|(index, (_, cell))| match cell {
                Cell::NumberCell { s, .. } if *s != style => Some((index as u32, *s)),
                _ => None,
            })
            .collect();
        Some(Row::Numbers {
            first_column,
            style,
            values,
            other_styles,
        })
    }

    /// The cells of the row with their column, each one made if need be.
    pub(super) fn to_cells(&self) -> Vec<(i32, Cell)> {
        self.iter()
            .map(|(column, cell)| (column, cell.into_owned()))
            .collect()
    }

    /// The cells of the row, to be changed: a row of numbers or of
    /// constants becomes a row of cells first.
    fn cells_mut(&mut self) -> &mut Vec<(i32, Cell)> {
        if !matches!(self, Row::Cells(_)) {
            *self = Row::Cells(self.to_cells());
        }
        match self {
            Row::Cells(cells) => cells,
            // It has just been made into cells
            Row::Numbers { .. } | Row::Constants { .. } => unreachable!(),
        }
    }

    /// The cell at a column, to be changed in place. A row of numbers becomes
    /// a row of cells first, if the cell is there at all.
    pub(super) fn get_mut(&mut self, column: i32) -> Option<&mut Cell> {
        // A row of numbers stays one if the cell is not there
        self.get(column)?;
        let cells = self.cells_mut();
        let index = position(cells, column).ok()?;
        Some(&mut cells[index].1)
    }

    pub(super) fn is_empty(&self) -> bool {
        match self {
            Row::Cells(cells) => cells.is_empty(),
            Row::Numbers { values, .. } => values.is_empty(),
            Row::Constants { cells, .. } => cells.is_empty(),
        }
    }

    /// The first and the last column that hold a cell, if any does.
    pub(super) fn columns(&self) -> Option<(i32, i32)> {
        match self {
            Row::Cells(cells) => Some((cells.first()?.0, cells.last()?.0)),
            Row::Numbers {
                first_column,
                values,
                ..
            } => {
                if values.is_empty() {
                    return None;
                }
                Some((*first_column, *first_column + values.len() as i32 - 1))
            }
            Row::Constants {
                first_column,
                cells,
            } => {
                if cells.is_empty() {
                    return None;
                }
                Some((*first_column, *first_column + cells.len() as i32 - 1))
            }
        }
    }

    /// Where the column is among the numbers of a row of numbers.
    #[inline]
    pub(super) fn offset(first_column: i32, values: &[f64], column: i32) -> Option<usize> {
        place(first_column, values.len(), column)
    }

    #[inline]
    pub(super) fn get(&self, column: i32) -> Option<Cow<'_, Cell>> {
        match self {
            Row::Cells(cells) => {
                let index = position(cells, column).ok()?;
                Some(Cow::Borrowed(&cells[index].1))
            }
            Row::Numbers {
                first_column,
                style,
                values,
                other_styles,
            } => {
                let offset = Row::offset(*first_column, values, column)?;
                Some(Cow::Owned(Cell::NumberCell {
                    v: values[offset],
                    s: style_at(*style, other_styles, offset),
                }))
            }
            Row::Constants {
                first_column,
                cells,
            } => {
                let offset = place(*first_column, cells.len(), column)?;
                cells[offset].to_cell().map(Cow::Owned)
            }
        }
    }

    #[inline]
    pub(super) fn peek(&self, column: i32) -> Peek<'_> {
        match self {
            Row::Cells(cells) => match position(cells, column) {
                Ok(index) => Peek::Cell(&cells[index].1),
                Err(_) => Peek::Nothing,
            },
            Row::Numbers {
                first_column,
                values,
                ..
            } => match Row::offset(*first_column, values, column) {
                Some(offset) => Peek::Number(values[offset]),
                None => Peek::Nothing,
            },
            Row::Constants {
                first_column,
                cells,
            } => match place(*first_column, cells.len(), column).map(|at| &cells[at]) {
                None | Some(Constant::Absent) => Peek::Nothing,
                Some(constant) => Peek::Constant(constant),
            },
        }
    }

    pub(super) fn iter(&self) -> RowIter<'_> {
        self.iter_from(i32::MIN)
    }

    /// The cells from `first_column` on.
    #[inline]
    pub(super) fn iter_from(&self, first_column: i32) -> RowIter<'_> {
        match self {
            Row::Cells(cells) => {
                let (Ok(start) | Err(start)) = position(cells, first_column);
                RowIter::Cells(cells[start..].iter())
            }
            Row::Numbers {
                first_column: row_first_column,
                style,
                values,
                other_styles,
            } => {
                let skipped = (first_column as i64 - *row_first_column as i64)
                    .clamp(0, values.len() as i64) as usize;
                let others_skipped =
                    other_styles.partition_point(|(at, _)| (*at as usize) < skipped);
                RowIter::Numbers {
                    column: *row_first_column + skipped as i32,
                    offset: skipped as u32,
                    style: *style,
                    values: values[skipped..].iter(),
                    other_styles: &other_styles[others_skipped..],
                }
            }
            Row::Constants {
                first_column: row_first_column,
                cells,
            } => {
                let skipped = (first_column as i64 - *row_first_column as i64)
                    .clamp(0, cells.len() as i64) as usize;
                RowIter::Constants {
                    column: *row_first_column + skipped as i32,
                    cells: cells[skipped..].iter(),
                }
            }
        }
    }

    pub(super) fn insert(&mut self, column: i32, cell: Cell) -> Option<Cell> {
        match self {
            Row::Numbers {
                first_column,
                style,
                values,
                other_styles,
            } => {
                // A number in the place of one of the same style, or one of
                // the style of the row right after the last: the row stays
                // what it is.
                if let Cell::NumberCell { v, s } = &cell {
                    if let Some(offset) = Row::offset(*first_column, values, column) {
                        if *s == style_at(*style, other_styles, offset) {
                            let old = std::mem::replace(&mut values[offset], *v);
                            return Some(Cell::NumberCell { v: old, s: *s });
                        }
                    } else if s == style
                        && column as i64 == *first_column as i64 + values.len() as i64
                    {
                        values.push(*v);
                        return None;
                    }
                }
                // Anything else that holds no formula makes it a row of
                // constants, if it can be one
                if Constant::of(&cell).is_some() {
                    if let Some(row) = Row::constants_of(&self.to_cells()) {
                        *self = row;
                        return self.insert(column, cell);
                    }
                }
            }
            Row::Constants {
                first_column,
                cells,
            } => {
                if let Some(constant) = Constant::of(&cell) {
                    if let Some(offset) = place(*first_column, cells.len(), column) {
                        return std::mem::replace(&mut cells[offset], constant).to_cell();
                    }
                    // Beyond either end, with a few gaps at most
                    let present = cells
                        .iter()
                        .filter(|c| !matches!(c, Constant::Absent))
                        .count();
                    let room = (present + 1) * (1 + GAPS_PER_CONSTANT);
                    let last_column = *first_column as i64 + cells.len() as i64 - 1;
                    if (column as i64) > last_column
                        && (column as i64 - *first_column as i64 + 1) as usize <= room
                    {
                        cells.resize((column - *first_column) as usize, Constant::Absent);
                        cells.push(constant);
                        return None;
                    }
                    if column < *first_column && (last_column - column as i64 + 1) as usize <= room
                    {
                        let gap = (*first_column - column) as usize;
                        cells.splice(
                            0..0,
                            std::iter::once(constant)
                                .chain(std::iter::repeat_n(Constant::Absent, gap - 1)),
                        );
                        *first_column = column;
                        return None;
                    }
                }
            }
            Row::Cells(cells) => {
                if cells.is_empty() {
                    if let Cell::NumberCell { v, s } = &cell {
                        *self = Row::Numbers {
                            first_column: column,
                            style: *s,
                            values: vec![*v],
                            other_styles: Box::default(),
                        };
                        return None;
                    }
                    if let Some(constant) = Constant::of(&cell) {
                        *self = Row::Constants {
                            first_column: column,
                            cells: vec![constant],
                        };
                        return None;
                    }
                }
            }
        }
        let cells = self.cells_mut();
        // Rows are mostly filled from left to right.
        if cells.last().is_none_or(|(last, _)| *last < column) {
            cells.push((column, cell));
            return None;
        }
        match position(cells, column) {
            Ok(index) => Some(std::mem::replace(&mut cells[index].1, cell)),
            Err(index) => {
                cells.insert(index, (column, cell));
                None
            }
        }
    }

    pub(super) fn remove(&mut self, column: i32) -> Option<Cell> {
        if let Row::Constants {
            first_column,
            cells,
        } = self
        {
            let offset = place(*first_column, cells.len(), column)?;
            let removed = std::mem::replace(&mut cells[offset], Constant::Absent).to_cell();
            // No gap at either end
            while matches!(cells.last(), Some(Constant::Absent)) {
                cells.pop();
            }
            let leading = cells
                .iter()
                .take_while(|c| matches!(c, Constant::Absent))
                .count();
            if leading > 0 {
                cells.drain(..leading);
                *first_column += leading as i32;
            }
            return removed;
        }
        if let Row::Numbers {
            first_column,
            style,
            values,
            other_styles,
        } = self
        {
            let offset = Row::offset(*first_column, values, column)?;
            // The last one, if it is of the style of the row
            let last = other_styles.last().map(|(at, _)| *at as usize);
            if offset + 1 == values.len() && last != Some(offset) {
                let v = values.pop()?;
                return Some(Cell::NumberCell { v, s: *style });
            }
        }
        let cells = self.cells_mut();
        let index = position(cells, column).ok()?;
        Some(cells.remove(index).1)
    }
}

/// See `SheetData::displace_columns`. The cells stay sorted: those that
/// move all move the same way, and land to the right of those that stay.
fn displace_cells(cells: &mut Vec<(i32, Cell)>, first_column: i32, delta: i32) {
    let deleted = (first_column + delta.min(0))..first_column;
    cells.retain(|(column, _)| !deleted.contains(column));
    for (column, _) in cells.iter_mut() {
        if *column >= first_column {
            *column += delta;
        }
    }
    cells.retain(|(column, _)| *column <= LAST_COLUMN);
}

impl Row {
    /// See `SheetData::displace_columns`. A row of numbers wholly to one side
    /// of `first_column` stays a row of numbers; one that straddles it is
    /// rebuilt from its cells.
    pub(super) fn displace_columns(&mut self, first_column: i32, delta: i32) {
        match self {
            Row::Cells(cells) => displace_cells(cells, first_column, delta),
            Row::Constants { .. } => {
                let mut cells = self.to_cells();
                displace_cells(&mut cells, first_column, delta);
                *self = Row::from_cells(cells);
            }
            Row::Numbers {
                first_column: row_first_column,
                values,
                ..
            } => {
                let last_column = *row_first_column + values.len() as i32 - 1;
                if last_column < first_column + delta.min(0) {
                    // Wholly to the left: nothing moves
                } else if *row_first_column >= first_column && last_column + delta <= LAST_COLUMN {
                    // Wholly to the right: slides as it is
                    *row_first_column += delta;
                } else {
                    let mut cells = self.to_cells();
                    displace_cells(&mut cells, first_column, delta);
                    *self = Row::from_cells(cells);
                }
            }
        }
    }
}

/// Two rows are equal when they have the same cells, however they hold them.
impl PartialEq for Row {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}
