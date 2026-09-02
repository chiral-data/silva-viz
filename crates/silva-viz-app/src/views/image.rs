// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Raster images, recognised by their magic bytes.

use super::human_size;
use silva_viz_core::{Blob, Claim, FileProbe, ImageKind, View, ViewerFactory};
use std::sync::Arc;

/// Decoding is done by `egui_extras`' image loader, so this ceiling is about
/// the decoded surface rather than the file: a 60 MB PNG is a texture no one
/// asked for.
const IMAGE_LIMIT: u64 = 64 * 1024 * 1024;

pub struct ImageFactory;

impl ViewerFactory for ImageFactory {
    fn id(&self) -> &'static str {
        "image"
    }

    fn claim(&self, probe: &FileProbe<'_>) -> Option<Claim> {
        if probe.size() > IMAGE_LIMIT {
            return None;
        }
        // By bytes, never by extension. This is what opens a `.txt` that is
        // really a PNG as a picture, and refuses a `.png` that is really text.
        let kind = probe.image_kind()?;
        Some(Claim::new(kind.label(), 20))
    }

    fn open(&self, blob: Blob) -> Box<dyn View> {
        Box::new(ImageView::new(blob))
    }
}

pub struct ImageView {
    name: String,
    kind: Option<ImageKind>,
    /// egui's loader caches by URI, so this has to be unique per file or two
    /// open images would show the same picture.
    uri: String,
    bytes: Option<Arc<[u8]>>,
    error: Option<String>,
    zoom: f32,
}

impl ImageView {
    fn new(blob: Blob) -> Self {
        let kind = blob.probe().image_kind();
        let (bytes, error) = match blob.read_all() {
            Ok(bytes) => (Some(Arc::from(bytes.into_boxed_slice())), None),
            Err(e) => (None, Some(e.to_string())),
        };
        Self {
            name: blob.name().to_string(),
            kind,
            uri: format!("bytes://{}", blob.path()),
            bytes,
            error,
            zoom: 1.0,
        }
    }
}

impl View for ImageView {
    fn title(&self) -> String {
        match self.kind {
            Some(kind) => format!("{} — {}", self.name, kind.label()),
            None => self.name.clone(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
            return;
        }
        let Some(bytes) = self.bytes.clone() else {
            return;
        };

        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut self.zoom, 0.1..=8.0).text("zoom"));
            if ui.button("Fit").clicked() {
                self.zoom = 1.0;
            }
            ui.weak(human_size(bytes.len() as u64));
        });
        ui.separator();

        let width = ui.available_width() * self.zoom;
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(
                    egui::Image::new(egui::ImageSource::Bytes {
                        uri: self.uri.clone().into(),
                        bytes: bytes.into(),
                    })
                    .max_width(width)
                    // Off, so `max_width` is a ceiling rather than a target:
                    // a 16x16 icon should stay 16x16, not fill the window.
                    .maintain_aspect_ratio(true)
                    .fit_to_original_size(1.0),
                );
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a_png_is_claimed_whatever_it_is_called() {
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";
        for name in ["real.png", "pretend.txt", "no-extension"] {
            let claim = ImageFactory
                .claim(&FileProbe::new(name, png, png.len() as u64))
                .expect("PNG magic should be claimed");
            assert_eq!(claim.label, "PNG");
        }
    }

    #[test]
    fn test_a_png_extension_over_text_is_refused() {
        let probe = FileProbe::new("lying.png", b"this is plainly not a png", 25);
        assert!(ImageFactory.claim(&probe).is_none());
    }

    #[test]
    fn test_an_image_beyond_the_ceiling_is_left_to_the_hex_viewer() {
        let png = b"\x89PNG\r\n\x1a\n";
        let probe = FileProbe::new("enormous.png", png, IMAGE_LIMIT + 1);
        assert!(ImageFactory.claim(&probe).is_none());
    }

    #[test]
    fn test_it_outbids_text_so_a_double_click_shows_the_picture() {
        // Priorities are only meaningful relative to each other, so the one
        // relationship that matters is pinned here rather than assumed.
        let png = b"\x89PNG\r\n\x1a\n";
        let probe = FileProbe::new("a.png", png, 8);
        assert!(ImageFactory.claim(&probe).unwrap().priority > 0);
    }
}
