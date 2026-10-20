//! Data tables for `useData` blocks: CSV files as described in RFC 4180.
//!
//! The first line names the columns; every other line is one row. Values are separated by a
//! comma, a semicolon or a tab, whichever the first line uses most. A value in double quotes
//! may hold separators, line breaks and quotes written twice (`""`). Lines may end with LF or
//! CR LF, a UTF-8 byte order mark is skipped, and blank lines are ignored.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::ops::RangeInclusive;
use std::path::Path;

use jevision_l10n::{language, tr, tr_in};

use crate::issue::Issue;
use crate::model::{BlockKind, Flow};
use crate::validate::{is_relative_path, placeholder_fields, placeholders, reachable};

const DELIMITERS: [char; 3] = [',', ';', '\t'];

/// A CSV file read into memory, with what it takes to write it back the same way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataTable {
    /// Names from the first line, without surrounding spaces.
    pub columns: Vec<String>,
    /// Every row has one value per column.
    pub rows: Vec<Vec<String>>,
    /// `,`, `;` or a tab.
    pub delimiter: char,
    /// Lines end with CR LF rather than LF.
    pub crlf: bool,
    /// The file starts with a UTF-8 byte order mark.
    pub bom: bool,
}

/// Why a file can't be used as a data table. Lines are counted from 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataError {
    Unreadable {
        reason: String,
    },
    Unwritable {
        reason: String,
    },
    NotText,
    Empty,
    BlankColumn {
        column: usize,
    },
    DuplicateColumn {
        name: String,
    },
    UnclosedQuote {
        line: usize,
    },
    StrayQuote {
        line: usize,
    },
    RowLength {
        line: usize,
        found: usize,
        expected: usize,
    },
}

impl std::error::Error for DataError {}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let params = self.params();
        let param = |name: &str| {
            params
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, value)| value.clone())
                .unwrap_or_default()
        };
        f.write_str(&data_problem(&language(), self.code(), &param))
    }
}

/// The text of the `data.*` problem `code` in `lang`, with the values `param` gives by name.
pub(crate) fn data_problem(lang: &str, code: &str, param: &dyn Fn(&str) -> String) -> String {
    let p = param;
    match code {
        "data.unreadable" => tr_in!(lang, "data.unreadable", p("reason")),
        "data.unwritable" => tr_in!(lang, "data.unwritable", p("reason")),
        "data.notText" => tr_in!(lang, "data.notText"),
        "data.empty" => tr_in!(lang, "data.empty"),
        "data.blankColumn" => tr_in!(lang, "data.blankColumn", p("column")),
        "data.duplicateColumn" => tr_in!(lang, "data.duplicateColumn", p("name")),
        "data.unclosedQuote" => tr_in!(lang, "data.unclosedQuote", p("line")),
        "data.strayQuote" => tr_in!(lang, "data.strayQuote", p("line")),
        "data.rowLength" => tr_in!(lang, "data.rowLength", p("line"), p("found"), p("expected")),
        _ => p("reason"),
    }
}

impl DataError {
    /// Localization key of the problem, such as `data.rowLength`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unreadable { .. } => "data.unreadable",
            Self::Unwritable { .. } => "data.unwritable",
            Self::NotText => "data.notText",
            Self::Empty => "data.empty",
            Self::BlankColumn { .. } => "data.blankColumn",
            Self::DuplicateColumn { .. } => "data.duplicateColumn",
            Self::UnclosedQuote { .. } => "data.unclosedQuote",
            Self::StrayQuote { .. } => "data.strayQuote",
            Self::RowLength { .. } => "data.rowLength",
        }
    }

    /// Values the localized text of [`Self::code`] refers to.
    pub fn params(&self) -> Vec<(&'static str, String)> {
        match self {
            Self::Unreadable { reason } | Self::Unwritable { reason } => {
                vec![("reason", reason.clone())]
            }
            Self::NotText | Self::Empty => Vec::new(),
            Self::BlankColumn { column } => vec![("column", column.to_string())],
            Self::DuplicateColumn { name } => vec![("name", name.clone())],
            Self::UnclosedQuote { line } | Self::StrayQuote { line } => {
                vec![("line", line.to_string())]
            }
            Self::RowLength {
                line,
                found,
                expected,
            } => vec![
                ("line", line.to_string()),
                ("found", found.to_string()),
                ("expected", expected.to_string()),
            ],
        }
    }
}

impl DataTable {
    pub fn load(path: &Path) -> Result<Self, DataError> {
        let bytes = fs::read(path).map_err(|e| DataError::Unreadable {
            reason: e.to_string(),
        })?;
        Self::parse(std::str::from_utf8(&bytes).map_err(|_| DataError::NotText)?)
    }

    pub fn parse(text: &str) -> Result<Self, DataError> {
        let (bom, text) = match text.strip_prefix('\u{feff}') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let delimiter = detect_delimiter(text);
        let crlf = text.find('\n').is_some_and(|i| text[..i].ends_with('\r'));
        let mut records = records(text, delimiter)?.into_iter();
        let (_, header) = records.next().ok_or(DataError::Empty)?;
        let columns: Vec<String> = header.iter().map(|c| c.trim().to_owned()).collect();
        check_columns(&columns)?;
        let rows = records
            .map(|(line, row)| {
                if row.len() == columns.len() {
                    Ok(row)
                } else {
                    Err(DataError::RowLength {
                        line,
                        found: row.len(),
                        expected: columns.len(),
                    })
                }
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            columns,
            rows,
            delimiter,
            crlf,
            bom,
        })
    }

    /// The table as CSV text with its delimiter, line endings and byte order mark. Values are
    /// quoted only when they must be.
    pub fn to_csv(&self) -> String {
        let end = if self.crlf { "\r\n" } else { "\n" };
        let mut out = String::from(if self.bom { "\u{feff}" } else { "" });
        let columns: Vec<String> = self.columns.iter().map(|c| c.trim().to_owned()).collect();
        for record in std::iter::once(&columns).chain(&self.rows) {
            for (i, value) in record.iter().enumerate() {
                if i > 0 {
                    out.push(self.delimiter);
                }
                // A lone empty value would read back as a blank line, which is skipped.
                if (record.len() == 1 && value.is_empty())
                    || value.contains([self.delimiter, '"', '\r', '\n'])
                {
                    out.push('"');
                    out.push_str(&value.replace('"', "\"\""));
                    out.push('"');
                } else {
                    out.push_str(value);
                }
            }
            out.push_str(end);
        }
        out
    }

    /// Writes the table to `path`. A table with a blank or repeated column name or a row of
    /// the wrong length is not written; rows are counted as lines, the column names being
    /// line 1.
    pub fn save(&self, path: &Path) -> Result<(), DataError> {
        check_columns(&self.columns)?;
        if let Some(i) = self.rows.iter().position(|r| r.len() != self.columns.len()) {
            return Err(DataError::RowLength {
                line: i + 2,
                found: self.rows[i].len(),
                expected: self.columns.len(),
            });
        }
        fs::write(path, self.to_csv()).map_err(|e| DataError::Unwritable {
            reason: e.to_string(),
        })
    }

    /// Value of `column` in row `row`, counted from 0.
    pub fn value(&self, row: usize, column: &str) -> Option<&str> {
        let i = self.columns.iter().position(|c| c == column)?;
        self.rows.get(row)?.get(i).map(String::as_str)
    }

    /// What a row is called in reports: its value in a column named `name` (in any case), or
    /// else in the first column, trimmed. Empty when that value is blank.
    pub fn label(&self, row: usize) -> String {
        let Some(values) = self.rows.get(row) else {
            return String::new();
        };
        let named = self
            .columns
            .iter()
            .position(|c| c.eq_ignore_ascii_case("name"))
            .and_then(|i| values.get(i))
            .filter(|v| !v.trim().is_empty());
        named
            .or(values.first())
            .map_or_else(String::new, |v| v.trim().to_owned())
    }
}

/// The data table a run goes through row by row: the file of the first `useData` block the
/// test comes to from its start, when it names one without placeholders.
pub fn primary_data_file(flow: &Flow) -> Option<&str> {
    let mut seen = HashSet::new();
    let mut stack = vec![flow.start.as_str()];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(block) = flow.blocks.iter().find(|b| b.id == id) else {
            continue;
        };
        if let BlockKind::UseData { file, .. } = &block.kind {
            return (!file.is_empty() && placeholders(file).is_empty()).then_some(file.as_str());
        }
        stack.extend(block.kind.edges().iter().rev().map(|(_, to)| *to));
    }
    None
}

/// Problems with the data tables a test saved in `dir` reads: files that aren't valid tables,
/// and `${data.COLUMN}` placeholders naming a column their tables lack. A placeholder may use
/// the columns of every `useData` block whose loop it runs in, and those of the test's
/// [`primary_data_file`], whose row stays chosen for the whole run. Missing files are left to
/// [`crate::validate_in`].
pub fn validate_data(flow: &Flow, dir: &Path) -> Vec<Issue> {
    let mut out = Vec::new();
    let index: HashMap<&str, usize> = flow
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (b.id.as_str(), i))
        .collect();
    let edges: Vec<Vec<usize>> = flow
        .blocks
        .iter()
        .map(|b| {
            b.kind
                .edges()
                .iter()
                .filter_map(|(_, to)| index.get(to).copied())
                .collect()
        })
        .collect();
    // Each table, `None` when it can't be read, with the blocks inside its loop.
    let mut tables: Vec<(&str, Option<DataTable>, Vec<bool>)> = Vec::new();
    for (i, b) in flow.blocks.iter().enumerate() {
        let BlockKind::UseData { file, body, .. } = &b.kind else {
            continue;
        };
        if !is_relative_path(file) || !placeholders(file).is_empty() {
            continue;
        }
        let mut inside = vec![false; flow.blocks.len()];
        if let Some(&body) = index.get(body.as_str()) {
            let mut cut = edges.clone();
            cut[i].clear();
            inside = reachable(&cut, body);
            inside[i] = false;
        }
        let path = dir.join(file);
        let table = DataTable::load(&path);
        if let Err(e) = &table {
            if path.exists() {
                let issue = Issue::error("flow.data.invalid")
                    .with("block", &b.id)
                    .with("field", "file")
                    .with("path", file)
                    .with("problem", e.code());
                out.push(
                    e.params()
                        .into_iter()
                        .fold(issue, |issue, (name, value)| issue.with(name, value)),
                );
            }
        }
        tables.push((file, table.ok(), inside));
    }
    let primary = primary_data_file(flow);
    for (i, b) in flow.blocks.iter().enumerate() {
        for (field, value) in placeholder_fields(&b.kind) {
            for (_, column) in placeholders(value)
                .into_iter()
                .flatten()
                .filter(|(ns, _)| *ns == "data")
            {
                let usable: Vec<(&str, &Option<DataTable>)> = tables
                    .iter()
                    .filter(|(file, _, inside)| inside[i] || Some(*file) == primary)
                    .map(|(file, table, _)| (*file, table))
                    .collect();
                // Nothing to check against, or a table that can't be read may have the column.
                let usable: Option<Vec<(&str, &DataTable)>> = usable
                    .into_iter()
                    .map(|(file, table)| table.as_ref().map(|t| (file, t)))
                    .collect();
                let Some(usable) = usable.filter(|u| !u.is_empty()) else {
                    continue;
                };
                if usable
                    .iter()
                    .any(|(_, t)| t.columns.iter().any(|c| c == column))
                {
                    continue;
                }
                let mut files: Vec<&str> = usable.iter().map(|(file, _)| *file).collect();
                files.dedup();
                let mut columns: Vec<&str> = Vec::new();
                for c in usable.iter().flat_map(|(_, t)| &t.columns) {
                    if !columns.contains(&c.as_str()) {
                        columns.push(c);
                    }
                }
                out.push(
                    Issue::error("flow.data.unknownColumn")
                        .with("block", &b.id)
                        .with("field", &field)
                        .with("column", column)
                        .with("path", files.join(", "))
                        .with("columns", columns.join(", ")),
                );
            }
        }
    }
    out
}

/// Reads a choice of data rows as people write it: `all`, one row such as `3`, or a range such
/// as `2-4`, rows counted from 1. Returns the rows counted from 0, or `None` for all of them.
pub fn parse_row_choice(text: &str) -> Result<Option<RangeInclusive<usize>>, String> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("all") {
        return Ok(None);
    }
    let row = |s: &str| s.trim().parse::<usize>().ok().filter(|&n| n > 0);
    let range = match text.split_once('-') {
        Some((first, last)) => row(first).zip(row(last)),
        None => row(text).map(|n| (n, n)),
    };
    match range {
        Some((first, last)) if first <= last => Ok(Some(first - 1..=last - 1)),
        Some((first, last)) => Err(tr!("data.rows.backwards", text, last, first)),
        None => Err(tr!("data.rows.invalid", text)),
    }
}

/// Every column has a name of its own, spaces around it aside.
fn check_columns(columns: &[String]) -> Result<(), DataError> {
    for (i, name) in columns.iter().enumerate() {
        let name = name.trim();
        if name.is_empty() {
            return Err(DataError::BlankColumn { column: i + 1 });
        }
        if columns[..i].iter().any(|c| c.trim() == name) {
            return Err(DataError::DuplicateColumn {
                name: name.to_owned(),
            });
        }
    }
    Ok(())
}

/// The delimiter the first line uses most, outside quotes; a comma when it uses none.
fn detect_delimiter(text: &str) -> char {
    let mut counts = [0usize; DELIMITERS.len()];
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            '\r' | '\n' if !quoted => break,
            _ if !quoted => {
                if let Some(i) = DELIMITERS.iter().position(|&d| d == c) {
                    counts[i] += 1;
                }
            }
            _ => {}
        }
    }
    // The earliest delimiter in the list wins a tie.
    let best = (0..DELIMITERS.len())
        .rev()
        .max_by_key(|&i| counts[i])
        .unwrap_or(0);
    if counts[best] == 0 {
        ','
    } else {
        DELIMITERS[best]
    }
}

/// Records of `text` with the line each starts on; blank lines are left out.
fn records(text: &str, delimiter: char) -> Result<Vec<(usize, Vec<String>)>, DataError> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    let mut line = 1;
    while chars.peek().is_some() {
        let start = line;
        let mut record = Vec::new();
        let mut quoted_any = false;
        loop {
            let mut value = String::new();
            if chars.peek() == Some(&'"') {
                quoted_any = true;
                let opened = line;
                chars.next();
                loop {
                    match chars.next() {
                        None => return Err(DataError::UnclosedQuote { line: opened }),
                        Some('"') if chars.peek() == Some(&'"') => {
                            chars.next();
                            value.push('"');
                        }
                        Some('"') => break,
                        Some(c) => {
                            if c == '\n' {
                                line += 1;
                            }
                            value.push(c);
                        }
                    }
                }
                match chars.peek() {
                    None | Some('\r' | '\n') => {}
                    Some(&c) if c == delimiter => {}
                    Some(_) => return Err(DataError::StrayQuote { line }),
                }
            } else {
                while let Some(&c) = chars.peek() {
                    if c == delimiter || c == '\r' || c == '\n' {
                        break;
                    }
                    if c == '"' {
                        return Err(DataError::StrayQuote { line });
                    }
                    value.push(c);
                    chars.next();
                }
            }
            record.push(value);
            match chars.next() {
                Some('\r') => {
                    chars.next_if_eq(&'\n');
                    line += 1;
                    break;
                }
                Some('\n') => {
                    line += 1;
                    break;
                }
                Some(_) => {}
                None => break,
            }
        }
        let blank = !quoted_any && record.len() == 1 && record[0].is_empty();
        if !blank {
            out.push((start, record));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> DataTable {
        DataTable::parse(text).unwrap()
    }

    #[test]
    fn reads_quotes_bom_and_crlf() {
        let t = table("\u{feff}email, name\r\na@x.io,\"Doe, \"\"J\"\"\"\r\nb@x.io,Bo\r\n");
        assert_eq!(t.columns, ["email", "name"]);
        assert_eq!(t.value(0, "name"), Some("Doe, \"J\""));
        assert_eq!(t.value(1, "email"), Some("b@x.io"));
        assert_eq!(t.value(2, "email"), None);
        assert!(t.bom && t.crlf);
        assert_eq!(t.delimiter, ',');
    }

    #[test]
    fn keeps_line_breaks_and_empty_values_and_skips_blank_lines() {
        let t = table("a,b\n\"one\ntwo\",\n\n,x\n");
        assert_eq!(t.rows, [["one\ntwo", ""], ["", "x"]]);
        assert!(!t.crlf && !t.bom);
        // Without a final line break the last row still counts.
        assert_eq!(table("a\n1").rows, [["1"]]);
        // A quoted empty value is a value, not a blank line.
        assert_eq!(table("a\n\"\"\n").rows, [[""]]);
    }

    #[test]
    fn detects_the_delimiter_from_the_first_line() {
        assert_eq!(table("a;b\n1,5;2\n").rows, [["1,5", "2"]]);
        assert_eq!(table("a\tb\tc;d\n1\t2\t3;4\n").delimiter, '\t');
        assert_eq!(table("\"x;y\",b\n1,2\n").delimiter, ',');
        assert_eq!(
            table("a,b;c\n1,2;3\n").delimiter,
            ',',
            "a tie goes to the comma"
        );
        assert_eq!(table("only\nv\n").delimiter, ',');
    }

    #[test]
    fn names_problems_with_their_line() {
        let err = |text: &str| DataTable::parse(text).unwrap_err();
        assert_eq!(err(""), DataError::Empty);
        assert_eq!(err("\n\n"), DataError::Empty);
        assert_eq!(err("a,,b\n"), DataError::BlankColumn { column: 2 });
        assert_eq!(
            err("a, b ,b\n"),
            DataError::DuplicateColumn { name: "b".into() }
        );
        assert_eq!(
            err("a\n1\n\"open\n\n"),
            DataError::UnclosedQuote { line: 3 }
        );
        assert_eq!(err("a\n\"x\ny\"z\n"), DataError::StrayQuote { line: 3 });
        assert_eq!(err("a\n5\" screen\n"), DataError::StrayQuote { line: 2 });
        assert_eq!(
            err("a,b\n1,2\n\"x\ny\"\n"),
            DataError::RowLength {
                line: 3,
                found: 1,
                expected: 2
            }
        );
        assert_eq!(
            err("a,b\n1,2\n\"x\ny\"\n").to_string(),
            "line 3 has 1 values, but the first line names 2 columns"
        );
        assert_eq!(err("a\n5\" screen\n").code(), "data.strayQuote");
    }

    #[test]
    fn writes_back_what_it_read() {
        for text in [
            "email;name\r\na@x.io;\"Doe; \"\"J\"\"\"\r\n",
            "\u{feff}a\tb\n\"one\ntwo\"\t\n",
            "a\n\"\"\nx\n",
        ] {
            assert_eq!(table(text).to_csv(), text);
        }
        let mut t = table("a,b\n1,2\n");
        t.columns.push("c".into());
        t.rows[0].push("x,y".into());
        t.rows.push(vec!["".into(), "".into(), "".into()]);
        assert_eq!(t.to_csv(), "a,b,c\n1,2,\"x,y\"\n,,\n");
    }

    #[test]
    fn saves_only_tables_that_read_back() {
        let dir = std::env::temp_dir().join(format!("jevision-data-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rows.csv");
        let mut t = table("a,b\n1,2\n");
        t.save(&path).unwrap();
        assert_eq!(DataTable::load(&path).unwrap(), t);

        t.columns[1] = "a".into();
        assert_eq!(
            t.save(&path),
            Err(DataError::DuplicateColumn { name: "a".into() })
        );
        t.columns[1] = " ".into();
        assert_eq!(t.save(&path), Err(DataError::BlankColumn { column: 2 }));
        t.columns[1] = " b ".into();
        t.save(&path).unwrap();
        assert_eq!(DataTable::load(&path).unwrap().columns, ["a", "b"]);
        t.columns[1] = "b".into();
        t.rows[0].pop();
        assert!(matches!(
            t.save(&path),
            Err(DataError::RowLength { line: 2, .. })
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "a,b\n1,2\n");

        fs::write(&path, [0xff, 0xfe]).unwrap();
        assert_eq!(DataTable::load(&path), Err(DataError::NotText));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn finds_columns_that_tables_lack() {
        let dir = std::env::temp_dir().join(format!("jevision-data-v-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("users.csv"), "email,password\na,b\n").unwrap();
        fs::write(dir.join("langs.csv"), "lang\nen\n").unwrap();
        fs::write(dir.join("bad.csv"), "a,a\n").unwrap();
        let flow = crate::parse(
            r#"{"format":1,"name":"T","app":"com.example.App","start":"users","blocks":{
                "users":{"type":"useData","file":"users.csv","body":"langs","next":"after"},
                "langs":{"type":"useData","file":"langs.csv","body":"in"},
                "in":{"type":"type","text":"${data.lang} ${data.email} ${data.nope}","next":"langs"},
                "after":{"type":"type","text":"${data.email} ${data.lang}","next":"bad"},
                "bad":{"type":"useData","file":"bad.csv","body":"b"},
                "b":{"type":"type","text":"${data.a}","next":"bad"}}}"#,
        )
        .unwrap();
        assert_eq!(primary_data_file(&flow), Some("users.csv"));
        let issues = validate_data(&flow, &dir);
        let found: Vec<(&str, &str, &str)> = issues
            .iter()
            .map(|i| {
                (
                    i.code.as_str(),
                    i.param("block").unwrap(),
                    i.param("columns").unwrap_or(""),
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                ("flow.data.invalid", "bad", ""),
                ("flow.data.unknownColumn", "after", "email, password"),
                ("flow.data.unknownColumn", "in", "lang, email, password"),
            ]
        );
        assert_eq!(issues[0].param("problem"), Some("data.duplicateColumn"));
        assert_eq!(issues[0].param("name"), Some("a"));
        assert_eq!(issues[1].param("column"), Some("lang"));
        assert_eq!(issues[1].param("path"), Some("users.csv"));
        assert_eq!(issues[2].param("column"), Some("nope"));
        assert_eq!(issues[2].param("path"), Some("langs.csv, users.csv"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_row_choices() {
        assert_eq!(parse_row_choice(" all "), Ok(None));
        assert_eq!(parse_row_choice("3"), Ok(Some(2..=2)));
        assert_eq!(parse_row_choice("2-4"), Ok(Some(1..=3)));
        for bad in ["0", "", "x", "4-2", "1-", "-1", "1-2-3"] {
            assert!(parse_row_choice(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn labels_rows_by_name_or_first_column() {
        let t = table("email,Name\na@x.io, Ann \nb@x.io,\n,\n");
        assert_eq!(t.label(0), "Ann");
        assert_eq!(t.label(1), "b@x.io");
        assert_eq!(t.label(2), "");
        assert_eq!(t.label(9), "");
    }

    #[test]
    fn problems_read_in_the_language_asked_for() {
        let error = DataError::RowLength {
            line: 3,
            found: 1,
            expected: 2,
        };
        assert_eq!(
            error.to_string(),
            "line 3 has 1 values, but the first line names 2 columns"
        );
        let params = error.params();
        let param = |name: &str| {
            params
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, value)| value.clone())
                .unwrap_or_default()
        };
        assert_eq!(
            data_problem("ru", error.code(), &param),
            "в строке 3 значений: 1, а в первой строке названо столбцов: 2"
        );
        assert_eq!(
            parse_row_choice("4-2"),
            Err("“4-2” ends before it starts; write the smaller row first, like 2-4.".into())
        );
        assert_eq!(
            jevision_l10n::tr_in!("ru", "data.rows.backwards", "4-2", 2, 4),
            "«4-2» заканчивается раньше, чем начинается: сначала напишите меньшую строку, например 2-4."
        );
    }
}
