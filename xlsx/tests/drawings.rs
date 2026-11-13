#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::panic)]

//! Images and charts written to xlsx: the parts, their relationships, and
//! XML that reads. `opens_in_libreoffice`, ignored, converts the workbook
//! with LibreOffice where it is installed.

use std::io::{Cursor, Read};

use ironcalc::base::drawings::{
    Anchor, CellPoint, Chart, ChartGrouping, ChartKind, ChartSeries, Drawing, DrawingKind,
    LegendPosition, SeriesName, SheetArea,
};
use ironcalc::base::Model;
use ironcalc::export::save_xlsx_to_writer;

/// A PNG of one transparent pixel.
const PIXEL: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

const KINDS: [ChartKind; 7] = [
    ChartKind::Column,
    ChartKind::Bar,
    ChartKind::Line,
    ChartKind::Area,
    ChartKind::Pie,
    ChartKind::Doughnut,
    ChartKind::Scatter,
];

/// Sales by region on a sheet named with a space, a chart of each kind on
/// the first sheet, and an image.
fn workbook() -> Model<'static> {
    let mut model = Model::new_empty("charts", "en", "UTC", "en").unwrap();
    model.add_sheet("Sales data").unwrap();
    model.set_user_input(1, 1, 1, "Region".to_string()).unwrap();
    model.set_user_input(1, 1, 2, "Q1".to_string()).unwrap();
    model.set_user_input(1, 1, 3, "Q2".to_string()).unwrap();
    for (row, (region, q1, q2)) in [("North", 10, 12), ("South", 7, 9), ("East", 13, 8)]
        .iter()
        .enumerate()
    {
        let row = row as i32 + 2;
        model.set_user_input(1, row, 1, region.to_string()).unwrap();
        model.set_user_input(1, row, 2, q1.to_string()).unwrap();
        model.set_user_input(1, row, 3, q2.to_string()).unwrap();
    }
    model.evaluate();
    let data = model.workbook.worksheets[1].sheet_id;
    let area = |row, column, height| SheetArea {
        sheet_id: data,
        row,
        column,
        width: 1,
        height,
    };
    for (index, kind) in KINDS.iter().enumerate() {
        let mut chart = Chart::new(*kind);
        chart.title = Some(format!("Sales {kind:?} & more"));
        chart.legend = Some(LegendPosition::Bottom);
        if *kind == ChartKind::Column {
            chart.grouping = ChartGrouping::Stacked;
        }
        chart.value_axis.number_format = Some("#,##0".to_string());
        chart.value_axis.min = Some(0.0);
        for column in [2, 3] {
            chart.series.push(ChartSeries {
                name: Some(SeriesName::Cell(area(1, column, 1))),
                categories: Some(area(2, if *kind == ChartKind::Scatter { 2 } else { 1 }, 3)),
                values: area(2, column, 3),
                color: (column == 2).then(|| "4472C4".to_string()),
            });
        }
        let row = 1 + 16 * index as i32;
        model
            .add_drawing(
                0,
                Drawing {
                    anchor: Anchor::TwoCell {
                        from: CellPoint::at(row, 2),
                        to: CellPoint::at(row + 14, 9),
                    },
                    name: format!("Chart {}", index + 1),
                    description: String::new(),
                    kind: DrawingKind::Chart(Box::new(chart)),
                },
            )
            .unwrap();
    }
    let media = model.add_media("image/png", PIXEL.to_vec()).unwrap();
    model
        .add_drawing(
            1,
            Drawing {
                anchor: Anchor::OneCell {
                    from: CellPoint::at(1, 6),
                    width: 64 * 9_525,
                    height: 64 * 9_525,
                },
                name: "Picture 1".to_string(),
                description: "A pixel".to_string(),
                kind: DrawingKind::Image { media },
            },
        )
        .unwrap();
    model
}

fn parts(model: &Model) -> Vec<(String, Vec<u8>)> {
    let bytes = save_xlsx_to_writer(model, Cursor::new(Vec::new()))
        .unwrap()
        .into_inner();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..archive.len())
        .map(|index| {
            let mut file = archive.by_index(index).unwrap();
            let mut content = Vec::new();
            file.read_to_end(&mut content).unwrap();
            (file.name().unwrap().to_string(), content)
        })
        .filter(|(name, _)| !name.ends_with('/'))
        .collect()
}

fn part<'a>(parts: &'a [(String, Vec<u8>)], name: &str) -> &'a str {
    let (_, content) = parts
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("no part {name}"));
    std::str::from_utf8(content).unwrap()
}

/// Reads an XML part to its end.
fn well_formed(name: &str, xml: &str) {
    let mut reader = quick_xml::Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(_) => {}
            Err(error) => panic!("{name}: {error}"),
        }
    }
}

#[test]
fn charts_and_images_are_written_with_their_parts() {
    let model = workbook();
    let parts = parts(&model);
    for (name, content) in &parts {
        if name.ends_with(".xml") || name.ends_with(".rels") {
            well_formed(name, std::str::from_utf8(content).unwrap());
        }
    }
    // The first sheet draws the charts, the second the image
    let sheet1 = part(&parts, "xl/worksheets/sheet1.xml");
    assert!(sheet1.contains("<drawing r:id=\"rId1\"/>"));
    assert!(
        part(&parts, "xl/worksheets/_rels/sheet1.xml.rels").contains("../drawings/drawing1.xml")
    );
    assert!(
        part(&parts, "xl/worksheets/_rels/sheet2.xml.rels").contains("../drawings/drawing2.xml")
    );
    let drawing1 = part(&parts, "xl/drawings/drawing1.xml");
    assert_eq!(drawing1.matches("<xdr:twoCellAnchor>").count(), 7);
    assert!(drawing1
        .contains("<xdr:from><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>0</xdr:row>"));
    let rels1 = part(&parts, "xl/drawings/_rels/drawing1.xml.rels");
    for number in 1..=7 {
        assert!(rels1.contains(&format!("../charts/chart{number}.xml")));
    }
    let drawing2 = part(&parts, "xl/drawings/drawing2.xml");
    assert!(drawing2.contains("<xdr:oneCellAnchor>"));
    assert!(drawing2.contains("descr=\"A pixel\""));
    assert!(part(&parts, "xl/drawings/_rels/drawing2.xml.rels").contains("../media/image1.png"));
    assert!(parts
        .iter()
        .any(|(name, bytes)| name == "xl/media/image1.png" && bytes == PIXEL));

    // Each chart reads the ranges of the data sheet
    let column = part(&parts, "xl/charts/chart1.xml");
    assert!(column.contains("<c:barDir val=\"col\"/><c:grouping val=\"stacked\"/>"));
    assert!(column.contains("<c:f>&apos;Sales data&apos;!$B$2:$B$4</c:f>"));
    assert!(column.contains("<c:f>&apos;Sales data&apos;!$A$2:$A$4</c:f>"));
    assert!(column.contains("<c:tx><c:strRef><c:f>&apos;Sales data&apos;!$B$1</c:f>"));
    assert!(column.contains("Sales Column &amp; more"));
    assert!(column.contains("<a:srgbClr val=\"4472C4\"/>"));
    assert!(column.contains("<c:numFmt formatCode=\"#,##0\" sourceLinked=\"0\"/>"));
    assert!(part(&parts, "xl/charts/chart2.xml").contains("<c:barDir val=\"bar\"/>"));
    assert!(part(&parts, "xl/charts/chart3.xml").contains("<c:lineChart>"));
    assert!(part(&parts, "xl/charts/chart4.xml").contains("<c:areaChart>"));
    assert!(!part(&parts, "xl/charts/chart5.xml").contains("<c:valAx>"));
    assert!(part(&parts, "xl/charts/chart6.xml").contains("<c:holeSize val=\"50\"/>"));
    let scatter = part(&parts, "xl/charts/chart7.xml");
    assert!(scatter.contains("<c:xVal><c:numRef><c:f>&apos;Sales data&apos;!$B$2:$B$4</c:f>"));

    // The types of the parts
    let types = part(&parts, "[Content_Types].xml");
    assert!(types.contains("<Default Extension=\"png\" ContentType=\"image/png\"/>"));
    assert!(types.contains("/xl/drawings/drawing2.xml"));
    assert!(types.contains("/xl/charts/chart7.xml"));
}

#[test]
fn a_sheet_with_links_and_a_drawing_numbers_them_apart() {
    let mut model = workbook();
    model
        .set_user_input(0, 1, 1, "=HYPERLINK(\"https://ironcalc.com\")".to_string())
        .unwrap();
    model.evaluate();
    let parts = parts(&model);
    let rels = part(&parts, "xl/worksheets/_rels/sheet1.xml.rels");
    let drawing_id = if rels.contains("hyperlink") { 2 } else { 1 };
    assert!(rels.contains(&format!("Id=\"rId{drawing_id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing\"")));
    assert!(part(&parts, "xl/worksheets/sheet1.xml")
        .contains(&format!("<drawing r:id=\"rId{drawing_id}\"/>")));
}

#[test]
fn a_workbook_without_drawings_has_no_drawing_parts() {
    let model = Model::new_empty("plain", "en", "UTC", "en").unwrap();
    let parts = parts(&model);
    assert!(
        !parts.iter().any(|(name, _)| name.starts_with("xl/drawings")
            || name.starts_with("xl/charts")
            || name.starts_with("xl/media"))
    );
    assert!(!part(&parts, "xl/worksheets/sheet1.xml").contains("<drawing"));
}

/// Converts the workbook to PDF with LibreOffice and looks for the titles of
/// the charts in it: `cargo test -p ironcalc --test drawings -- --ignored`.
#[test]
#[ignore = "needs LibreOffice (soffice)"]
fn opens_in_libreoffice() {
    let directory = std::env::temp_dir().join(format!("ironcalc-drawings-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("charts.xlsx");
    let bytes = save_xlsx_to_writer(&workbook(), Cursor::new(Vec::new()))
        .unwrap()
        .into_inner();
    std::fs::write(&file, bytes).unwrap();
    let status = std::process::Command::new("soffice")
        .args(["--headless", "--convert-to", "pdf", "--outdir"])
        .arg(&directory)
        .arg(&file)
        .status()
        .expect("soffice");
    assert!(status.success());
    let pdf = directory.join("charts.pdf");
    assert!(std::fs::metadata(&pdf).unwrap().len() > 1000);
    if std::env::var("KEEP_DRAWINGS").is_err() {
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
