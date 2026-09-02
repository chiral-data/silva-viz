// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Delimited text as a table.

use super::{line, line_count, line_offsets};
use egui::TextStyle;
use egui_extras::{Column, TableBuilder};
use silva_viz_core::{Blob, Claim, FileProbe, View, ViewerFactory};

/// The same ceiling the text viewer uses, for the same reason: the file is
/// decoded whole before anything is shown.
const TABLE_LIMIT: u64 = 8 * 1024 * 1024;

/// How many lines of the head are checked before believing a delimiter.
const SNIFF_LINES: usize = 5;

/// Splits one record, honouring double quotes.
///
/// Not a CSV library. It handles the quoting rule that actually bites — a
/// delimiter inside `"..."` — and the doubled `""` escape, and stops there;
/// anything more (embedded newlines, alternative escapes) is a reason to reach
/// for a parser rather than to grow this.
fn split_record(record: &str, delimiter: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = record.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == delimiter && !quoted => fields.push(std::mem::take(&mut field)),
            c => field.push(c),
        }
    }
    fields.push(field);
    fields
}

/// The delimiter this file uses, if its first few lines agree on one.
///
/// Agreement is the whole test. A prose file contains commas too; what it does
/// not do is contain the *same number* of them on five lines running.
fn sniff(head: &str, ext: Option<&str>) -> Option<char> {
    let offsets = line_offsets(head);
    let total = line_count(&offsets);
    // The last line of a head is usually cut mid-record, so it is not evidence.
    let usable = total.saturating_sub(1).min(SNIFF_LINES);
    if usable < 2 {
        return None;
    }

    let candidates: &[char] = match ext {
        Some("csv") => &[','],
        Some("tsv") | Some("tab") => &['\t'],
        _ => &[',', '\t', ';'],
    };

    candidates.iter().copied().find(|&delimiter| {
        let counts: Vec<usize> = (0..usable)
            .map(|n| split_record(line(head, &offsets, n), delimiter).len())
            .collect();
        counts[0] >= 2 && counts.iter().all(|c| *c == counts[0])
    })
}

pub struct TableFactory;

impl ViewerFactory for TableFactory {
    fn id(&self) -> &'static str {
        "table"
    }

    fn claim(&self, probe: &FileProbe<'_>) -> Option<Claim> {
        if probe.size() > TABLE_LIMIT || probe.has_nul() || !probe.head_is_utf8() {
            return None;
        }
        let head = String::from_utf8_lossy(probe.head());
        let delimiter = sniff(&head, probe.ext())?;
        let columns = split_record(line(&head, &line_offsets(&head), 0), delimiter).len();
        Some(Claim::new(format!("Table ({columns} columns)"), 10))
    }

    fn open(&self, blob: Blob) -> Box<dyn View> {
        Box::new(TableView::new(blob))
    }
}

pub struct TableView {
    name: String,
    text: String,
    offsets: Vec<usize>,
    delimiter: char,
    header: Vec<String>,
    error: Option<String>,
}

impl TableView {
    fn new(blob: Blob) -> Self {
        let (text, error) = match blob.read_all() {
            Ok(bytes) => (String::from_utf8_lossy(&bytes).into_owned(), None),
            Err(e) => (String::new(), Some(e.to_string())),
        };
        let offsets = line_offsets(&text);
        // Re-sniffed on the whole file rather than trusted from the claim: the
        // factory and the view are separate calls, and a view that took the
        // delimiter on faith would be wrong the day someone opens a file in
        // this viewer from the menu without it having bid.
        let delimiter = sniff(&text, None).unwrap_or(',');
        let header = split_record(line(&text, &offsets, 0), delimiter);
        Self {
            name: blob.name().to_string(),
            text,
            offsets,
            delimiter,
            header,
            error,
        }
    }

    fn body_rows(&self) -> usize {
        line_count(&self.offsets).saturating_sub(1)
    }
}

impl View for TableView {
    fn title(&self) -> String {
        format!(
            "{} — {} x {}",
            self.name,
            self.body_rows(),
            self.header.len()
        )
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
            return;
        }

        let row_h = ui.text_style_height(&TextStyle::Body) + 4.0;
        let rows = self.body_rows();
        let mut table = TableBuilder::new(ui).striped(true).column(Column::auto());
        for _ in 0..self.header.len() {
            table = table.column(Column::auto().at_least(60.0).clip(true));
        }

        table
            .header(row_h, |mut header| {
                header.col(|ui| {
                    ui.strong("#");
                });
                for name in &self.header {
                    header.col(|ui| {
                        ui.strong(name);
                    });
                }
            })
            .body(|body| {
                body.rows(row_h, rows, |mut row| {
                    // Row 0 of the file is the header, so the body starts at 1.
                    let n = row.index() + 1;
                    let fields = split_record(line(&self.text, &self.offsets, n), self.delimiter);
                    row.col(|ui| {
                        ui.weak(format!("{n}"));
                    });
                    for column in 0..self.header.len() {
                        row.col(|ui| {
                            // A ragged row is shown as short rather than
                            // shifting every later column left by one.
                            ui.label(fields.get(column).map(String::as_str).unwrap_or(""));
                        });
                    }
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a_delimiter_inside_quotes_does_not_split_the_field() {
        assert_eq!(
            split_record(r#"a,"b,c",d"#, ','),
            ["a", "b,c", "d"].map(String::from)
        );
    }

    #[test]
    fn test_a_doubled_quote_is_one_quote() {
        assert_eq!(
            split_record(r#""say ""hi""",x"#, ','),
            [r#"say "hi""#, "x"].map(String::from)
        );
    }

    #[test]
    fn test_consistent_columns_are_what_identify_a_table() {
        let csv = "a,b,c\n1,2,3\n4,5,6\n7,8,9\n";
        assert_eq!(sniff(csv, Some("csv")), Some(','));
    }

    #[test]
    fn test_prose_full_of_commas_is_not_a_table() {
        // Every line has commas; no two lines have the same number of them.
        let prose = "one, two\nthree\nfour, five, six\nseven, eight\n";
        assert_eq!(sniff(prose, None), None);
    }

    #[test]
    fn test_a_single_column_file_is_not_a_table() {
        // A .smi or a word list has one field per line. Showing it as a
        // one-column table would be worse than showing it as text.
        assert_eq!(sniff("alpha\nbeta\ngamma\ndelta\n", None), None);
        assert!(
            TableFactory
                .claim(&FileProbe::new("a.smi", b"alpha\nbeta\ngamma\n", 17))
                .is_none()
        );
    }

    #[test]
    fn test_a_tsv_is_recognised_from_its_tabs() {
        let tsv = "a\tb\n1\t2\n3\t4\n";
        assert_eq!(sniff(tsv, Some("tsv")), Some('\t'));
    }

    #[test]
    fn test_the_claim_says_how_many_columns_it_found() {
        let probe = FileProbe::new("d.csv", b"a,b,c\n1,2,3\n4,5,6\n", 18);
        let claim = TableFactory.claim(&probe).expect("a csv should be claimed");
        assert_eq!(claim.label, "Table (3 columns)");
        assert_eq!(claim.priority, 10);
    }
}
