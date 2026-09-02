// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What is known about a file without opening it in anything.
//!
//! Never the default view — it bids lowest of all five. It exists for the
//! moment a file did not open the way you expected, and it answers that by
//! showing the same evidence the factories bid on.

use super::{human_size, text};
use silva_viz_core::{Blob, Claim, FileProbe, View, ViewerFactory};

pub struct MetaFactory;

impl ViewerFactory for MetaFactory {
    fn id(&self) -> &'static str {
        "meta"
    }

    fn claim(&self, _probe: &FileProbe<'_>) -> Option<Claim> {
        Some(Claim::new("Metadata", -200))
    }

    fn open(&self, blob: Blob) -> Box<dyn View> {
        Box::new(MetaView { blob })
    }
}

pub struct MetaView {
    blob: Blob,
}

impl View for MetaView {
    fn title(&self) -> String {
        format!("{} — metadata", self.blob.name())
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        let probe = self.blob.probe();

        egui::Grid::new("metadata")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                let mut row = |label: &str, value: String| {
                    ui.strong(label);
                    ui.label(value);
                    ui.end_row();
                };
                row("name", probe.name().to_string());
                row("path", self.blob.path().to_string());
                row(
                    "size",
                    format!("{} ({} bytes)", human_size(probe.size()), probe.size()),
                );
                row("extension", probe.ext().unwrap_or("(none)").to_string());
                row(
                    "looks like",
                    match probe.image_kind() {
                        Some(kind) => format!("{} image", kind.label()),
                        None if probe.has_nul() => "binary (NUL bytes in the first 4 KiB)".into(),
                        None if probe.head_is_utf8() => "UTF-8 text".into(),
                        None => "binary".into(),
                    },
                );
                // The reason a file the user expected as text opened as hex.
                row(
                    "text viewer",
                    match text::verdict(&probe) {
                        Ok(()) => "accepts".to_string(),
                        Err(reason) => format!("declines — {reason}"),
                    },
                );
            });

        ui.add_space(8.0);
        ui.strong("first bytes");
        let preview: String = probe
            .head()
            .iter()
            .take(32)
            .map(|b| format!("{b:02x} "))
            .collect();
        ui.monospace(if preview.is_empty() {
            "(empty file)".to_string()
        } else {
            preview
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_bids_on_everything_but_never_wins() {
        let probe = FileProbe::new("a.txt", b"hello", 5);
        let claim = MetaFactory.claim(&probe).expect("metadata always bids");
        // Below the hex viewer's floor, so it is last in every menu.
        assert!(claim.priority < -100);
    }
}
