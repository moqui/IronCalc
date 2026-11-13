//! The stored form of a workbook: what `Model::to_bytes` writes and
//! `Model::from_bytes` reads, and what an `.ic` file holds.
//!
//! It is the workbook encoded with `bitcode`, which writes the fields of each
//! type in order and nothing that says which they are. A field added to the
//! workbook or a sheet would make every workbook stored before unreadable.
//! So the stored form starts with a mark and a version, and a workbook
//! stored before the mark existed is version 0. Each version the model has
//! had is read with the types it had then, kept for that, and turned into
//! the model of now.

use crate::types::{Workbook, Worksheet};

/// What a stored workbook starts with. A workbook stored without it starts
/// with the length of its list of shared strings, which is never these four
/// bytes followed by a known version.
const MARK: [u8; 4] = *b"\0ICW";

/// The version written now. 2 added images and charts: `Workbook::media`
/// and `Worksheet::drawings`.
const VERSION: u16 = 2;

/// The stored form of a workbook.
pub fn workbook_to_bytes(workbook: &Workbook) -> Vec<u8> {
    let body = bitcode::encode(workbook);
    let mut bytes = Vec::with_capacity(MARK.len() + 2 + body.len());
    bytes.extend_from_slice(&MARK);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&body);
    bytes
}

/// The workbook a stored form holds, of any version up to this one.
pub fn workbook_from_bytes(bytes: &[u8]) -> Result<Workbook, String> {
    let fail = |e: bitcode::Error| format!("Error parsing workbook: {e}");
    let Some(rest) = bytes.strip_prefix(&MARK) else {
        // Version 0, stored before there was a mark: the layout of version 1
        return bitcode::decode::<v1::Workbook>(bytes)
            .map(Workbook::from)
            .map_err(fail);
    };
    let (Some(version), Some(body)) = (rest.get(..2), rest.get(2..)) else {
        return Err("Error parsing workbook: too short".to_string());
    };
    match u16::from_le_bytes([version[0], version[1]]) {
        1 => bitcode::decode::<v1::Workbook>(body)
            .map(Workbook::from)
            .map_err(fail),
        2 => bitcode::decode(body).map_err(fail),
        later => Err(format!(
            "The workbook was stored by a later version of IronCalc (stored form {later})"
        )),
    }
}

/// The workbook and the sheet as versions 0 and 1 stored them, before images
/// and charts. Their other types have not changed.
mod v1 {
    use std::collections::HashMap;

    use bitcode::{Decode, Encode};

    use crate::cf_types::ConditionalFormatting;
    use crate::types::{
        Col, Color, Comment, DefinedName, Link, MergedCell, Metadata, Row, SheetData, SheetState,
        Styles, Table, Theme, WorkbookSettings,
    };

    #[derive(Encode, Decode)]
    pub(super) struct Workbook {
        pub shared_strings: Vec<String>,
        pub defined_names: Vec<DefinedName>,
        pub worksheets: Vec<Worksheet>,
        pub styles: Styles,
        pub name: String,
        pub settings: WorkbookSettings,
        pub metadata: Metadata,
        pub tables: HashMap<String, Table>,
        pub theme: Theme,
    }

    #[derive(Encode, Decode)]
    pub(super) struct Worksheet {
        pub dimension: String,
        pub cols: Vec<Col>,
        pub rows: Vec<Row>,
        pub name: String,
        pub sheet_data: SheetData,
        pub shared_formulas: Vec<String>,
        pub sheet_id: u32,
        pub state: SheetState,
        pub color: Color,
        pub merged_cells: Vec<MergedCell>,
        pub comments: Vec<Comment>,
        pub frozen_rows: i32,
        pub frozen_columns: i32,
        pub show_grid_lines: bool,
        pub conditional_formatting: Vec<ConditionalFormatting>,
        pub links: HashMap<(i32, i32), Link>,
    }
}

impl From<v1::Worksheet> for Worksheet {
    fn from(old: v1::Worksheet) -> Worksheet {
        Worksheet {
            dimension: old.dimension,
            cols: old.cols,
            rows: old.rows,
            name: old.name,
            sheet_data: old.sheet_data,
            shared_formulas: old.shared_formulas,
            sheet_id: old.sheet_id,
            state: old.state,
            color: old.color,
            merged_cells: old.merged_cells,
            comments: old.comments,
            frozen_rows: old.frozen_rows,
            frozen_columns: old.frozen_columns,
            show_grid_lines: old.show_grid_lines,
            conditional_formatting: old.conditional_formatting,
            links: old.links,
            drawings: Vec::new(),
        }
    }
}

impl From<v1::Workbook> for Workbook {
    fn from(old: v1::Workbook) -> Workbook {
        Workbook {
            shared_strings: old.shared_strings,
            defined_names: old.defined_names,
            worksheets: old.worksheets.into_iter().map(Worksheet::from).collect(),
            styles: old.styles,
            name: old.name,
            settings: old.settings,
            metadata: old.metadata,
            tables: old.tables,
            theme: old.theme,
            media: Vec::new(),
        }
    }
}
