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

use crate::types::Workbook;

/// What a stored workbook starts with. A workbook stored without it starts
/// with the length of its list of shared strings, which is never these four
/// bytes followed by a known version.
const MARK: [u8; 4] = *b"\0ICW";

/// The version written now.
const VERSION: u16 = 1;

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
        return bitcode::decode(bytes).map_err(fail);
    };
    let (Some(version), Some(body)) = (rest.get(..2), rest.get(2..)) else {
        return Err("Error parsing workbook: too short".to_string());
    };
    match u16::from_le_bytes([version[0], version[1]]) {
        1 => bitcode::decode(body).map_err(fail),
        later => Err(format!(
            "The workbook was stored by a later version of IronCalc (stored form {later})"
        )),
    }
}
