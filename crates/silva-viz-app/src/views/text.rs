// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Text, virtualised by line.

use super::{line, line_count, line_offsets};
use egui::TextStyle;
use silva_viz_core::{Blob, Claim, FileProbe, View, ViewerFactory};

/// Above this, the text viewer declines and the hex viewer takes the file.
///
/// The number is about the *decoding*, not the display: `read_all` plus a UTF-8
/// validation of a gigabyte is a freeze the user cannot cancel, and the reason
/// the limit is small is that nobody reads a gigabyte of text in a window.
pub const TEXT_LIMIT: u64 = 8 * 1024 * 1024;

/// Whether the text viewer will take this file, and if not, why not.
///
/// Shared with the metadata viewer so that a file the text viewer refused can
/// say so on screen, rather than the user guessing why the menu is short.
pub fn verdict(probe: &FileProbe<'_>) -> Result<(), String> {
    if probe.size() > TEXT_LIMIT {
        return Err(format!(
            "larger than the {} limit",
            super::human_size(TEXT_LIMIT)
        ));
    }
    if probe.has_nul() {
        return Err("contains NUL bytes".to_string());
    }
    if !probe.head_is_utf8() {
        return Err("not valid UTF-8".to_string());
    }
    Ok(())
}

pub struct TextFactory;

impl ViewerFactory for TextFactory {
    fn id(&self) -> &'static str {
        "text"
    }

    fn claim(&self, probe: &FileProbe<'_>) -> Option<Claim> {
        verdict(probe).ok()?;
        Some(Claim::new("Text", 0))
    }

    fn open(&self, blob: Blob) -> Box<dyn View> {
        Box::new(TextView::new(blob))
    }
}

pub struct TextView {
    name: String,
    text: String,
    offsets: Vec<usize>,
    error: Option<String>,
    wrap: bool,
}

impl TextView {
    fn new(blob: Blob) -> Self {
        let (text, error) = match blob.read_all() {
            // Lossy rather than a hard failure: the head was valid UTF-8 or the
            // factory would not have bid, so a bad byte further in is a local
            // defect in an otherwise readable file. Showing the rest beats
            // showing nothing.
            Ok(bytes) => (String::from_utf8_lossy(&bytes).into_owned(), None),
            Err(e) => (String::new(), Some(e.to_string())),
        };
        let offsets = line_offsets(&text);
        Self {
            name: blob.name().to_string(),
            text,
            offsets,
            error,
            wrap: false,
        }
    }
}

impl View for TextView {
    fn title(&self) -> String {
        format!("{} — {} lines", self.name, line_count(&self.offsets))
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
            return;
        }

        ui.horizontal(|ui| {
            ui.checkbox(&mut self.wrap, "Wrap");
        });
        ui.separator();

        let row_h = ui.text_style_height(&TextStyle::Monospace);
        let total = line_count(&self.offsets);
        let gutter = format!("{total}").len();

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show_rows(ui, row_h, total, |ui, rows| {
                for n in rows {
                    let body = line(&self.text, &self.offsets, n);
                    ui.horizontal(|ui| {
                        ui.add_enabled(
                            false,
                            egui::Label::new(
                                egui::RichText::new(format!("{:>gutter$}", n + 1)).monospace(),
                            ),
                        );
                        let text = egui::RichText::new(body).monospace();
                        // Wrapping breaks the one-row-per-line contract that
                        // `show_rows` is built on, so a wrapped long line
                        // overlaps its neighbour. Truncating instead keeps the
                        // rows honest, and the horizontal scrollbar is the way
                        // to see the rest.
                        ui.add(egui::Label::new(text).wrap_mode(if self.wrap {
                            egui::TextWrapMode::Truncate
                        } else {
                            egui::TextWrapMode::Extend
                        }));
                    });
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_text_is_claimed() {
        let probe = FileProbe::new("a.txt", b"hello\nworld\n", 12);
        assert!(verdict(&probe).is_ok());
        assert!(TextFactory.claim(&probe).is_some());
    }

    #[test]
    fn test_a_png_pretending_to_be_a_txt_is_refused_by_its_bytes() {
        // The case the whole sniffing design exists for: an extension check
        // would open this and paint a screenful of replacement characters.
        let probe = FileProbe::new("pretend.txt", b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR", 20);
        assert_eq!(verdict(&probe).unwrap_err(), "contains NUL bytes");
        assert!(TextFactory.claim(&probe).is_none());
    }

    #[test]
    fn test_a_file_over_the_limit_is_refused_and_says_so() {
        let probe = FileProbe::new("huge.log", b"plain text", TEXT_LIMIT + 1);
        assert_eq!(
            verdict(&probe).unwrap_err(),
            "larger than the 8.0 MiB limit"
        );
    }

    #[test]
    fn test_a_file_exactly_at_the_limit_is_still_taken() {
        let probe = FileProbe::new("big.log", b"plain text", TEXT_LIMIT);
        assert!(verdict(&probe).is_ok());
    }
}
