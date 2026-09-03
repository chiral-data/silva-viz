// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Ported from `crates/chem-app/src/structure_view.rs` in
// https://github.com/chiral-data/rust-chem, which is MIT licensed
// (Copyright (c) 2021 Chiral Ltd.). The MIT licence permits this; its notice
// is retained here because it requires that.

//! The egui half of 2D structure depiction.
//!
//! Describing a structure is [`chem::draw`]'s job and needs no toolkit. This is
//! the backend that paints the description, plus the two pieces that can only
//! come from the GUI: measuring a label with the font that will actually draw
//! it, and following the app's light/dark setting.

use chem::core::molecule::Molecule;
use chem::draw::{StructureOptions, StructureShape, StructureTheme, describe_structure};
use egui::{Color32, FontId, Response, Sense, Shape, Stroke, Ui, Vec2, Widget};

/// A widget drawing one molecule's 2D structure.
///
/// A molecule without coordinates renders a placeholder rather than an empty
/// rectangle — [`describe_structure`] emits the text itself — so the reason a
/// panel looks blank is visible on it.
pub struct StructureView<'a> {
    molecule: &'a Molecule,
    options: StructureOptions,
    desired_size: Vec2,
}

impl<'a> StructureView<'a> {
    pub fn new(molecule: &'a Molecule, desired_size: Vec2) -> Self {
        Self {
            molecule,
            options: StructureOptions::default(),
            desired_size,
        }
    }

    pub fn with_options(mut self, options: StructureOptions) -> Self {
        self.options = options;
        self
    }
}

/// A palette following egui's current visuals, so structures track the app's
/// theme rather than carrying a setting of their own.
///
/// A free function rather than a `StructureTheme::from_visuals` method: the type
/// belongs to `chem`, and an inherent method would have to live there — which
/// would put `egui` back into the crate that was built without it.
pub fn theme_from_visuals(visuals: &egui::Visuals) -> StructureTheme {
    if visuals.dark_mode {
        StructureTheme::dark()
    } else {
        StructureTheme::light()
    }
}

/// Paints a description with egui.
fn paint_structure(painter: &egui::Painter, shapes: &[StructureShape]) {
    for shape in shapes {
        match shape {
            StructureShape::Line {
                from,
                to,
                width,
                color,
            } => {
                painter.line_segment([*from, *to], Stroke::new(*width, *color));
            }
            StructureShape::DashedLine {
                from,
                to,
                width,
                color,
                dash,
            } => {
                painter.extend(Shape::dashed_line(
                    &[*from, *to],
                    Stroke::new(*width, *color),
                    *dash,
                    *dash,
                ));
            }
            StructureShape::Disc {
                center,
                radius,
                color,
            } => {
                painter.circle_filled(*center, *radius, *color);
            }
            StructureShape::Text {
                pos,
                align,
                text,
                size,
                color,
            } => {
                painter.text(*pos, *align, text, FontId::proportional(*size), *color);
            }
        }
    }
}

impl Widget for StructureView<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, response) = ui.allocate_exact_size(self.desired_size, Sense::hover());

        if !ui.is_rect_visible(rect) {
            return response;
        }

        let theme = theme_from_visuals(ui.visuals());
        let weak_color = ui.visuals().weak_text_color();

        // Scoped so the measuring borrow of `ui` ends before the painting one
        // begins. `describe_structure` pulls each bond's endpoint back to the
        // edge of the atom label it meets, so the insets have to be computed
        // with the very font metrics that will draw those labels — sharing one
        // backend's metrics with another is what makes bonds strike through
        // text.
        let shapes = {
            let measure = |text: &str, size: f32| {
                ui.painter()
                    .layout_no_wrap(
                        text.to_owned(),
                        FontId::proportional(size),
                        Color32::PLACEHOLDER,
                    )
                    .size()
            };
            describe_structure(
                self.molecule,
                rect,
                &self.options,
                &theme,
                weak_color,
                &measure,
            )
        };

        paint_structure(ui.painter(), &shapes);
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_the_palette_follows_the_app_theme_rather_than_a_setting_of_its_own() {
        assert_eq!(
            theme_from_visuals(&egui::Visuals::dark()).foreground,
            StructureTheme::dark().foreground
        );
        assert_eq!(
            theme_from_visuals(&egui::Visuals::light()).foreground,
            StructureTheme::light().foreground
        );
    }

    /// Ethanol as a molfile, with real 2D coordinates in the atom block.
    const ETHANOL: &str = "ethanol\n  chem\n\n  3  2  0  0  0  0  0  0  0  0999 V2000\n\
        0.0000    0.0000    0.0000 C   0  0\n    1.2990    0.7500    0.0000 C   0  0\n\
        2.5981    0.0000    0.0000 O   0  0\n  1  2  1  0\n  2  3  1  0\nM  END\n$$$$\n";

    fn describe(molecule: &Molecule) -> Vec<StructureShape> {
        describe_structure(
            molecule,
            egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(400.0, 400.0)),
            &StructureOptions::default(),
            &StructureTheme::light(),
            Color32::GRAY,
            // A stand-in for egui's own metrics, which need a font atlas.
            &|text: &str, size: f32| Vec2::new(text.len() as f32 * size * 0.5, size),
        )
    }

    #[test]
    fn test_a_molfile_carrying_coordinates_is_drawn_rather_than_placeheld() {
        // The automated half of "does an SDF actually draw?". A molfile stores
        // per-atom coordinates, so this must produce real bond lines — a lone
        // Text shape would mean the layout was lost between the file and here.
        let outcome = chem::io::reader::read(ETHANOL, chem::io::reader::Format::Sdf);
        assert!(outcome.skipped.is_empty(), "{:?}", outcome.skipped);
        let molecule = &outcome.records[0].molecule;
        assert!(molecule.has_coords(), "a molfile carries its own layout");

        let shapes = describe(molecule);
        let lines = shapes
            .iter()
            .filter(|s| matches!(s, StructureShape::Line { .. }))
            .count();
        assert_eq!(lines, 2, "two bonds, two lines: {shapes:?}");
        assert!(
            shapes.iter().any(|s| matches!(
                s,
                StructureShape::Text { text, .. } if text == "O"
            )),
            "the oxygen should be labelled: {shapes:?}"
        );
    }

    #[test]
    fn test_a_molecule_with_no_coordinates_is_described_as_text_rather_than_nothing() {
        // The placeholder comes from `chem`, but that it arrives at all is what
        // stops an unlaid-out molecule looking like a rendering failure.
        let molecule = chem::io::smiles::parse_smiles("CCO").expect("ethanol parses");
        assert!(!molecule.has_coords());
        let shapes = describe(&molecule);
        assert!(
            matches!(shapes.as_slice(), [StructureShape::Text { .. }]),
            "expected one placeholder label, got {shapes:?}"
        );
    }
}
