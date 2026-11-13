#![allow(clippy::unwrap_used)]

//! Images and charts over the cells: kept, stored, and moved with the rows,
//! columns and sheets they sit on and read.

use crate::drawings::{
    Anchor, CellPoint, Chart, ChartKind, ChartSeries, Drawing, DrawingKind, SeriesName, SheetArea,
};
use crate::test::util::new_empty_model;
use crate::Model;

fn area(sheet_id: u32, row: i32, column: i32, width: i32, height: i32) -> SheetArea {
    SheetArea {
        sheet_id,
        row,
        column,
        width,
        height,
    }
}

/// A chart of the amounts in B2:B6 by the names in A2:A6 of the first sheet,
/// over D2:J15.
fn chart(model: &Model) -> Drawing {
    let id = model.workbook.worksheets[0].sheet_id;
    let mut chart = Chart::new(ChartKind::Column);
    chart.title = Some("Amounts".to_string());
    chart.series.push(ChartSeries {
        name: Some(SeriesName::Cell(area(id, 1, 2, 1, 1))),
        categories: Some(area(id, 2, 1, 1, 5)),
        values: area(id, 2, 2, 1, 5),
        color: Some("4472C4".to_string()),
    });
    Drawing {
        anchor: Anchor::TwoCell {
            from: CellPoint::at(2, 4),
            to: CellPoint::at(15, 10),
        },
        name: "Chart 1".to_string(),
        description: String::new(),
        kind: DrawingKind::Chart(Box::new(chart)),
    }
}

fn series(model: &Model, sheet: u32) -> Vec<ChartSeries> {
    match &model.get_drawings(sheet).unwrap()[0].kind {
        DrawingKind::Chart(chart) => chart.series.clone(),
        DrawingKind::Image { .. } => Vec::new(),
    }
}

fn anchor(model: &Model, sheet: u32, index: usize) -> Anchor {
    model.get_drawings(sheet).unwrap()[index].anchor
}

#[test]
fn drawings_are_kept_and_stored() {
    let mut model = new_empty_model();
    let png = vec![0x89, b'P', b'N', b'G', 1, 2, 3];
    let media = model.add_media("image/png", png.clone()).unwrap();
    assert!(model.add_media("image/tiff", vec![]).is_err());
    let image = Drawing {
        anchor: Anchor::OneCell {
            from: CellPoint::at(1, 1),
            width: 200 * crate::drawings::EMU_PER_PIXEL,
            height: 100 * crate::drawings::EMU_PER_PIXEL,
        },
        name: "Picture 1".to_string(),
        description: "A logo".to_string(),
        kind: DrawingKind::Image { media },
    };
    assert_eq!(model.add_drawing(0, image.clone()).unwrap(), 0);
    let drawing = chart(&model);
    assert_eq!(model.add_drawing(0, drawing.clone()).unwrap(), 1);

    let read = Model::from_bytes(&model.to_bytes(), "en").unwrap();
    assert_eq!(
        read.get_drawings(0).unwrap(),
        [image.clone(), drawing.clone()]
    );
    assert_eq!(read.workbook.media[0].bytes, png);
    assert_eq!(read.workbook.media[0].name, "image1.png");

    // Replaced and removed
    let mut moved = drawing.clone();
    moved.name = "Chart 2".to_string();
    model.update_drawing(0, 1, moved.clone()).unwrap();
    assert_eq!(model.get_drawings(0).unwrap()[1], moved);
    assert_eq!(model.remove_drawing(0, 0).unwrap(), image);
    assert_eq!(model.get_drawings(0).unwrap().len(), 1);
    assert!(model.remove_drawing(0, 5).is_err());

    // What does not exist is refused
    let mut missing = image.clone();
    missing.kind = DrawingKind::Image { media: 7 };
    assert!(model.add_drawing(0, missing).is_err());
    let mut elsewhere = drawing;
    if let DrawingKind::Chart(chart) = &mut elsewhere.kind {
        chart.series[0].values.sheet_id = 99;
    }
    assert!(model.add_drawing(0, elsewhere).is_err());
}

#[test]
fn drawings_move_with_rows_and_columns() {
    let mut model = new_empty_model();
    let drawing = chart(&model);
    model.add_drawing(0, drawing).unwrap();
    let id = model.workbook.worksheets[0].sheet_id;

    // Rows inserted above everything: all moves down
    model.insert_rows(0, 1, 2).unwrap();
    assert_eq!(
        anchor(&model, 0, 0),
        Anchor::TwoCell {
            from: CellPoint::at(4, 4),
            to: CellPoint::at(17, 10)
        }
    );
    assert_eq!(series(&model, 0)[0].values, area(id, 4, 2, 1, 5));
    assert_eq!(
        series(&model, 0)[0].name,
        Some(SeriesName::Cell(area(id, 3, 2, 1, 1)))
    );

    // Inside the data: the ranges stretch, as references do
    model.insert_rows(0, 6, 3).unwrap();
    assert_eq!(series(&model, 0)[0].values, area(id, 4, 2, 1, 8));
    assert_eq!(series(&model, 0)[0].categories, Some(area(id, 4, 1, 1, 8)));

    // Deleted inside: they shrink
    model.delete_rows(0, 5, 4).unwrap();
    assert_eq!(series(&model, 0)[0].values, area(id, 4, 2, 1, 4));

    // A column inserted before the chart and after the data
    model.insert_columns(0, 3, 1).unwrap();
    assert_eq!(
        anchor(&model, 0, 0),
        Anchor::TwoCell {
            from: CellPoint::at(4, 5),
            to: CellPoint::at(16, 11)
        }
    );
    assert_eq!(series(&model, 0)[0].values, area(id, 4, 2, 1, 4));

    // The values' column deleted: the series goes
    model.delete_columns(0, 2, 1).unwrap();
    assert!(series(&model, 0).is_empty());
}

#[test]
fn ranges_into_other_sheets_follow_them() {
    let mut model = new_empty_model();
    model.add_sheet("Data").unwrap();
    let data_id = model.workbook.worksheets[1].sheet_id;
    let mut drawing = chart(&model);
    if let DrawingKind::Chart(chart) = &mut drawing.kind {
        chart.series[0].values = area(data_id, 2, 2, 1, 5);
        chart.series[0].categories = Some(area(data_id, 2, 1, 1, 5));
    }
    model.add_drawing(0, drawing).unwrap();

    // Rows of the data's sheet move its ranges, not the anchor
    model.insert_rows(1, 1, 1).unwrap();
    assert_eq!(series(&model, 0)[0].values, area(data_id, 3, 2, 1, 5));
    assert_eq!(
        anchor(&model, 0, 0),
        Anchor::TwoCell {
            from: CellPoint::at(2, 4),
            to: CellPoint::at(15, 10)
        }
    );
    // Rows of the chart's sheet move the anchor and the name it reads
    model.insert_rows(0, 1, 1).unwrap();
    assert_eq!(series(&model, 0)[0].values, area(data_id, 3, 2, 1, 5));
    assert_eq!(
        series(&model, 0)[0].name,
        Some(SeriesName::Cell(area(
            model.workbook.worksheets[0].sheet_id,
            2,
            2,
            1,
            1
        )))
    );

    // Moving sheets changes nothing; deleting the data's sheet drops the series
    model.move_sheet(1, 0).unwrap();
    assert_eq!(series(&model, 1)[0].values, area(data_id, 3, 2, 1, 5));
    model.delete_sheet(0).unwrap();
    assert!(series(&model, 0).is_empty());
}
