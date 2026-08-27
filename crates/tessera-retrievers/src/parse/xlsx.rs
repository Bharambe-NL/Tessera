//! XLSX, through calamine.
//!
//! Doc 05 section 8.2: "A spreadsheet chunk carries the row range." So a chunk
//! here is a band of rows rendered as text, not a cell and not a whole sheet. A
//! cell on its own says "8.4" and means nothing; a whole sheet is one
//! undifferentiated blob that no citation can point into. A row band keeps the
//! header with the values and gives the citation somewhere to land.

use std::path::Path;

use calamine::{Data, Reader, open_workbook_auto};

use crate::chunking::{Chunk, ChunkLocation, enforce_cap};
use crate::parse::ParseError;

/// Rows per chunk, after the header.
const ROWS_PER_CHUNK: usize = 12;

pub fn parse(path: &Path) -> Result<Vec<Chunk>, ParseError> {
    let mut workbook = open_workbook_auto(path).map_err(|e| {
        let detail = e.to_string();
        if detail.contains("Password") || detail.contains("encrypt") {
            ParseError::Protected(path.display().to_string())
        } else {
            ParseError::Malformed {
                format: "xlsx",
                detail,
            }
        }
    })?;

    let mut out = Vec::new();
    let mut sequence = 0usize;

    let sheets: Vec<String> = workbook.sheet_names().to_vec();
    for sheet in sheets {
        let Ok(range) = workbook.worksheet_range(&sheet) else {
            continue;
        };
        let rows: Vec<Vec<String>> = range
            .rows()
            .map(|row| row.iter().map(render_cell).collect())
            .collect();
        if rows.is_empty() {
            continue;
        }

        // The first row is the header and rides along with every band, because
        // "8.4" under a column nobody named is not a fact.
        let header = &rows[0];
        let header_line = header.join(" | ");

        for (band, chunk_rows) in rows[1..].chunks(ROWS_PER_CHUNK).enumerate() {
            let from = 1 + band * ROWS_PER_CHUNK + 1;
            let to = from + chunk_rows.len() - 1;

            let mut text = String::new();
            text.push_str(&sheet);
            text.push('\n');
            text.push_str(&header_line);
            text.push('\n');
            for row in chunk_rows {
                text.push_str(&row.join(" | "));
                text.push('\n');
            }
            let text = text.trim_end().to_string();
            if text
                .lines()
                .skip(2)
                .all(|l| l.trim().is_empty() || l.chars().all(|c| c == '|' || c.is_whitespace()))
            {
                continue;
            }

            out.push(Chunk::new(
                text,
                ChunkLocation::RowRange {
                    sheet: sheet.clone(),
                    from,
                    to,
                },
                sequence,
            ));
            sequence += 1;
        }
    }

    Ok(enforce_cap(out))
}

fn render_cell(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(s) => s.trim().to_string(),
        Data::Float(f) => {
            // Whole numbers read as "12" rather than "12.0", because the
            // matcher and the reader both expect the number as written.
            if (f.fract()).abs() < f64::EPSILON {
                format!("{}", *f as i64)
            } else {
                format!("{f}")
            }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(d) => d.to_string(),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("#{e:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_number_renders_without_a_trailing_zero() {
        assert_eq!(render_cell(&Data::Float(12.0)), "12");
        assert_eq!(render_cell(&Data::Float(8.4)), "8.4");
        assert_eq!(render_cell(&Data::Int(3)), "3");
        assert_eq!(render_cell(&Data::Empty), "");
    }

    #[test]
    fn a_missing_file_is_an_error_not_a_panic() {
        let path = std::env::temp_dir().join("tessera-no-such-workbook.xlsx");
        assert!(parse(&path).is_err());
    }

    #[test]
    fn a_file_that_is_not_a_workbook_is_an_error_not_a_panic() {
        let dir = std::env::temp_dir().join(format!("tessera-xlsx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("broken.xlsx");
        std::fs::write(&path, b"not a workbook").expect("write");
        assert!(parse(&path).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
