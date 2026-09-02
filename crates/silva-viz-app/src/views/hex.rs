// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A hex dump of a file of any size.
//!
//! This is the floor of the registry: it bids on everything, at a priority
//! below every other viewer, so no file is ever un-openable. It is also the
//! only viewer that never calls `read_all` — it pages, which is what lets it
//! open a file larger than memory.

use super::human_size;
use egui::TextStyle;
use egui_extras::{Column, TableBuilder};
use silva_viz_core::{Blob, Claim, FileProbe, View, ViewerFactory};

const BYTES_PER_ROW: u64 = 16;

/// How much is read on a cache miss.
///
/// Forty visible rows are 640 bytes, so one read serves a screenful with room
/// to scroll; making it much larger would just stall the first frame.
const CHUNK: usize = 64 * 1024;

pub struct HexFactory;

impl ViewerFactory for HexFactory {
    fn id(&self) -> &'static str {
        "hex"
    }

    fn claim(&self, _probe: &FileProbe<'_>) -> Option<Claim> {
        Some(Claim::new("Hex", -100))
    }

    fn open(&self, blob: Blob) -> Box<dyn View> {
        Box::new(HexView {
            blob,
            chunk_at: 0,
            chunk: Vec::new(),
            error: None,
        })
    }
}

pub struct HexView {
    blob: Blob,
    chunk_at: u64,
    chunk: Vec<u8>,
    error: Option<String>,
}

impl HexView {
    /// The 16 bytes at `offset`, reading a fresh chunk only when the cached one
    /// does not already cover them.
    fn row(&mut self, offset: u64) -> &[u8] {
        let covered = offset >= self.chunk_at
            && offset + BYTES_PER_ROW <= self.chunk_at + self.chunk.len() as u64;
        if !covered {
            // Aligned to the chunk size so that scrolling backwards does not
            // re-read a slightly different window on every row.
            let start = offset - (offset % CHUNK as u64);
            match self.blob.read_range(start, CHUNK) {
                Ok(bytes) => {
                    self.chunk_at = start;
                    self.chunk = bytes;
                    self.error = None;
                }
                Err(e) => {
                    self.error = Some(e.to_string());
                    self.chunk_at = start;
                    self.chunk = Vec::new();
                }
            }
        }
        let lo = (offset - self.chunk_at) as usize;
        let hi = (lo + BYTES_PER_ROW as usize).min(self.chunk.len());
        self.chunk.get(lo..hi).unwrap_or(&[])
    }
}

fn hex_columns(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(BYTES_PER_ROW as usize * 3 + 1);
    for i in 0..BYTES_PER_ROW as usize {
        if i == BYTES_PER_ROW as usize / 2 {
            out.push(' ');
        }
        match bytes.get(i) {
            Some(b) => out.push_str(&format!("{b:02x} ")),
            // Padded rather than short so the ASCII column of the final row
            // stays lined up with every row above it.
            None => out.push_str("   "),
        }
    }
    out
}

fn ascii_column(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| {
            if b.is_ascii_graphic() || *b == b' ' {
                *b as char
            } else {
                '.'
            }
        })
        .collect()
}

impl View for HexView {
    fn title(&self) -> String {
        format!("{} — {}", self.blob.name(), human_size(self.blob.size()))
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }

        let rows = self.blob.size().div_ceil(BYTES_PER_ROW).max(1);
        // `show_rows`/`body.rows` take a `usize` count. A file with more than
        // `usize::MAX` rows cannot exist on a machine that could address it, so
        // the saturating cast is a formality rather than a truncation.
        let rows = usize::try_from(rows).unwrap_or(usize::MAX);
        let row_h = ui.text_style_height(&TextStyle::Monospace);

        TableBuilder::new(ui)
            .striped(true)
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::remainder())
            .header(row_h, |mut header| {
                for title in ["offset", "bytes", "ascii"] {
                    header.col(|ui| {
                        ui.strong(title);
                    });
                }
            })
            .body(|body| {
                body.rows(row_h, rows, |mut row| {
                    let offset = row.index() as u64 * BYTES_PER_ROW;
                    let bytes = self.row(offset).to_vec();
                    row.col(|ui| {
                        ui.monospace(format!("{offset:08x}"));
                    });
                    row.col(|ui| {
                        ui.monospace(hex_columns(&bytes));
                    });
                    row.col(|ui| {
                        ui.monospace(ascii_column(&bytes));
                    });
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_bids_on_everything_so_no_file_is_unopenable() {
        for (name, head) in [
            ("a.txt", &b"hi"[..]),
            ("x.bin", &[0u8, 255][..]),
            ("", &[][..]),
        ] {
            assert!(HexFactory.claim(&FileProbe::new(name, head, 2)).is_some());
        }
    }

    #[test]
    fn test_a_short_final_row_is_padded_so_the_ascii_column_stays_aligned() {
        let full = hex_columns(&[0u8; 16]);
        let short = hex_columns(&[0u8; 3]);
        assert_eq!(full.len(), short.len());
    }

    #[test]
    fn test_unprintable_bytes_become_dots_rather_than_control_characters() {
        assert_eq!(ascii_column(b"ab\x00\x1f c"), "ab.. c");
    }
}
