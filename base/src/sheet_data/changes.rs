//! What has been written to a sheet since the last evaluation took notice.

/// The most separate areas kept before the record gives up and says that
/// anything may have changed. Writes come in runs (a row, a column, a pasted
/// block), and each run is one area.
const MOST_AREAS: usize = 4096;

/// A rectangle of cells of one sheet, both corners included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangedArea {
    pub row1: i32,
    pub column1: i32,
    pub row2: i32,
    pub column2: i32,
}

/// The cells written to a sheet since the changes were last taken, as areas
/// merged where they touch; or that anything may have changed, after rows or
/// columns were inserted, deleted or moved (every reference into the sheet
/// may have moved with them), a row was hidden or shown, or too many
/// separate areas were written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    pub(crate) areas: Vec<ChangedArea>,
    pub(crate) anything: bool,
}

impl Changes {
    /// Nothing was written.
    pub fn is_empty(&self) -> bool {
        !self.anything && self.areas.is_empty()
    }

    /// Anything may have changed.
    pub fn anything(&self) -> bool {
        self.anything
    }

    /// The areas written, when `anything` is false.
    pub fn areas(&self) -> &[ChangedArea] {
        &self.areas
    }

    pub(super) fn note(&mut self, row1: i32, column1: i32, row2: i32, column2: i32) {
        if self.anything {
            return;
        }
        if let Some(last) = self.areas.last_mut() {
            let rows_touch = row1 <= last.row2.saturating_add(1) && row2 >= last.row1 - 1;
            let columns_touch =
                column1 <= last.column2.saturating_add(1) && column2 >= last.column1 - 1;
            // The same columns in the next row, or the same rows in the next
            // column: one area
            if last.column1 == column1 && last.column2 == column2 && rows_touch {
                last.row1 = last.row1.min(row1);
                last.row2 = last.row2.max(row2);
                return;
            }
            if last.row1 == row1 && last.row2 == row2 && columns_touch {
                last.column1 = last.column1.min(column1);
                last.column2 = last.column2.max(column2);
                return;
            }
            if last.row1 <= row1
                && row2 <= last.row2
                && last.column1 <= column1
                && column2 <= last.column2
            {
                return;
            }
        }
        if self.areas.len() >= MOST_AREAS {
            self.note_anything();
            return;
        }
        self.areas.push(ChangedArea {
            row1,
            column1,
            row2,
            column2,
        });
    }

    pub(super) fn note_anything(&mut self) {
        self.anything = true;
        self.areas = Vec::new();
    }
}
