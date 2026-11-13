//! The rows of a sheet, by position: chunks of consecutive rows, allocated as
//! they are needed.

use std::borrow::Cow;
use std::sync::OnceLock;

use bitcode::{Decode, Encode};

use super::changes::Changes;
use super::row::{Constant, Peek, Row};
use crate::constants::LAST_ROW;
use crate::types::Cell;

const CHUNK_BITS: u32 = 10;
const CHUNK_ROWS: usize = 1 << CHUNK_BITS;
const CHUNK_MASK: usize = CHUNK_ROWS - 1;

/// The cells of a worksheet, by row and column. Both are 1-indexed.
///
/// A row can exist without cells: `set_row` with no cells creates one. The xlsx
/// importer does that for rows that carry a height or a style and nothing
/// else, and the exporter writes a `<row>` element for every row that exists
/// here, so such rows survive a round trip. Removing the last cell of a row
/// with `remove_cell` removes the row.
///
/// Rows outside the sheet, below 1 or beyond the last row, cannot hold cells:
/// writing to them does nothing and reading them finds nothing.
#[derive(Encode, Decode, Debug, Clone, Default)]
pub struct SheetData {
    /// `chunks[i]` holds rows `i * CHUNK_ROWS ..`, if any of them exists.
    chunks: Vec<Option<Box<Chunk>>>,
    /// How many rows exist.
    row_count: usize,
    /// The used area, once someone has asked for it: see `used_area`. Not
    /// saved; it is found again from the rows.
    #[bitcode(skip)]
    used_area: OnceLock<Option<UsedArea>>,
    /// What has been written since the changes were last taken: see
    /// `take_changes`. Not saved.
    #[bitcode(skip)]
    changes: Changes,
}

/// The smallest rectangle that holds every cell of a sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsedArea {
    pub min_row: i32,
    pub max_row: i32,
    pub min_column: i32,
    pub max_column: i32,
}

impl UsedArea {
    fn include(&mut self, row: i32, first_column: i32, last_column: i32) {
        self.min_row = self.min_row.min(row);
        self.max_row = self.max_row.max(row);
        self.min_column = self.min_column.min(first_column);
        self.max_column = self.max_column.max(last_column);
    }
}

#[derive(Encode, Decode, Debug, Clone)]
struct Chunk {
    /// Always `CHUNK_ROWS` long.
    rows: Vec<Option<Row>>,
    /// How many of them exist. A chunk with none is freed.
    used: usize,
}

impl Chunk {
    fn new() -> Self {
        let mut rows = Vec::new();
        rows.resize_with(CHUNK_ROWS, || None);
        Chunk { rows, used: 0 }
    }
}

/// The chunk and the place in it of a row, if the row is within the sheet.
#[inline]
fn locate(row: i32) -> Option<(usize, usize)> {
    if !(1..=LAST_ROW).contains(&row) {
        return None;
    }
    let row = row as usize;
    Some((row >> CHUNK_BITS, row & CHUNK_MASK))
}

impl SheetData {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub(super) fn row(&self, row: i32) -> Option<&Row> {
        let (chunk, index) = locate(row)?;
        self.chunks.get(chunk)?.as_ref()?.rows[index].as_ref()
    }

    /// The row, to be changed. Whoever adds a cell to it or takes one away
    /// says so to the used area: `grow_used_area`, `forget_used_area`.
    #[inline]
    fn row_mut(&mut self, row: i32) -> Option<&mut Row> {
        let (chunk, index) = locate(row)?;
        self.chunks.get_mut(chunk)?.as_mut()?.rows[index].as_mut()
    }

    /// The smallest rectangle that holds every cell of the sheet, `None` if
    /// it has no cells. A row without cells is not part of it.
    ///
    /// It is found from the first and the last cell of every row, with no
    /// walk over the cells, and kept: a cell put on the sheet stretches it,
    /// and it is only found again after cells were removed. Functions ask
    /// for it on every evaluation, to walk a whole column or a whole row no
    /// further than the sheet goes.
    pub fn used_area(&self) -> Option<UsedArea> {
        *self.used_area.get_or_init(|| {
            let mut area: Option<UsedArea> = None;
            for (row, row_data) in self.rows_in_order() {
                let Some((first_column, last_column)) = row_data.columns() else {
                    continue;
                };
                match &mut area {
                    Some(area) => area.include(row, first_column, last_column),
                    None => {
                        area = Some(UsedArea {
                            min_row: row,
                            max_row: row,
                            min_column: first_column,
                            max_column: last_column,
                        })
                    }
                }
            }
            area
        })
    }

    /// A cell was put at a position: the used area, if it is known, holds it.
    fn grow_used_area(&mut self, row: i32, column: i32) {
        match self.used_area.get_mut() {
            Some(Some(area)) => area.include(row, column, column),
            // No cells before this one
            Some(None) => self.forget_used_area(),
            None => {}
        }
    }

    /// Cells were removed or moved: the used area has to be found again.
    fn forget_used_area(&mut self) {
        self.used_area = OnceLock::new();
    }

    /// The row, created if it does not exist. `None` outside the sheet.
    fn row_or_insert(&mut self, row: i32) -> Option<&mut Row> {
        let (chunk, index) = locate(row)?;
        if chunk >= self.chunks.len() {
            self.chunks.resize_with(chunk + 1, || None);
        }
        let chunk = self.chunks[chunk].get_or_insert_with(|| Box::new(Chunk::new()));
        if chunk.rows[index].is_none() {
            chunk.rows[index] = Some(Row::default());
            chunk.used += 1;
            self.row_count += 1;
        }
        chunk.rows[index].as_mut()
    }

    /// Every row that exists with its number, in ascending order.
    fn rows_in_order(&self) -> impl Iterator<Item = (i32, &Row)> {
        self.chunks
            .iter()
            .enumerate()
            .filter_map(|(chunk_index, chunk)| Some((chunk_index, chunk.as_ref()?)))
            .flat_map(|(chunk_index, chunk)| {
                chunk
                    .rows
                    .iter()
                    .enumerate()
                    .filter_map(move |(index, row)| {
                        let number = (chunk_index << CHUNK_BITS) | index;
                        Some((number as i32, row.as_ref()?))
                    })
            })
    }

    /// The cell at a position, if there is one.
    #[inline]
    pub fn cell(&self, row: i32, column: i32) -> Option<Cow<'_, Cell>> {
        self.row(row)?.get(column)
    }

    /// The cell at a position, to be changed in place.
    pub fn cell_mut(&mut self, row: i32, column: i32) -> Option<&mut Cell> {
        self.changes.note(row, column, row, column);
        self.row_mut(row)?.get_mut(column)
    }

    /// Puts a cell at a position and returns the one that was there.
    pub fn set_cell(&mut self, row: i32, column: i32, cell: Cell) -> Option<Cell> {
        let old = self.row_or_insert(row)?.insert(column, cell);
        self.changes.note(row, column, row, column);
        if old.is_none() {
            self.grow_used_area(row, column);
        }
        old
    }

    /// Removes the cell at a position and returns it.
    pub fn remove_cell(&mut self, row: i32, column: i32) -> Option<Cell> {
        let row_data = self.row_mut(row)?;
        let cell = row_data.remove(column)?;
        if row_data.is_empty() {
            self.take_row(row);
        }
        self.changes.note(row, column, row, column);
        self.forget_used_area();
        Some(cell)
    }

    /// True if the sheet has no rows.
    pub fn is_empty(&self) -> bool {
        self.row_count == 0
    }

    /// Every cell of the sheet with its row and column, in natural order:
    /// by row, and within a row by column.
    pub fn cells(&self) -> impl Iterator<Item = (i32, i32, Cow<'_, Cell>)> {
        self.rows_in_order().flat_map(|(row, row_data)| {
            row_data
                .iter()
                .map(move |(column, cell)| (row, column, cell))
        })
    }

    /// The rows that exist, in ascending order.
    pub fn rows(&self) -> Vec<i32> {
        self.rows_in_order().map(|(row, _)| row).collect()
    }

    /// The columns of a row that have cells, in ascending order.
    pub fn columns_in_row(&self, row: i32) -> Vec<i32> {
        match self.row(row) {
            Some(row_data) => row_data.iter().map(|(column, _)| column).collect(),
            None => Vec::new(),
        }
    }

    /// The cells of a row with their column, in ascending order of column.
    pub fn cells_in_row(&self, row: i32) -> Vec<(i32, Cow<'_, Cell>)> {
        match self.row(row) {
            Some(row_data) => row_data.iter().collect(),
            None => Vec::new(),
        }
    }

    /// The cells of a row from `first_column` to `last_column`, both included,
    /// with their column, in ascending order of column. Reading a stretch of a
    /// row this way finds the row once, not once per cell.
    pub fn cells_in_row_between(
        &self,
        row: i32,
        first_column: i32,
        last_column: i32,
    ) -> impl Iterator<Item = (i32, Cow<'_, Cell>)> {
        self.row(row)
            .map(|row_data| row_data.iter_from(first_column))
            .into_iter()
            .flatten()
            .take_while(move |(column, _)| *column <= last_column)
    }

    /// The numbers of a row from `first_column` to `last_column`, both
    /// included, if the row holds them as numbers and nothing else, with none
    /// missing. `None` is no news about the row: its cells say what it holds.
    #[inline]
    pub fn numbers_in_row_between(
        &self,
        row: i32,
        first_column: i32,
        last_column: i32,
    ) -> Option<&[f64]> {
        match self.row(row)? {
            Row::Numbers {
                first_column: row_first_column,
                values,
                ..
            } => {
                let first = Row::offset(*row_first_column, values, first_column)?;
                let last = Row::offset(*row_first_column, values, last_column)?;
                values.get(first..=last)
            }
            Row::Cells(_) | Row::Constants { .. } => None,
        }
    }

    /// The memory the cells of the sheet take, in bytes: the rows and what
    /// they hold, but not the texts they show, which are the workbook's
    /// shared strings. Found from the sizes of the rows, not cell by cell
    /// except in rows of cells that may hold text of their own. What the
    /// allocator adds to each allocation is left out.
    pub fn heap_size(&self) -> usize {
        let chunks = self.chunks.capacity() * std::mem::size_of::<Option<Box<Chunk>>>();
        let rows: usize = self
            .chunks
            .iter()
            .flatten()
            .map(|chunk| {
                std::mem::size_of::<Chunk>()
                    + chunk.rows.capacity() * std::mem::size_of::<Option<Row>>()
                    + chunk
                        .rows
                        .iter()
                        .flatten()
                        .map(Row::heap_size)
                        .sum::<usize>()
            })
            .sum();
        chunks + rows
    }

    /// What is at a position, for a quick look: the constant itself if the
    /// row holds the position as one, the cell borrowed if it holds it as a
    /// cell. Unlike `cell` it never makes a cell to hand out.
    #[inline]
    pub(crate) fn peek(&self, row: i32, column: i32) -> Peek<'_> {
        match self.row(row) {
            Some(row_data) => row_data.peek(column),
            None => Peek::Nothing,
        }
    }

    /// The constants of a row from `first_column` to `last_column`, if the
    /// row holds nothing but constants, with the column of the first of them;
    /// a gap is `Constant::Absent`. The columns of the stretch beyond the
    /// slice, on either side, hold no cell. `None` is no news about the row:
    /// its cells say what it holds.
    #[inline]
    pub(crate) fn constants_in_row_between(
        &self,
        row: i32,
        first_column: i32,
        last_column: i32,
    ) -> Option<(i32, &[Constant])> {
        match self.row(row)? {
            Row::Constants {
                first_column: row_first_column,
                cells,
            } => {
                let row_last_column = *row_first_column as i64 + cells.len() as i64 - 1;
                let from = (first_column as i64).max(*row_first_column as i64);
                let to = (last_column as i64).min(row_last_column);
                if from > to {
                    return Some((first_column, &[]));
                }
                let start = (from - *row_first_column as i64) as usize;
                let end = (to - *row_first_column as i64) as usize;
                Some((from as i32, &cells[start..=end]))
            }
            Row::Cells(_) | Row::Numbers { .. } => None,
        }
    }

    /// Removes every cell of a row, and the row.
    pub fn remove_row(&mut self, row: i32) {
        if let Some((first, last)) = self.row(row).and_then(Row::columns) {
            self.changes.note(row, first, row, last);
        }
        self.take_row(row);
    }

    /// Takes a row out of the sheet. The row no longer exists afterwards.
    fn take_row(&mut self, row: i32) -> Option<Row> {
        let (chunk_index, index) = locate(row)?;
        let chunk = self.chunks.get_mut(chunk_index)?.as_mut()?;
        let taken = chunk.rows[index].take()?;
        self.used_area = OnceLock::new();
        chunk.used -= 1;
        self.row_count -= 1;
        if chunk.used == 0 {
            self.chunks[chunk_index] = None;
        }
        Some(taken)
    }

    /// Puts a row in the sheet in place of whatever was there. Outside the
    /// sheet it is dropped.
    fn put_row(&mut self, row: i32, data: Row) {
        if let Some(slot) = self.row_or_insert(row) {
            *slot = data;
        }
        self.forget_used_area();
    }

    /// Moves every row from `first_row` on by `delta` rows: row `r` becomes
    /// row `r + delta`, cells and all. This is what inserting rows above
    /// `first_row` does, and deleting them: with a negative `delta` the rows
    /// the others land on, `first_row + delta` to `first_row - 1`, are removed
    /// first. A row that would land beyond the last row is dropped. The sheet
    /// is not read cell by cell: a row moves in one piece.
    pub fn displace_rows(&mut self, first_row: i32, delta: i32) {
        if delta == 0 {
            return;
        }
        self.changes.note_anything();
        let rows: Vec<i32> = self
            .rows_in_order()
            .map(|(row, _)| row)
            .filter(|row| *row >= first_row + delta.min(0))
            .collect();
        if delta > 0 {
            // From the bottom up, so that a row lands on a row already moved
            for row in rows.into_iter().rev() {
                if let Some(data) = self.take_row(row) {
                    self.put_row(row + delta, data);
                }
            }
        } else {
            for row in rows {
                if row < first_row {
                    self.take_row(row);
                } else if let Some(data) = self.take_row(row) {
                    self.put_row(row + delta, data);
                }
            }
        }
    }

    /// Moves every cell from `first_column` on by `delta` columns, in every
    /// row: the cell in column `c` goes to column `c + delta`. This is what
    /// inserting columns before `first_column` does, and deleting them: with a
    /// negative `delta` the cells in the columns the others land on,
    /// `first_column + delta` to `first_column - 1`, are removed first. A row
    /// left with no cells by that is removed, as it is when its last cell is
    /// removed. A cell that would land beyond the last column is dropped.
    pub fn displace_columns(&mut self, first_column: i32, delta: i32) {
        if delta == 0 {
            return;
        }
        self.changes.note_anything();
        self.forget_used_area();
        let mut emptied = Vec::new();
        for (chunk_index, chunk) in self.chunks.iter_mut().enumerate() {
            let Some(chunk) = chunk else {
                continue;
            };
            for (index, row) in chunk.rows.iter_mut().enumerate() {
                let Some(row_data) = row else {
                    continue;
                };
                let was_empty = row_data.is_empty();
                row_data.displace_columns(first_column, delta);
                if !was_empty && row_data.is_empty() {
                    emptied.push(((chunk_index << CHUNK_BITS) | index) as i32);
                }
            }
        }
        for row in emptied {
            self.take_row(row);
        }
    }

    /// Replaces the cells of a row. The row exists afterwards, even with no
    /// cells. If a column is given twice the last one stays.
    pub fn set_row(&mut self, row: i32, cells: impl IntoIterator<Item = (i32, Cell)>) {
        let before = self.row(row).and_then(Row::columns);
        let Some(row_data) = self.row_or_insert(row) else {
            return;
        };
        // Collected at its exact size: a row grown cell by cell from empty
        // starts with room for four, and a sheet with a million short rows
        // paid for that four times over.
        let mut cells: Vec<(i32, Cell)> = cells.into_iter().collect();
        // A file gives the cells of a row from left to right, and then there
        // is nothing to sort and no column given twice.
        if !cells.is_sorted_by(|earlier, later| earlier.0 < later.0) {
            cells.sort_by_key(|(column, _)| *column);
            // Of two cells in the same column the later one stays: it is moved
            // into the slot of the earlier one, which `dedup_by` then keeps.
            cells.dedup_by(|later, earlier| {
                if later.0 == earlier.0 {
                    std::mem::swap(&mut later.1, &mut earlier.1);
                    true
                } else {
                    false
                }
            });
        }
        cells.shrink_to_fit();
        *row_data = Row::from_cells(cells);
        // What was there and what is there now
        let after = row_data.columns();
        let span = match (before, after) {
            (Some((a1, a2)), Some((b1, b2))) => Some((a1.min(b1), a2.max(b2))),
            (span, None) | (None, span) => span,
        };
        if let Some((first, last)) = span {
            self.changes.note(row, first, row, last);
        }
        self.forget_used_area();
    }

    /// Hands the index of every shared string the sheet shows to `f`.
    pub fn for_each_shared_string(&self, mut f: impl FnMut(i32)) {
        for chunk in self.chunks.iter().flatten() {
            for row in chunk.rows.iter().flatten() {
                row.for_each_shared_string(&mut f);
            }
        }
    }

    /// Gives every shared string the sheet shows its new index,
    /// `renumbered[old]`. What the cells show does not change, so it is not
    /// recorded as written.
    pub fn renumber_shared_strings(&mut self, renumbered: &[i32]) {
        for chunk in self.chunks.iter_mut().flatten() {
            for row in chunk.rows.iter_mut().flatten() {
                row.renumber_shared_strings(renumbered);
            }
        }
    }

    /// What has been written to the sheet since the last call, and starts
    /// a new record. An evaluation takes the changes to know what to
    /// evaluate again, and takes them once more at its end, so that what it
    /// wrote itself does not count.
    pub fn take_changes(&mut self) -> Changes {
        std::mem::take(&mut self.changes)
    }

    /// Says that anything in the sheet may have changed, for a change the
    /// cells do not show: a row hidden or shown (SUBTOTAL leaves hidden rows
    /// out).
    pub fn note_anything_changed(&mut self) {
        self.changes.note_anything();
    }
}

/// Two sheets are equal when they have the same rows and the same cells.
/// Which chunks happen to be allocated is not part of that.
impl PartialEq for SheetData {
    fn eq(&self, other: &Self) -> bool {
        self.row_count == other.row_count && self.rows_in_order().eq(other.rows_in_order())
    }
}
