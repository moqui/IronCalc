//! The drawings of the sheets: images and charts, read from each sheet's
//! drawing part, its relationships, the images in `xl/media` and the chart
//! parts. A chart is read when it is one the model has (column, bar, line,
//! area, pie, doughnut, scatter); anything else, other charts, shapes,
//! groups, chart sheets, is left out and said so in the notes the import
//! returns.

use std::collections::HashMap;
use std::io::{BufReader, Read};

use ironcalc_base::drawings::{
    Anchor, CellPoint, Chart, ChartAxis, ChartGrouping, ChartKind, ChartSeries, Drawing,
    DrawingKind, LegendPosition, Media, SeriesName, SheetArea,
};
use ironcalc_base::expressions::utils::column_to_number;
use ironcalc_base::types::Worksheet;

use super::worksheets::Relationship;
use super::xml::XmlNode;

const RELATIONSHIPS_NS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// The sheets of the workbook as charts name them, with their ids.
type SheetIds = Vec<(String, u32)>;

/// What the import of the drawings left out, for the notes.
pub(crate) type Notes = Vec<String>;

fn read_part<R: Read + std::io::Seek>(
    archive: &mut zip::read::ZipArchive<R>,
    path: &str,
) -> Option<Vec<u8>> {
    let mut file = archive.by_name(path).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn parse_part<R: Read + std::io::Seek>(
    archive: &mut zip::read::ZipArchive<R>,
    path: &str,
) -> Option<XmlNode> {
    let file = archive.by_name(path).ok()?;
    XmlNode::parse(BufReader::new(file)).ok()
}

/// `../charts/chart1.xml` from `xl/drawings/drawing1.xml`: `xl/charts/chart1.xml`.
fn resolve(base: &str, target: &str) -> String {
    if let Some(absolute) = target.strip_prefix('/') {
        return absolute.to_string();
    }
    let mut parts: Vec<&str> = base.split('/').collect();
    parts.pop();
    for piece in target.split('/') {
        match piece {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            piece => parts.push(piece),
        }
    }
    parts.join("/")
}

/// The relationships of a part: id to (type, resolved target).
fn relationships<R: Read + std::io::Seek>(
    archive: &mut zip::read::ZipArchive<R>,
    part: &str,
) -> HashMap<String, (String, String)> {
    let (folder, file) = part.rsplit_once('/').unwrap_or(("", part));
    let path = format!("{folder}/_rels/{file}.rels");
    let mut found = HashMap::new();
    let Some(doc) = parse_part(archive, &path) else {
        return found;
    };
    for rel in doc.children().filter(|n| n.has_tag_name("Relationship")) {
        if let (Some(id), Some(kind), Some(target)) = (
            rel.attribute("Id"),
            rel.attribute("Type"),
            rel.attribute("Target"),
        ) {
            found.insert(id.to_string(), (kind.to_string(), resolve(part, target)));
        }
    }
    found
}

fn child<'a>(node: &'a XmlNode, name: &str) -> Option<&'a XmlNode> {
    node.children().find(|n| n.has_tag_name(name))
}

fn child_text<'a>(node: &'a XmlNode, name: &str) -> Option<&'a str> {
    child(node, name).and_then(XmlNode::text)
}

fn val<'a>(node: &'a XmlNode, name: &str) -> Option<&'a str> {
    child(node, name).and_then(|n| n.attribute("val"))
}

/// All the text of the runs below a node, as a title holds it.
fn rich_text(node: &XmlNode) -> String {
    node.descendants()
        .filter(|n| n.has_tag_name("t"))
        .filter_map(XmlNode::text)
        .collect::<Vec<_>>()
        .join("")
}

fn marker(node: &XmlNode) -> Option<CellPoint> {
    let number = |name: &str| child_text(node, name)?.trim().parse::<i64>().ok();
    Some(CellPoint {
        column: number("col")? as i32 + 1,
        column_offset: number("colOff").unwrap_or(0),
        row: number("row")? as i32 + 1,
        row_offset: number("rowOff").unwrap_or(0),
    })
}

/// `'Sales data'!$B$2:$B$4` as an area, its sheet named by id.
fn area(formula: &str, sheets: &SheetIds) -> Option<SheetArea> {
    let (sheet, range) = formula.trim().rsplit_once('!')?;
    let sheet = match sheet.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        Some(quoted) => quoted.replace("''", "'"),
        None => sheet.to_string(),
    };
    let (_, sheet_id) = sheets
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(&sheet))?;
    let corner = |text: &str| -> Option<(i32, i32)> {
        let text = text.replace('$', "");
        let split = text.find(|c: char| c.is_ascii_digit())?;
        let column = column_to_number(&text[..split]).ok()?;
        let row = text[split..].parse::<i32>().ok()?;
        Some((row, column))
    };
    let (first, last) = match range.split_once(':') {
        Some((a, b)) => (corner(a)?, corner(b)?),
        None => {
            let only = corner(range)?;
            (only, only)
        }
    };
    let (row1, row2) = (first.0.min(last.0), first.0.max(last.0));
    let (column1, column2) = (first.1.min(last.1), first.1.max(last.1));
    Some(SheetArea {
        sheet_id: *sheet_id,
        row: row1,
        column: column1,
        width: column2 - column1 + 1,
        height: row2 - row1 + 1,
    })
}

/// The formula of a data reference: `<c:val><c:numRef><c:f>…`.
fn reference(node: &XmlNode) -> Option<&str> {
    node.descendants()
        .find(|n| n.has_tag_name("f"))
        .and_then(XmlNode::text)
}

fn axis(node: Option<&XmlNode>) -> ChartAxis {
    let Some(node) = node else {
        return ChartAxis::default();
    };
    let scaling = child(node, "scaling");
    let bound = |name: &str| {
        scaling
            .and_then(|s| val(s, name))
            .and_then(|v| v.parse::<f64>().ok())
    };
    let number_format = child(node, "numFmt").and_then(|n| {
        if n.attribute("sourceLinked") == Some("1") {
            None
        } else {
            n.attribute("formatCode").map(str::to_string)
        }
    });
    ChartAxis {
        title: child(node, "title")
            .map(rich_text)
            .filter(|t| !t.is_empty()),
        number_format,
        min: bound("min"),
        max: bound("max"),
    }
}

/// The chart of a chart part, if it is one the model has.
fn chart(doc: &XmlNode, sheets: &SheetIds, name: &str, notes: &mut Notes) -> Option<Chart> {
    let chart_node = child(doc, "chart")?;
    let plot = child(chart_node, "plotArea")?;
    let plots: Vec<&XmlNode> = plot
        .children()
        .filter(|n| n.tag_name().ends_with("Chart"))
        .collect();
    let Some(first) = plots.first() else {
        notes.push(format!("{name}: a chart with nothing to plot was left out"));
        return None;
    };
    let kind = match first.tag_name() {
        "barChart" if val(first, "barDir") == Some("bar") => ChartKind::Bar,
        "barChart" => ChartKind::Column,
        "lineChart" => ChartKind::Line,
        "areaChart" => ChartKind::Area,
        "pieChart" => ChartKind::Pie,
        "doughnutChart" => ChartKind::Doughnut,
        "scatterChart" => ChartKind::Scatter,
        other => {
            notes.push(format!("{name}: a chart of type {other} was left out"));
            return None;
        }
    };
    if plots.len() > 1 {
        notes.push(format!(
            "{name}: a chart of more than one type keeps only its first ({})",
            first.tag_name()
        ));
    }
    let grouping = match val(first, "grouping") {
        Some("stacked") => ChartGrouping::Stacked,
        Some("percentStacked") => ChartGrouping::PercentStacked,
        _ => ChartGrouping::Standard,
    };
    let mut series = Vec::new();
    for ser in first.children().filter(|n| n.has_tag_name("ser")) {
        let (categories_tag, values_tag) = if kind == ChartKind::Scatter {
            ("xVal", "yVal")
        } else {
            ("cat", "val")
        };
        let Some(values) = child(ser, values_tag)
            .and_then(reference)
            .and_then(|f| area(f, sheets))
        else {
            notes.push(format!(
                "{name}: a series whose values are not a range of the workbook was left out"
            ));
            continue;
        };
        let name = child(ser, "tx").and_then(|tx| {
            if let Some(formula) = reference(tx) {
                area(formula, sheets).map(SeriesName::Cell)
            } else {
                child_text(tx, "v").map(|v| SeriesName::Text(v.to_string()))
            }
        });
        let color = child(ser, "spPr").and_then(|sp| {
            sp.descendants()
                .find(|n| n.has_tag_name("srgbClr"))
                .and_then(|n| n.attribute("val"))
                .map(str::to_string)
        });
        series.push(ChartSeries {
            name,
            categories: child(ser, categories_tag)
                .and_then(reference)
                .and_then(|f| area(f, sheets)),
            values,
            color,
        });
    }
    // The axes, by the ids the plot names: the first is the categories'
    let ids: Vec<&str> = first
        .children()
        .filter(|n| n.has_tag_name("axId"))
        .filter_map(|n| n.attribute("val"))
        .collect();
    let axis_of = |id: Option<&&str>| {
        plot.children()
            .filter(|n| n.tag_name().ends_with("Ax"))
            .find(|n| val(n, "axId") == id.copied())
    };
    let auto_title_deleted = val(chart_node, "autoTitleDeleted") == Some("1");
    let title = child(chart_node, "title")
        .map(rich_text)
        .filter(|t| !t.is_empty() && !auto_title_deleted);
    let legend = child(chart_node, "legend").map(|legend| match val(legend, "legendPos") {
        Some("l") => LegendPosition::Left,
        Some("t") => LegendPosition::Top,
        Some("b") => LegendPosition::Bottom,
        _ => LegendPosition::Right,
    });
    Some(Chart {
        kind,
        grouping,
        title,
        legend,
        series,
        category_axis: axis(axis_of(ids.first())),
        value_axis: axis(axis_of(ids.get(1))),
    })
}

/// The drawings of one drawing part.
#[allow(clippy::too_many_arguments)]
fn load_drawing_part<R: Read + std::io::Seek>(
    archive: &mut zip::read::ZipArchive<R>,
    part: &str,
    sheet_name: &str,
    sheets: &SheetIds,
    media: &mut Vec<Media>,
    media_by_path: &mut HashMap<String, u32>,
    notes: &mut Notes,
) -> Vec<Drawing> {
    let mut drawings = Vec::new();
    let Some(doc) = parse_part(archive, part) else {
        notes.push(format!("{sheet_name}: its drawings could not be read"));
        return drawings;
    };
    let rels = relationships(archive, part);
    for anchor_node in doc.children() {
        let anchor = match anchor_node.tag_name() {
            "twoCellAnchor" => {
                match (
                    child(anchor_node, "from").and_then(marker),
                    child(anchor_node, "to").and_then(marker),
                ) {
                    (Some(from), Some(to)) => Anchor::TwoCell { from, to },
                    _ => continue,
                }
            }
            "oneCellAnchor" => {
                let ext = child(anchor_node, "ext");
                let size = |name: &str| {
                    ext.and_then(|e| e.attribute(name))
                        .and_then(|v| v.parse::<i64>().ok())
                        .unwrap_or(0)
                };
                match child(anchor_node, "from").and_then(marker) {
                    Some(from) => Anchor::OneCell {
                        from,
                        width: size("cx"),
                        height: size("cy"),
                    },
                    None => continue,
                }
            }
            "absoluteAnchor" => {
                notes.push(format!(
                    "{sheet_name}: a drawing placed by position, not by cell, was left out"
                ));
                continue;
            }
            _ => continue,
        };
        let Some(object) = anchor_node.children().find(|n| {
            matches!(
                n.tag_name(),
                "pic" | "graphicFrame" | "sp" | "grpSp" | "cxnSp"
            )
        }) else {
            continue;
        };
        let properties = object.descendants().find(|n| n.has_tag_name("cNvPr"));
        let name = properties
            .and_then(|p| p.attribute("name"))
            .unwrap_or_default()
            .to_string();
        let description = properties
            .and_then(|p| p.attribute("descr"))
            .unwrap_or_default()
            .to_string();
        let kind = match object.tag_name() {
            "pic" => {
                let target = object
                    .descendants()
                    .find(|n| n.has_tag_name("blip"))
                    .and_then(|b| {
                        b.attribute_ns(RELATIONSHIPS_NS, "embed")
                            .or(b.attribute("embed"))
                    })
                    .and_then(|id| rels.get(id))
                    .map(|(_, target)| target.clone());
                let Some(target) = target else {
                    notes.push(format!(
                        "{sheet_name}: an image without its file was left out"
                    ));
                    continue;
                };
                let index = match media_by_path.get(&target) {
                    Some(index) => *index,
                    None => {
                        let extension = target
                            .rsplit_once('.')
                            .map(|(_, e)| e.to_ascii_lowercase())
                            .unwrap_or_default();
                        let content_type = match extension.as_str() {
                            "png" => "image/png",
                            "jpg" | "jpeg" => "image/jpeg",
                            "gif" => "image/gif",
                            _ => {
                                notes.push(format!(
                                    "{sheet_name}: an image of type {extension} was left out"
                                ));
                                continue;
                            }
                        };
                        let Some(bytes) = read_part(archive, &target) else {
                            notes.push(format!(
                                "{sheet_name}: an image without its file was left out"
                            ));
                            continue;
                        };
                        let index = media.len() as u32;
                        let extension = if extension == "jpg" {
                            "jpeg"
                        } else {
                            &extension
                        };
                        media.push(Media {
                            name: format!("image{}.{extension}", index + 1),
                            content_type: content_type.to_string(),
                            bytes,
                        });
                        media_by_path.insert(target, index);
                        index
                    }
                };
                DrawingKind::Image { media: index }
            }
            "graphicFrame" => {
                let target = object
                    .descendants()
                    .find(|n| n.has_tag_name("chart"))
                    .and_then(|c| c.attribute_ns(RELATIONSHIPS_NS, "id").or(c.attribute("id")))
                    .and_then(|id| rels.get(id))
                    .map(|(_, target)| target.clone());
                let Some(target) = target else {
                    notes.push(format!(
                        "{sheet_name}: {name}, a frame that is not a chart, was left out"
                    ));
                    continue;
                };
                let label = format!("{sheet_name}: {name}");
                let Some(chart) =
                    parse_part(archive, &target).and_then(|doc| chart(&doc, sheets, &label, notes))
                else {
                    continue;
                };
                DrawingKind::Chart(Box::new(chart))
            }
            other => {
                let what = match other {
                    "sp" => "a shape",
                    "grpSp" => "a group of shapes",
                    _ => "a connector",
                };
                notes.push(format!("{sheet_name}: {what} ({name}) was left out"));
                continue;
            }
        };
        drawings.push(Drawing {
            anchor,
            name,
            description,
            kind,
        });
    }
    drawings
}

/// Reads the drawings of every sheet into `worksheets`, the images into the
/// list returned, and notes what was left out. `sheet_parts` gives the part
/// of each sheet, in the order of `worksheets`; `chart_sheets`, the names of
/// the chart sheets, which are left out.
pub(crate) fn load_drawings<R: Read + std::io::Seek>(
    archive: &mut zip::read::ZipArchive<R>,
    sheet_parts: &[String],
    worksheets: &mut [Worksheet],
    chart_sheets: &[String],
    notes: &mut Notes,
) -> Vec<Media> {
    let sheets: SheetIds = worksheets
        .iter()
        .map(|w| (w.name.clone(), w.sheet_id))
        .collect();
    let mut media = Vec::new();
    let mut media_by_path = HashMap::new();
    for (worksheet, part) in worksheets.iter_mut().zip(sheet_parts) {
        let drawing_parts: Vec<String> = relationships(archive, part)
            .into_values()
            .filter(|(kind, _)| kind.ends_with("/drawing"))
            .map(|(_, target)| target)
            .collect();
        for drawing_part in drawing_parts {
            let name = worksheet.name.clone();
            worksheet.drawings.extend(load_drawing_part(
                archive,
                &drawing_part,
                &name,
                &sheets,
                &mut media,
                &mut media_by_path,
                notes,
            ));
        }
    }
    for name in chart_sheets {
        notes.push(format!("{name}: a chart sheet was left out"));
    }
    media
}

/// The part of each worksheet in the workbook's relationships, in order,
/// and the names of the chart sheets.
pub(crate) fn sheet_parts(
    workbook_sheets: &[(String, String)],
    rels: &HashMap<String, Relationship>,
) -> (Vec<String>, Vec<String>) {
    let mut parts = Vec::new();
    let mut chart_sheets = Vec::new();
    for (name, rel_id) in workbook_sheets {
        let Some(rel) = rels.get(rel_id) else {
            continue;
        };
        let path = match rel.target.strip_prefix('/') {
            Some(p) => p.to_string(),
            None => format!("xl/{}", rel.target),
        };
        if rel.rel_type.ends_with("worksheet") {
            parts.push(path);
        } else if rel.rel_type.ends_with("chartsheet") {
            chart_sheets.push(name.clone());
        }
    }
    (parts, chart_sheets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_resolve_against_their_part() {
        assert_eq!(
            resolve("xl/drawings/drawing1.xml", "../charts/chart1.xml"),
            "xl/charts/chart1.xml"
        );
        assert_eq!(
            resolve("xl/worksheets/sheet1.xml", "../drawings/drawing2.xml"),
            "xl/drawings/drawing2.xml"
        );
        assert_eq!(
            resolve("xl/drawings/drawing1.xml", "/xl/media/a.png"),
            "xl/media/a.png"
        );
    }

    #[test]
    fn references_become_areas_of_known_sheets() {
        let sheets = vec![("Sales data".to_string(), 7), ("Sheet1".to_string(), 1)];
        let area_of =
            |f: &str| area(f, &sheets).map(|a| (a.sheet_id, a.row, a.column, a.width, a.height));
        assert_eq!(area_of("'Sales data'!$B$2:$B$4"), Some((7, 2, 2, 1, 3)));
        assert_eq!(area_of("Sheet1!$A$1"), Some((1, 1, 1, 1, 1)));
        assert_eq!(area_of("sheet1!C3:A1"), Some((1, 1, 1, 3, 3)));
        assert_eq!(area_of("Other!A1"), None);
        assert_eq!(area_of("A1:B2"), None);
        assert_eq!(area_of("Sheet1!myName"), None);
    }
}
