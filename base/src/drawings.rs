//! Images and charts over the cells of a sheet.
//!
//! A drawing sits over the cells, anchored to them as a workbook anchors
//! it: from one cell to another, moving and growing with the rows and
//! columns under it, or from one cell with a size of its own. An image is
//! bytes the workbook holds once (`Workbook::media`), however many drawings
//! show it. A chart is a business chart over ranges of cells: columns, bars,
//! lines, areas, a pie or a doughnut, or points.
//!
//! The ranges a chart reads name their sheet by `sheet_id`, which stays with
//! the sheet when sheets are moved, and move with the rows and columns of
//! that sheet as references in formulas do.

use bitcode::{Decode, Encode};

use crate::expressions::parser::stringify::DisplaceData;
use crate::model::Model;

/// English Metric Units in a pixel at 96 dots an inch: how offsets and sizes
/// are measured.
pub const EMU_PER_PIXEL: i64 = 9_525;

/// A point of a sheet: a cell, and how far into it, in EMU.
#[derive(Encode, Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPoint {
    pub row: i32,
    pub column: i32,
    pub row_offset: i64,
    pub column_offset: i64,
}

impl CellPoint {
    /// The top left corner of a cell.
    pub fn at(row: i32, column: i32) -> CellPoint {
        CellPoint {
            row,
            column,
            row_offset: 0,
            column_offset: 0,
        }
    }
}

/// Where a drawing is.
#[derive(Encode, Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// From one point to another: it moves and grows with the cells under it.
    TwoCell { from: CellPoint, to: CellPoint },
    /// From a point, with a size of its own in EMU: it moves with the cell
    /// under its corner.
    OneCell {
        from: CellPoint,
        width: i64,
        height: i64,
    },
}

/// A rectangle of cells of a sheet, the sheet named by its `sheet_id`.
#[derive(Encode, Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SheetArea {
    pub sheet_id: u32,
    pub row: i32,
    pub column: i32,
    pub width: i32,
    pub height: i32,
}

/// What a drawing shows.
#[derive(Encode, Decode, Debug, Clone, PartialEq)]
pub enum DrawingKind {
    /// An image: the index of its bytes in `Workbook::media`.
    Image {
        media: u32,
    },
    Chart(Box<Chart>),
}

/// An image or a chart over the cells of a sheet.
#[derive(Encode, Decode, Debug, Clone, PartialEq)]
pub struct Drawing {
    pub anchor: Anchor,
    /// Its name, unique in the sheet as a workbook names them.
    pub name: String,
    /// What it shows, for someone who cannot see it.
    pub description: String,
    pub kind: DrawingKind,
}

/// The bytes of an image, held once by the workbook.
#[derive(Encode, Decode, Debug, Clone, PartialEq, Eq)]
pub struct Media {
    /// A file name, with the extension its type has (`image1.png`).
    pub name: String,
    /// `image/png`, `image/jpeg` or `image/gif`.
    pub content_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Encode, Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartKind {
    /// Vertical bars.
    Column,
    /// Horizontal bars.
    Bar,
    Line,
    Area,
    Pie,
    Doughnut,
    /// Points with a number on each axis.
    Scatter,
}

/// How the series of a column, bar, line or area chart sit together.
#[derive(Encode, Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartGrouping {
    /// Side by side, or each line on its own.
    Standard,
    /// Each on top of the ones before.
    Stacked,
    /// On top of each other, as parts of a hundred percent.
    PercentStacked,
}

#[derive(Encode, Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegendPosition {
    Right,
    Left,
    Top,
    Bottom,
}

/// The name of a series: given, or the text of a cell.
#[derive(Encode, Decode, Debug, Clone, PartialEq, Eq)]
pub enum SeriesName {
    Text(String),
    Cell(SheetArea),
}

/// One series of a chart: its values, and the categories they are for (or,
/// for points, the numbers of the other axis).
#[derive(Encode, Decode, Debug, Clone, PartialEq, Eq)]
pub struct ChartSeries {
    pub name: Option<SeriesName>,
    pub categories: Option<SheetArea>,
    pub values: SheetArea,
    /// Its colour, `RRGGBB`; the theme's when none.
    pub color: Option<String>,
}

/// An axis of a chart.
#[derive(Encode, Decode, Debug, Clone, PartialEq, Default)]
pub struct ChartAxis {
    pub title: Option<String>,
    /// How its numbers are shown (`#,##0.00`); the cells' when none.
    pub number_format: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Encode, Decode, Debug, Clone, PartialEq)]
pub struct Chart {
    pub kind: ChartKind,
    pub grouping: ChartGrouping,
    pub title: Option<String>,
    /// Where its legend is; no legend when none.
    pub legend: Option<LegendPosition>,
    pub series: Vec<ChartSeries>,
    /// The axis of the categories, or of the first number of a point.
    pub category_axis: ChartAxis,
    pub value_axis: ChartAxis,
}

impl Chart {
    /// A chart of one kind with a legend on the right, no title and no
    /// series yet.
    pub fn new(kind: ChartKind) -> Chart {
        Chart {
            kind,
            grouping: ChartGrouping::Standard,
            title: None,
            legend: Some(LegendPosition::Right),
            series: Vec::new(),
            category_axis: ChartAxis::default(),
            value_axis: ChartAxis::default(),
        }
    }
}

/// Where a row or a column is after rows or columns are inserted, deleted or
/// moved; `None` if it was deleted.
fn displaced(index: i32, at: i32, delta: i32, moved: bool) -> Option<i32> {
    if moved {
        // One row or column moved by `delta`, the ones between going the
        // other way
        if index == at {
            Some(index + delta)
        } else if delta > 0 && index > at && index <= at + delta {
            Some(index - 1)
        } else if delta < 0 && index < at && index >= at + delta {
            Some(index + 1)
        } else {
            Some(index)
        }
    } else if index < at {
        Some(index)
    } else if delta < 0 && index < at - delta {
        None
    } else {
        Some(index + delta)
    }
}

/// A span `first..=last` after the displacement; `None` if all of it was
/// deleted. Inserted inside it, it grows; deleted inside it, it shrinks.
fn displaced_span(first: i32, last: i32, at: i32, delta: i32, moved: bool) -> Option<(i32, i32)> {
    if moved {
        let a = displaced(first, at, delta, true)?;
        let b = displaced(last, at, delta, true)?;
        return Some((a.min(b), a.max(b)));
    }
    if delta > 0 {
        // Inserted at `at`: what is at or after it moves
        let first = if first >= at { first + delta } else { first };
        let last = if last >= at { last + delta } else { last };
        return Some((first, last));
    }
    let first = match displaced(first, at, delta, false) {
        Some(row) => row,
        // Its first rows are gone: it starts where they were
        None => at,
    };
    let last = match displaced(last, at, delta, false) {
        Some(row) => row,
        None => at - 1,
    };
    if first > last {
        return None;
    }
    Some((first, last))
}

/// What a `DisplaceData` says, for drawings: the sheet, whether rows (or
/// columns), where, by how much, and whether it is a move.
fn decode(data: &DisplaceData) -> Option<(u32, bool, i32, i32, bool)> {
    match data {
        DisplaceData::Row { sheet, row, delta } => Some((*sheet, true, *row, *delta, false)),
        DisplaceData::Column {
            sheet,
            column,
            delta,
        } => Some((*sheet, false, *column, *delta, false)),
        DisplaceData::RowMove { sheet, row, delta } => Some((*sheet, true, *row, *delta, true)),
        DisplaceData::ColumnMove {
            sheet,
            column,
            delta,
        } => Some((*sheet, false, *column, *delta, true)),
        _ => None,
    }
}

impl SheetArea {
    /// The area after the displacement, `None` if it was deleted.
    fn displaced(&self, rows: bool, at: i32, delta: i32, moved: bool) -> Option<SheetArea> {
        let mut area = *self;
        if rows {
            let (first, last) =
                displaced_span(self.row, self.row + self.height - 1, at, delta, moved)?;
            area.row = first;
            area.height = last - first + 1;
        } else {
            let (first, last) =
                displaced_span(self.column, self.column + self.width - 1, at, delta, moved)?;
            area.column = first;
            area.width = last - first + 1;
        }
        Some(area)
    }
}

impl CellPoint {
    fn displaced(&self, rows: bool, at: i32, delta: i32, moved: bool, end: bool) -> CellPoint {
        let mut point = *self;
        let index = if rows { self.row } else { self.column };
        let new = displaced(index, at, delta, moved).unwrap_or(if end { at - 1 } else { at });
        if rows {
            point.row = new.max(1);
        } else {
            point.column = new.max(1);
        }
        point
    }
}

impl<'a> Model<'a> {
    /// The drawings of a sheet, in the order they are drawn.
    pub fn get_drawings(&self, sheet: u32) -> Result<&[Drawing], String> {
        Ok(&self.workbook.worksheet(sheet)?.drawings)
    }

    /// Adds a drawing over a sheet, drawn over the others, and returns its
    /// index.
    pub fn add_drawing(&mut self, sheet: u32, drawing: Drawing) -> Result<usize, String> {
        self.check_drawing(&drawing)?;
        let drawings = &mut self.workbook.worksheet_mut(sheet)?.drawings;
        drawings.push(drawing);
        Ok(drawings.len() - 1)
    }

    /// Replaces a drawing.
    pub fn update_drawing(
        &mut self,
        sheet: u32,
        index: usize,
        drawing: Drawing,
    ) -> Result<(), String> {
        self.check_drawing(&drawing)?;
        let slot = self
            .workbook
            .worksheet_mut(sheet)?
            .drawings
            .get_mut(index)
            .ok_or_else(|| format!("No drawing {index} in sheet {sheet}"))?;
        *slot = drawing;
        Ok(())
    }

    /// Removes a drawing and returns it.
    pub fn remove_drawing(&mut self, sheet: u32, index: usize) -> Result<Drawing, String> {
        let drawings = &mut self.workbook.worksheet_mut(sheet)?.drawings;
        if index >= drawings.len() {
            return Err(format!("No drawing {index} in sheet {sheet}"));
        }
        Ok(drawings.remove(index))
    }

    /// Adds the bytes of an image to the workbook and returns the index an
    /// image drawing shows it by. `content_type` is `image/png`,
    /// `image/jpeg` or `image/gif`.
    pub fn add_media(&mut self, content_type: &str, bytes: Vec<u8>) -> Result<u32, String> {
        let extension = match content_type {
            "image/png" => "png",
            "image/jpeg" => "jpeg",
            "image/gif" => "gif",
            other => return Err(format!("Images of type '{other}' are not supported")),
        };
        let media = &mut self.workbook.media;
        let index = media.len() as u32;
        media.push(Media {
            name: format!("image{}.{extension}", index + 1),
            content_type: content_type.to_string(),
            bytes,
        });
        Ok(index)
    }

    fn check_drawing(&self, drawing: &Drawing) -> Result<(), String> {
        match &drawing.kind {
            DrawingKind::Image { media } => {
                if *media as usize >= self.workbook.media.len() {
                    return Err(format!("No image {media} in the workbook"));
                }
            }
            DrawingKind::Chart(chart) => {
                let known = |area: &SheetArea| {
                    self.workbook
                        .worksheets
                        .iter()
                        .any(|worksheet| worksheet.sheet_id == area.sheet_id)
                        && area.row >= 1
                        && area.column >= 1
                        && area.width >= 1
                        && area.height >= 1
                };
                for series in &chart.series {
                    let name = match &series.name {
                        Some(SeriesName::Cell(area)) => Some(area),
                        _ => None,
                    };
                    if !known(&series.values)
                        || series.categories.as_ref().is_some_and(|a| !known(a))
                        || name.is_some_and(|a| !known(a))
                    {
                        return Err("A series reads cells outside the workbook".to_string());
                    }
                }
            }
        }
        Ok(())
    }

    /// Moves the drawings and the ranges of the charts with rows or columns
    /// inserted, deleted or moved: the drawings of that sheet, and the
    /// ranges into it from any sheet. A series whose values are all deleted
    /// goes.
    pub(crate) fn displace_drawings(&mut self, data: &DisplaceData) {
        let Some((sheet, rows, at, delta, moved)) = decode(data) else {
            return;
        };
        let Some(sheet_id) = self
            .workbook
            .worksheets
            .get(sheet as usize)
            .map(|w| w.sheet_id)
        else {
            return;
        };
        for (index, worksheet) in self.workbook.worksheets.iter_mut().enumerate() {
            let here = index as u32 == sheet;
            for drawing in &mut worksheet.drawings {
                if here {
                    drawing.anchor = match drawing.anchor {
                        Anchor::TwoCell { from, to } => {
                            let from = from.displaced(rows, at, delta, moved, false);
                            let mut to = to.displaced(rows, at, delta, moved, true);
                            to.row = to.row.max(from.row);
                            to.column = to.column.max(from.column);
                            Anchor::TwoCell { from, to }
                        }
                        Anchor::OneCell {
                            from,
                            width,
                            height,
                        } => Anchor::OneCell {
                            from: from.displaced(rows, at, delta, moved, false),
                            width,
                            height,
                        },
                    };
                }
                let DrawingKind::Chart(chart) = &mut drawing.kind else {
                    continue;
                };
                let move_area = |area: &SheetArea| -> Option<SheetArea> {
                    if area.sheet_id == sheet_id {
                        area.displaced(rows, at, delta, moved)
                    } else {
                        Some(*area)
                    }
                };
                chart.series.retain_mut(|series| {
                    let Some(values) = move_area(&series.values) else {
                        return false;
                    };
                    series.values = values;
                    series.categories = series.categories.as_ref().and_then(move_area);
                    if let Some(SeriesName::Cell(area)) = &series.name {
                        series.name = move_area(area).map(SeriesName::Cell);
                    }
                    true
                });
            }
        }
    }

    /// Drops what read a sheet that is gone: the series of the charts of
    /// the other sheets that read it.
    pub(crate) fn forget_sheet_in_drawings(&mut self, sheet_id: u32) {
        for worksheet in &mut self.workbook.worksheets {
            for drawing in &mut worksheet.drawings {
                let DrawingKind::Chart(chart) = &mut drawing.kind else {
                    continue;
                };
                chart.series.retain_mut(|series| {
                    if series.values.sheet_id == sheet_id {
                        return false;
                    }
                    if series.categories.is_some_and(|a| a.sheet_id == sheet_id) {
                        series.categories = None;
                    }
                    if matches!(&series.name, Some(SeriesName::Cell(a)) if a.sheet_id == sheet_id) {
                        series.name = None;
                    }
                    true
                });
            }
        }
    }
}
