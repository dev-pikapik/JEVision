//! Tables for `useData` blocks.

use std::path::Path;

/// A CSV file: the first row names the columns, every other row is one pass of the loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataTable {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl DataTable {
    /// Reads a UTF-8 CSV file. Every row must have as many fields as there are columns.
    pub fn load(path: &Path) -> Result<Self, csv::Error> {
        Self::read(csv::Reader::from_path(path)?)
    }

    pub fn parse(text: &str) -> Result<Self, csv::Error> {
        Self::read(csv::Reader::from_reader(text.as_bytes()))
    }

    fn read<R: std::io::Read>(mut reader: csv::Reader<R>) -> Result<Self, csv::Error> {
        let columns = reader
            .headers()?
            .iter()
            .map(|c| c.trim().to_owned())
            .collect();
        let rows = reader
            .records()
            .map(|r| r.map(|r| r.iter().map(str::to_owned).collect()))
            .collect::<Result<_, _>>()?;
        Ok(Self { columns, rows })
    }

    /// Value of `column` in row `row`.
    pub fn value(&self, row: usize, column: &str) -> Option<&str> {
        let i = self.columns.iter().position(|c| c == column)?;
        self.rows.get(row)?.get(i).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_quoted_fields_bom_and_crlf() {
        let t =
            DataTable::parse("\u{feff}email, name\r\na@x.io,\"Doe, \"\"J\"\"\"\r\nb@x.io,Bo\r\n")
                .unwrap();
        assert_eq!(t.columns, ["email", "name"]);
        assert_eq!(t.value(0, "name"), Some("Doe, \"J\""));
        assert_eq!(t.value(1, "email"), Some("b@x.io"));
        assert_eq!(t.value(2, "email"), None);
        assert!(DataTable::parse("a,b\n1\n").is_err(), "short row");
    }
}
