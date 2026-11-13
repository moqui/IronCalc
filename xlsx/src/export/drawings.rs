//! The drawings of the sheets: for each sheet that has any, a drawing part
//! (`xl/drawings/drawingN.xml`) with the anchor of each image and chart, its
//! relationships to the images (`xl/media`) and the charts
//! (`xl/charts/chartN.xml`), and a chart part for each chart.

use ironcalc_base::drawings::{
    Anchor, CellPoint, Chart, ChartAxis, ChartGrouping, ChartKind, Drawing, DrawingKind,
    LegendPosition, SeriesName, SheetArea,
};
use ironcalc_base::expressions::utils::{number_to_column, quote_name};
use ironcalc_base::types::Workbook;

use super::escape::escape_xml;
use super::xml_constants::XML_DECLARATION;

/// The parts of the drawings of one sheet.
pub(crate) struct SheetDrawings {
    /// `N` of `xl/drawings/drawingN.xml`.
    pub(crate) number: usize,
    pub(crate) xml: String,
    pub(crate) rels: String,
    /// `N` of `xl/charts/chartN.xml`, and the part.
    pub(crate) charts: Vec<(usize, String)>,
}

const SPREADSHEET_DRAWING: &str =
    "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
const DRAWING_MAIN: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const CHART: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// The drawing parts of every sheet, `None` for a sheet without drawings,
/// numbered across the workbook.
pub(crate) fn get_sheet_drawings(workbook: &Workbook) -> Vec<Option<SheetDrawings>> {
    let mut drawing_number = 0;
    let mut chart_number = 0;
    workbook
        .worksheets
        .iter()
        .map(|worksheet| {
            if worksheet.drawings.is_empty() {
                return None;
            }
            drawing_number += 1;
            let mut anchors = Vec::new();
            let mut relationships = Vec::new();
            let mut charts = Vec::new();
            for (index, drawing) in worksheet.drawings.iter().enumerate() {
                let rel_id = relationships.len() + 1;
                let object = match &drawing.kind {
                    DrawingKind::Image { media } => {
                        let Some(media) = workbook.media.get(*media as usize) else {
                            continue;
                        };
                        relationships.push(format!(
                            "<Relationship Id=\"rId{rel_id}\" Type=\"{RELATIONSHIPS}/image\" Target=\"../media/{}\"/>",
                            escape_xml(&media.name)
                        ));
                        picture(drawing, index, rel_id)
                    }
                    DrawingKind::Chart(chart) => {
                        chart_number += 1;
                        relationships.push(format!(
                            "<Relationship Id=\"rId{rel_id}\" Type=\"{RELATIONSHIPS}/chart\" Target=\"../charts/chart{chart_number}.xml\"/>"
                        ));
                        charts.push((chart_number, chart_xml(chart, workbook)));
                        graphic_frame(drawing, index, rel_id)
                    }
                };
                anchors.push(anchor(&drawing.anchor, &object));
            }
            Some(SheetDrawings {
                number: drawing_number,
                xml: format!(
                    "{XML_DECLARATION}\n<xdr:wsDr xmlns:xdr=\"{SPREADSHEET_DRAWING}\" xmlns:a=\"{DRAWING_MAIN}\">{}</xdr:wsDr>",
                    anchors.join("")
                ),
                rels: format!(
                    "{XML_DECLARATION}\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{}</Relationships>",
                    relationships.join("")
                ),
                charts,
            })
        })
        .collect()
}

/// A point as an anchor gives it: rows and columns from 0.
fn marker(tag: &str, point: &CellPoint) -> String {
    format!(
        "<xdr:{tag}><xdr:col>{}</xdr:col><xdr:colOff>{}</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>{}</xdr:rowOff></xdr:{tag}>",
        point.column - 1,
        point.column_offset,
        point.row - 1,
        point.row_offset
    )
}

fn anchor(anchor: &Anchor, object: &str) -> String {
    match anchor {
        Anchor::TwoCell { from, to } => format!(
            "<xdr:twoCellAnchor>{}{}{object}<xdr:clientData/></xdr:twoCellAnchor>",
            marker("from", from),
            marker("to", to)
        ),
        Anchor::OneCell {
            from,
            width,
            height,
        } => format!(
            "<xdr:oneCellAnchor>{}<xdr:ext cx=\"{width}\" cy=\"{height}\"/>{object}<xdr:clientData/></xdr:oneCellAnchor>",
            marker("from", from)
        ),
    }
}

/// The id of a drawing in its part: 1 is the part's own.
fn shape_id(index: usize) -> usize {
    index + 2
}

fn picture(drawing: &Drawing, index: usize, rel_id: usize) -> String {
    let (width, height) = match drawing.anchor {
        Anchor::OneCell { width, height, .. } => (width, height),
        Anchor::TwoCell { .. } => (0, 0),
    };
    format!(
        "<xdr:pic><xdr:nvPicPr><xdr:cNvPr id=\"{}\" name=\"{}\" descr=\"{}\"/><xdr:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></xdr:cNvPicPr></xdr:nvPicPr>\
<xdr:blipFill><a:blip xmlns:r=\"{RELATIONSHIPS}\" r:embed=\"rId{rel_id}\"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill>\
<xdr:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{width}\" cy=\"{height}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic>",
        shape_id(index),
        escape_xml(&drawing.name),
        escape_xml(&drawing.description)
    )
}

fn graphic_frame(drawing: &Drawing, index: usize, rel_id: usize) -> String {
    format!(
        "<xdr:graphicFrame macro=\"\"><xdr:nvGraphicFramePr><xdr:cNvPr id=\"{}\" name=\"{}\" descr=\"{}\"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr>\
<xdr:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/></xdr:xfrm>\
<a:graphic><a:graphicData uri=\"{CHART}\"><c:chart xmlns:c=\"{CHART}\" xmlns:r=\"{RELATIONSHIPS}\" r:id=\"rId{rel_id}\"/></a:graphicData></a:graphic></xdr:graphicFrame>",
        shape_id(index),
        escape_xml(&drawing.name),
        escape_xml(&drawing.description)
    )
}

/// A range as a chart refers to it: `'Sheet name'!$A$2:$A$6`. Empty if its
/// sheet is gone.
fn reference(area: &SheetArea, workbook: &Workbook) -> String {
    let Some(worksheet) = workbook
        .worksheets
        .iter()
        .find(|worksheet| worksheet.sheet_id == area.sheet_id)
    else {
        return String::new();
    };
    let corner = |row: i32, column: i32| {
        format!(
            "${}${row}",
            number_to_column(column).unwrap_or_else(|| "A".to_string())
        )
    };
    let first = corner(area.row, area.column);
    let range = if area.width == 1 && area.height == 1 {
        first
    } else {
        format!(
            "{first}:{}",
            corner(area.row + area.height - 1, area.column + area.width - 1)
        )
    };
    format!("{}!{range}", quote_name(&worksheet.name))
}

fn text(value: &str) -> String {
    format!(
        "<c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>{}</a:t></a:r></a:p></c:rich></c:tx>",
        escape_xml(value)
    )
}

fn title(value: &Option<String>) -> String {
    match value {
        Some(value) => format!("<c:title>{}<c:overlay val=\"0\"/></c:title>", text(value)),
        None => String::new(),
    }
}

/// Values the chart reads as numbers (`numRef`) or as texts (`strRef`).
fn data(tag: &str, kind: &str, area: &SheetArea, workbook: &Workbook) -> String {
    format!(
        "<c:{tag}><c:{kind}><c:f>{}</c:f></c:{kind}></c:{tag}>",
        escape_xml(&reference(area, workbook))
    )
}

fn series_xml(chart: &Chart, workbook: &Workbook) -> String {
    let mut all = Vec::new();
    for (index, series) in chart.series.iter().enumerate() {
        let name = match &series.name {
            Some(SeriesName::Text(name)) => {
                format!("<c:tx><c:v>{}</c:v></c:tx>", escape_xml(name))
            }
            Some(SeriesName::Cell(area)) => format!(
                "<c:tx><c:strRef><c:f>{}</c:f></c:strRef></c:tx>",
                escape_xml(&reference(area, workbook))
            ),
            None => String::new(),
        };
        let lines = matches!(chart.kind, ChartKind::Line | ChartKind::Scatter);
        let fill = match &series.color {
            Some(color) if lines => format!(
                "<c:spPr><a:ln w=\"28575\"><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill></a:ln></c:spPr>",
                escape_xml(color)
            ),
            Some(color) => format!(
                "<c:spPr><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill></c:spPr>",
                escape_xml(color)
            ),
            None => String::new(),
        };
        let marker = if lines {
            "<c:marker><c:symbol val=\"none\"/></c:marker>"
        } else {
            ""
        };
        let invert = if matches!(chart.kind, ChartKind::Column | ChartKind::Bar) {
            "<c:invertIfNegative val=\"0\"/>"
        } else {
            ""
        };
        let (categories, values) = if chart.kind == ChartKind::Scatter {
            (
                series
                    .categories
                    .as_ref()
                    .map(|area| data("xVal", "numRef", area, workbook))
                    .unwrap_or_default(),
                data("yVal", "numRef", &series.values, workbook),
            )
        } else {
            (
                series
                    .categories
                    .as_ref()
                    .map(|area| data("cat", "strRef", area, workbook))
                    .unwrap_or_default(),
                data("val", "numRef", &series.values, workbook),
            )
        };
        let smooth = if lines { "<c:smooth val=\"0\"/>" } else { "" };
        all.push(format!(
            "<c:ser><c:idx val=\"{index}\"/><c:order val=\"{index}\"/>{name}{fill}{invert}{marker}{categories}{values}{smooth}</c:ser>"
        ));
    }
    all.join("")
}

fn grouping(chart: &Chart, bars: bool) -> &'static str {
    match (chart.grouping, bars) {
        (ChartGrouping::Standard, true) => "clustered",
        (ChartGrouping::Standard, false) => "standard",
        (ChartGrouping::Stacked, _) => "stacked",
        (ChartGrouping::PercentStacked, _) => "percentStacked",
    }
}

fn axis_parts(axis: &ChartAxis) -> (String, String, String) {
    let scaling = format!(
        "<c:scaling><c:orientation val=\"minMax\"/>{}{}</c:scaling>",
        axis.max
            .map(|max| format!("<c:max val=\"{max}\"/>"))
            .unwrap_or_default(),
        axis.min
            .map(|min| format!("<c:min val=\"{min}\"/>"))
            .unwrap_or_default()
    );
    let number_format = axis
        .number_format
        .as_ref()
        .map(|format| {
            format!(
                "<c:numFmt formatCode=\"{}\" sourceLinked=\"0\"/>",
                escape_xml(format)
            )
        })
        .unwrap_or_default();
    (scaling, title(&axis.title), number_format)
}

/// The two axes of a chart that has them: categories (or a value axis for
/// the first number of a point) and values.
fn axes_xml(chart: &Chart) -> String {
    let (horizontal, vertical) = if chart.kind == ChartKind::Bar {
        ("l", "b")
    } else {
        ("b", "l")
    };
    let (scaling, title, number_format) = axis_parts(&chart.category_axis);
    let category = if chart.kind == ChartKind::Scatter {
        format!(
            "<c:valAx><c:axId val=\"1\"/>{scaling}<c:delete val=\"0\"/><c:axPos val=\"{horizontal}\"/>{title}{number_format}<c:majorTickMark val=\"out\"/><c:minorTickMark val=\"none\"/><c:tickLblPos val=\"nextTo\"/><c:crossAx val=\"2\"/><c:crosses val=\"autoZero\"/><c:crossBetween val=\"midCat\"/></c:valAx>"
        )
    } else {
        format!(
            "<c:catAx><c:axId val=\"1\"/>{scaling}<c:delete val=\"0\"/><c:axPos val=\"{horizontal}\"/>{title}{number_format}<c:majorTickMark val=\"out\"/><c:minorTickMark val=\"none\"/><c:tickLblPos val=\"nextTo\"/><c:crossAx val=\"2\"/><c:crosses val=\"autoZero\"/><c:auto val=\"1\"/><c:lblAlgn val=\"ctr\"/><c:lblOffset val=\"100\"/><c:noMultiLvlLbl val=\"0\"/></c:catAx>"
        )
    };
    let (scaling, title, number_format) = axis_parts(&chart.value_axis);
    let between = if chart.kind == ChartKind::Scatter {
        "midCat"
    } else {
        "between"
    };
    let value = format!(
        "<c:valAx><c:axId val=\"2\"/>{scaling}<c:delete val=\"0\"/><c:axPos val=\"{vertical}\"/><c:majorGridlines/>{title}{number_format}<c:majorTickMark val=\"out\"/><c:minorTickMark val=\"none\"/><c:tickLblPos val=\"nextTo\"/><c:crossAx val=\"1\"/><c:crosses val=\"autoZero\"/><c:crossBetween val=\"{between}\"/></c:valAx>"
    );
    format!("{category}{value}")
}

/// The chart part of a chart.
pub(crate) fn chart_xml(chart: &Chart, workbook: &Workbook) -> String {
    let series = series_xml(chart, workbook);
    let axis_ids = "<c:axId val=\"1\"/><c:axId val=\"2\"/>";
    let plot = match chart.kind {
        ChartKind::Column | ChartKind::Bar => {
            let direction = if chart.kind == ChartKind::Bar {
                "bar"
            } else {
                "col"
            };
            let overlap = if chart.grouping == ChartGrouping::Standard {
                ""
            } else {
                "<c:overlap val=\"100\"/>"
            };
            format!(
                "<c:barChart><c:barDir val=\"{direction}\"/><c:grouping val=\"{}\"/><c:varyColors val=\"0\"/>{series}<c:gapWidth val=\"150\"/>{overlap}{axis_ids}</c:barChart>{}",
                grouping(chart, true),
                axes_xml(chart)
            )
        }
        ChartKind::Line => format!(
            "<c:lineChart><c:grouping val=\"{}\"/><c:varyColors val=\"0\"/>{series}<c:marker val=\"1\"/>{axis_ids}</c:lineChart>{}",
            grouping(chart, false),
            axes_xml(chart)
        ),
        ChartKind::Area => format!(
            "<c:areaChart><c:grouping val=\"{}\"/><c:varyColors val=\"0\"/>{series}{axis_ids}</c:areaChart>{}",
            grouping(chart, false),
            axes_xml(chart)
        ),
        ChartKind::Scatter => format!(
            "<c:scatterChart><c:scatterStyle val=\"lineMarker\"/><c:varyColors val=\"0\"/>{series}{axis_ids}</c:scatterChart>{}",
            axes_xml(chart)
        ),
        ChartKind::Pie => format!(
            "<c:pieChart><c:varyColors val=\"1\"/>{series}<c:firstSliceAng val=\"0\"/></c:pieChart>"
        ),
        ChartKind::Doughnut => format!(
            "<c:doughnutChart><c:varyColors val=\"1\"/>{series}<c:firstSliceAng val=\"0\"/><c:holeSize val=\"50\"/></c:doughnutChart>"
        ),
    };
    let auto_title_deleted = if chart.title.is_some() { "0" } else { "1" };
    let legend = match chart.legend {
        Some(position) => {
            let position = match position {
                LegendPosition::Right => "r",
                LegendPosition::Left => "l",
                LegendPosition::Top => "t",
                LegendPosition::Bottom => "b",
            };
            format!("<c:legend><c:legendPos val=\"{position}\"/><c:overlay val=\"0\"/></c:legend>")
        }
        None => String::new(),
    };
    format!(
        "{XML_DECLARATION}\n<c:chartSpace xmlns:c=\"{CHART}\" xmlns:a=\"{DRAWING_MAIN}\" xmlns:r=\"{RELATIONSHIPS}\"><c:roundedCorners val=\"0\"/><c:chart>{}<c:autoTitleDeleted val=\"{auto_title_deleted}\"/><c:plotArea><c:layout/>{plot}</c:plotArea>{legend}<c:plotVisOnly val=\"1\"/><c:dispBlanksAs val=\"gap\"/></c:chart></c:chartSpace>",
        title(&chart.title)
    )
}
