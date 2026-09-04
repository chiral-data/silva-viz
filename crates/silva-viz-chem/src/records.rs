// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A structure file's records, one on screen at a time.
//!
//! Written for every format [`chem::io::reader`] reads rather than for SDF
//! alone: the formats differ in how a file is probed and in whether
//! coordinates arrive with it, and in nothing this view can see. So the
//! `.smi` viewer adds a factory rather than a second copy of this.

use crate::structure::StructureView;
use chem::draw::StructureOptions;
use chem::io::reader::{self, Format, Record, Skipped};
use silva_viz_core::{Blob, View};

/// Above this a structure viewer declines and the hex viewer takes the file.
///
/// The number is about the *read*, not the display: [`reader::read`] takes the
/// whole file as a `&str` and parses every record before anything is shown, so
/// this is the size of a freeze the user cannot cancel. Paging a structure file
/// needs `Blob::read_range` and is a story of its own.
///
/// One constant for every format, because the reason is the same for all of
/// them. It was called `SDF_LIMIT` while SDF was the only format, which stopped
/// being true the moment `.smi` arrived.
pub const SIZE_LIMIT: u64 = 32 * 1024 * 1024;

/// Records parsed at open. Beyond this the file is truncated and the view says
/// so, because the cost is per record rather than per byte.
const MAX_RECORDS: usize = 20_000;

/// One record, with the parts that are expensive or awkward to recompute each frame.
struct Shown {
    record: Record,
    /// Where the file this came from would say it is, in whatever units
    /// [`reader`] counts for that format: a record ordinal for SDF, a physical
    /// line number for SMILES. Never inferred — see [`attach_positions`].
    position: usize,
    /// Sorted once here because [`chem::core::molecule::Molecule::properties`]
    /// hands back a `HashMap`, whose iteration order changes between runs — an
    /// SDF's data fields would otherwise shuffle every time the file reopened.
    properties: Vec<(String, String)>,
}

pub struct RecordsView {
    file: String,
    label: &'static str,
    records: Vec<Shown>,
    skipped: Vec<Skipped>,
    selected: usize,
    /// The records the file actually held, when [`MAX_RECORDS`] cut it short.
    total: Option<usize>,
    /// A failure to read the bytes at all, which is not the same as a file
    /// whose records failed to parse.
    error: Option<String>,
    /// How the structures are drawn. Fixed per format for now; story D makes
    /// it adjustable and persists it.
    options: StructureOptions,
    /// Kept only to label positions, which are counted differently per format.
    format: Format,
}

impl RecordsView {
    pub fn new(blob: Blob, format: Format, label: &'static str) -> Self {
        let mut view = Self {
            file: blob.name().to_string(),
            label,
            records: Vec::new(),
            skipped: Vec::new(),
            selected: 0,
            total: None,
            error: None,
            options: options_for(format),
            format,
        };

        let bytes = match blob.read_all() {
            Ok(bytes) => bytes,
            Err(e) => {
                view.error = Some(e.to_string());
                return view;
            }
        };
        // Lossy rather than a hard failure: the head was valid UTF-8 or the
        // factory would not have bid, so a bad byte further in is a local
        // defect in an otherwise readable file.
        let content = String::from_utf8_lossy(&bytes);

        let (content, held) = truncate(&content, format, MAX_RECORDS);
        if held > MAX_RECORDS {
            view.total = Some(held);
        }

        let outcome = reader::read(&content, format);
        view.skipped = outcome.skipped;
        view.records = attach_positions(
            outcome.records,
            &view.skipped,
            &record_positions(&content, format),
        );

        // A record that parsed into no atoms at all is a failure wearing a
        // success's clothes — chem 0.6 does this to a V3000 molfile, reporting
        // `Ok` for an atomless molecule that then draws as an empty panel. Move
        // it where the user can see it.
        let (drawable, empty): (Vec<_>, Vec<_>) = std::mem::take(&mut view.records)
            .into_iter()
            .partition(|shown| shown.record.molecule.num_atoms() > 0);
        view.records = drawable;
        view.skipped.extend(empty.into_iter().map(|shown| Skipped {
            position: shown.position,
            input: String::new(),
            error: "parsed as a molecule with no atoms (a V3000 molfile?)".to_string(),
        }));
        view.skipped.sort_by_key(|s| s.position);

        view
    }

    fn current(&self) -> Option<&Shown> {
        self.records.get(self.selected)
    }

    /// Gives the selected molecule coordinates if it has none.
    ///
    /// SMILES carries no layout, so one has to be generated before anything can
    /// be drawn. There is deliberately no "already laid out" flag to go with
    /// this: [`chem::core::layout::ensure_coords`] returns early when the
    /// molecule already has coordinates, so the molecule *is* the cache and
    /// this is a no-op for SDF and for any record drawn before.
    ///
    /// The return value is ignored on purpose. It means "has coordinates now",
    /// not "generated them", so it cannot tell a file's own layout from a
    /// computed one and there is nothing here worth branching on.
    fn lay_out_selected(&mut self) {
        if let Some(shown) = self.records.get_mut(self.selected) {
            chem::core::layout::ensure_coords(&mut shown.record.molecule);
        }
    }

    fn stepper(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let count = self.records.len();
            let enabled = count > 1;
            if ui
                .add_enabled(enabled && self.selected > 0, egui::Button::new("◀"))
                .clicked()
            {
                self.selected -= 1;
            }
            ui.label(format!("record {} of {count}", self.selected + 1));
            if ui
                .add_enabled(enabled && self.selected + 1 < count, egui::Button::new("▶"))
                .clicked()
            {
                self.selected += 1;
            }
            if let Some(total) = self.total {
                ui.separator();
                ui.weak(format!("first {count} of {total} in the file"));
            }
        });
    }

    fn details(&self, ui: &mut egui::Ui, shown: &Shown) {
        let molecule = &shown.record.molecule;
        egui::Grid::new("record-details")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                let mut row = |label: &str, value: String| {
                    ui.strong(label);
                    ui.label(value);
                    ui.end_row();
                };
                // `Record::name` rather than `Molecule::name`: the reader
                // substitutes `Molecule_N` for a record whose name line is
                // blank, and the molecule itself keeps the `None`.
                row("name", shown.record.name.clone());
                row("formula", molecule.formula());
                row("weight", format!("{:.2}", molecule.molecular_weight()));
                row(
                    "atoms / bonds",
                    format!("{} / {}", molecule.num_atoms(), molecule.num_bonds()),
                );
                if let Some(smiles) = &shown.record.smiles {
                    row("smiles", smiles.clone());
                }
                for (key, value) in &shown.properties {
                    row(key, value.clone());
                }
            });
    }

    fn failures(&self, ui: &mut egui::Ui) {
        if self.skipped.is_empty() {
            return;
        }
        let total = self.records.len() + self.skipped.len();
        let summary = format!(
            "{} of {total} records could not be read",
            self.skipped.len()
        );
        let noun = position_noun(self.format);
        // Three columns only when something fills the middle one: SMILES
        // carries the offending token in `input`, SDF leaves it empty because
        // a record is a multi-line block and quoting it back is noise.
        let has_input = self.skipped.iter().any(|s| !s.input.is_empty());
        egui::CollapsingHeader::new(egui::RichText::new(summary).color(ui.visuals().warn_fg_color))
            .default_open(self.records.is_empty())
            .show(ui, |ui| {
                egui::Grid::new("record-failures")
                    .num_columns(if has_input { 3 } else { 2 })
                    .striped(true)
                    .show(ui, |ui| {
                        for skip in &self.skipped {
                            ui.strong(format!("{noun} {}", skip.position));
                            if has_input {
                                ui.monospace(&skip.input);
                            }
                            ui.label(&skip.error);
                            ui.end_row();
                        }
                    });
            });
    }
}

impl View for RecordsView {
    fn title(&self) -> String {
        match (self.records.len(), &self.error) {
            (_, Some(_)) => format!("{} — {}", self.file, self.label),
            (0, None) => format!("{} — {} (no records)", self.file, self.label),
            (1, None) => format!("{} — {}", self.file, self.label),
            (n, None) => format!(
                "{} — {} ({} of {n})",
                self.file,
                self.label,
                self.selected + 1
            ),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
            return;
        }

        self.stepper(ui);
        ui.separator();

        // After the stepper, so a record stepped onto this frame is laid out
        // before it is drawn rather than one frame late.
        self.lay_out_selected();

        // The structure takes most of the window and the details take the rest,
        // rather than the structure shrinking to whatever the details leave —
        // a depiction two centimetres tall is not worth showing.
        let structure_height = (ui.available_height() * 0.62).max(160.0);
        let width = ui.available_width();
        if let Some(shown) = self.current() {
            ui.add(
                StructureView::new(
                    &shown.record.molecule,
                    egui::Vec2::new(width, structure_height),
                )
                .with_options(self.options),
            );
        } else if self.skipped.is_empty() {
            ui.weak("this file held no records");
        }

        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Cloned index rather than borrowing self across the closure:
                // `details` needs `&self` while the scroll area holds `ui`.
                if let Some(shown) = self.records.get(self.selected) {
                    self.details(ui, shown);
                    ui.add_space(8.0);
                }
                self.failures(ui);
            });
    }
}

/// The depiction options a format opens with.
///
/// An SDF carries hydrogens as real atoms in the graph, and drawing every one
/// of them buries the skeleton the structure exists to show — a benzene ring
/// becomes twelve vertices instead of six. `chem`'s own field documentation
/// recommends against it for exactly this format. SMILES leaves hydrogens
/// implicit, so the flag has nothing to hide there and `chem`'s default
/// stands.
///
/// Hidden atoms take their bonds with them, so this removes the H labels and
/// their bonds together rather than leaving strokes pointing at nothing.
fn options_for(format: Format) -> StructureOptions {
    let mut options = StructureOptions::default();
    if matches!(format, Format::Sdf) {
        options.explicit_hydrogens = false;
    }
    options
}

/// The file text cut to at most `max` records, and how many it actually held.
///
/// Truncating the *text* rather than the parsed records is what keeps every
/// format rule inside `chem`: this needs to know where one record ends, and
/// nothing whatever about what is inside it.
fn truncate(content: &str, format: Format, max: usize) -> (String, usize) {
    let ends_record: fn(&str) -> bool = match format {
        // One molecule per `$$$$`-terminated block.
        Format::Sdf => |line: &str| line.trim() == "$$$$",
        // One molecule per line, blanks and `#` comments ignored — the same
        // lines `reader::read_smiles` chooses to count.
        Format::Smiles => |line: &str| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with('#')
        },
    };

    let mut held = 0;
    let mut cut = None;
    for (index, line) in content.lines().enumerate() {
        if ends_record(line) {
            held += 1;
            if held == max {
                cut = Some(index);
            }
        }
    }
    // A trailing block with no terminator — which a single-molecule SDF is.
    if matches!(format, Format::Sdf)
        && content
            .lines()
            .rev()
            .take_while(|l| l.trim() != "$$$$")
            .any(|l| !l.trim().is_empty())
    {
        held += 1;
    }

    match cut {
        Some(last) if held > max => (
            content
                .lines()
                .take(last + 1)
                .collect::<Vec<_>>()
                .join("\n"),
            held,
        ),
        _ => (content.to_string(), held),
    }
}

/// The positions [`reader`] will report for this file's records, in order.
///
/// Not the same counting system in both formats, which is the whole reason
/// this exists. `read_sdf` increments only on `$$$$`, so an SDF position is a
/// record ordinal. `read_smiles` takes its position from
/// `content.lines().enumerate()` *before* deciding whether to skip a blank or
/// a `#` comment, so a SMILES position is a physical line number and the
/// skipped lines consume numbers without producing anything. `Skipped`'s own
/// documentation says so: "Line number for SMILES, record number for SDF".
fn record_positions(content: &str, format: Format) -> Vec<usize> {
    match format {
        // One per `$$$$`, plus a trailing block with no terminator — which a
        // single-molecule file usually is.
        Format::Sdf => {
            let mut n = content.lines().filter(|l| l.trim() == "$$$$").count();
            if content
                .lines()
                .rev()
                .take_while(|l| l.trim() != "$$$$")
                .any(|l| !l.trim().is_empty())
            {
                n += 1;
            }
            (1..=n).collect()
        }
        // The line numbers of the lines the reader will actually look at.
        Format::Smiles => content
            .lines()
            .enumerate()
            .filter(|(_, line)| {
                let line = line.trim();
                !line.is_empty() && !line.starts_with('#')
            })
            .map(|(index, _)| index + 1)
            .collect(),
    }
}

/// Pairs each parsed record with the position its file would name it by.
///
/// `chem` reports a position for every record it *skipped* and none for the
/// ones it kept, so the kept positions have to come from somewhere. An earlier
/// version inferred them as the positions in `1..=total` that the skip list did
/// not mention. That is right for SDF and wrong for SMILES, where positions are
/// line numbers and blanks and comments consume them — a file of a comment, a
/// blank and two molecules has its records at lines 3 and 4, which no
/// arithmetic over `1..=4` recovers.
///
/// So the positions are walked rather than deduced: `positions` is what the
/// reader will count, in order, and whatever it does not report as failed is
/// the next kept record.
fn attach_positions(records: Vec<Record>, skipped: &[Skipped], positions: &[usize]) -> Vec<Shown> {
    let failed: std::collections::HashSet<usize> = skipped.iter().map(|s| s.position).collect();
    let mut kept = positions.iter().copied().filter(|p| !failed.contains(p));
    records
        .into_iter()
        .map(|record| {
            let mut properties: Vec<(String, String)> = record
                .molecule
                .properties()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            properties.sort();
            Shown {
                position: kept.next().unwrap_or(0),
                record,
                properties,
            }
        })
        .collect()
}

/// What one of this format's positions is called, for the failure list.
///
/// A SMILES failure at "record 3" sends the reader to the wrong place in their
/// file; the number is a line number, so the word has to be `line`.
fn position_noun(format: Format) -> &'static str {
    match format {
        Format::Sdf => "record",
        Format::Smiles => "line",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` single-atom molfile records, each `$$$$`-terminated.
    fn sdf_records(n: usize) -> String {
        (1..=n)
            .map(|i| {
                format!(
                    "mol{i}\n  chem\n\n  1  0  0  0  0  0  0  0  0  0999 V2000\n    0.0000    0.0000    0.0000 C   0  0\nM  END\n$$$$\n"
                )
            })
            .collect()
    }

    /// Two good records with a data field each, and a broken one between them
    /// whose counts line promises ten atoms and supplies none.
    fn mixed_file() -> String {
        let good = |name: &str, activity: &str| {
            format!(
                "{name}\n  chem\n\n  3  2  0  0  0  0  0  0  0  0999 V2000\n    0.0000    0.0000    0.0000 C   0  0\n    1.2990    0.7500    0.0000 C   0  0\n    2.5981    0.0000    0.0000 O   0  0\n  1  2  1  0\n  2  3  1  0\nM  END\n> <ACTIVITY>\n{activity}\n\n$$$$\n"
            )
        };
        let broken = "broken\n  chem\n\n 10  9  0  0  0  0  0  0  0  0999 V2000\nM  END\n$$$$\n";
        format!("{}{broken}{}", good("first", "1.5"), good("second", "2.5"))
    }

    fn view_of(name: &str, content: &str) -> RecordsView {
        let mut mem = silva_viz_core::MemSource::new();
        let id = mem.add(name, content.as_bytes().to_vec());
        let source: silva_viz_core::SharedSource = std::rc::Rc::new(std::cell::RefCell::new(mem));
        let blob = Blob::open(source, id).expect("opening the blob");
        RecordsView::new(blob, Format::Sdf, "SDF")
    }

    /// Runs one frame of a view the way the shell would, and returns how many
    /// shapes it painted — so "it did not panic" is joined by "it drew".
    fn render(view: &mut RecordsView) -> usize {
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| view.ui(ui));
        });
        output.shapes.len()
    }

    #[test]
    fn test_the_whole_view_renders_a_frame_without_panicking() {
        // The stepper, the structure, the details grid and the failure list all
        // run here. Nothing else in this crate paints, so a panic in any of
        // them would otherwise first appear in front of a user.
        let mut view = view_of("mixed.sdf", &mixed_file());
        assert_eq!(view.records.len(), 2);
        assert_eq!(view.skipped.len(), 1);
        assert!(render(&mut view) > 0, "the first frame painted nothing");
        // A second frame, in case the first cached something it should not.
        assert!(render(&mut view) > 0, "the second frame painted nothing");
    }

    #[test]
    fn test_a_file_whose_every_record_failed_still_renders_and_says_so() {
        // `ReadOutcome::is_empty` only consults `records`, so a file like this
        // reads as "empty" upstream. It must not render as a blank window.
        let all_bad = "broken\n  chem\n\n 10  9  0  0  0  0  0  0  0  0999 V2000\nM  END\n$$$$\n";
        let mut view = view_of("bad.sdf", all_bad);
        assert!(view.records.is_empty());
        assert_eq!(view.skipped.len(), 1);
        assert!(view.title().contains("no records"), "{}", view.title());
        assert!(render(&mut view) > 0, "the failure list must still paint");
    }

    #[test]
    fn test_the_title_counts_the_selected_record_of_a_multi_record_file() {
        let mut view = view_of("mixed.sdf", &mixed_file());
        assert!(
            view.title().starts_with("mixed.sdf — SDF (1 of 2)"),
            "{}",
            view.title()
        );
        view.selected = 1;
        assert!(
            view.title().starts_with("mixed.sdf — SDF (2 of 2)"),
            "{}",
            view.title()
        );
    }

    #[test]
    fn test_a_v3000_record_after_a_good_one_is_reported_rather_than_drawn_blank() {
        // The probe declines a file whose *first* record is V3000, but a later
        // one slips through — and chem reports it as a successfully read
        // molecule with no atoms, which would paint an empty panel.
        let good = "first\n  chem\n\n  1  0  0  0  0  0  0  0  0  0999 V2000\n    0.0000    0.0000    0.0000 C   0  0\nM  END\n$$$$\n";
        let v3000 = "second\n  chem\n\n  0  0  0  0  0  0  0  0  0  0999 V3000\nM  V30 BEGIN CTAB\nM  V30 COUNTS 21 22\nM  END\n$$$$\n";
        let mut view = view_of("late.sdf", &format!("{good}{v3000}"));

        assert_eq!(view.records.len(), 1, "the atomless record is not drawable");
        assert_eq!(view.skipped.len(), 1);
        assert_eq!(view.skipped[0].position, 2);
        assert!(
            view.skipped[0].error.contains("no atoms"),
            "{:?}",
            view.skipped[0]
        );
        render(&mut view);
    }

    /// Methane with all four hydrogens as real atoms in the graph, which is how
    /// an SDF usually carries them: 5 atoms, 4 bonds.
    const METHANE_WITH_H: &str = "methane\n  chem\n\n  5  4  0  0  0  0  0  0  0  0999 V2000\n\
        0.0000    0.0000    0.0000 C   0  0\n    1.0000    0.0000    0.0000 H   0  0\n\
       -1.0000    0.0000    0.0000 H   0  0\n    0.0000    1.0000    0.0000 H   0  0\n\
        0.0000   -1.0000    0.0000 H   0  0\n  1  2  1  0\n  1  3  1  0\n  1  4  1  0\n\
      1  5  1  0\nM  END\n$$$$\n";

    #[test]
    fn test_an_sdf_hides_the_hydrogens_it_carries_as_atoms() {
        // Not a test that a flag holds a value — a test that the flag has its
        // effect. An SDF stores hydrogens explicitly, and drawing all of them
        // buries the skeleton, so the SDF viewer opens with them hidden.
        let view = view_of("methane.sdf", METHANE_WITH_H);
        assert_eq!(view.records.len(), 1);
        assert_eq!(
            view.records[0].record.molecule.num_atoms(),
            5,
            "the molecule still holds every hydrogen"
        );
        assert!(!view.options.explicit_hydrogens);

        let drawn =
            crate::structure::describe_for_test(&view.records[0].record.molecule, &view.options);
        let labels: Vec<&str> = drawn
            .iter()
            .filter_map(|s| match s {
                chem::draw::StructureShape::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            !labels.contains(&"H"),
            "no hydrogen should be labelled: {labels:?}"
        );

        // And with them shown, they come back — so the assertion above is
        // about the option rather than about this molecule.
        let mut shown_options = view.options;
        shown_options.explicit_hydrogens = true;
        let with_h =
            crate::structure::describe_for_test(&view.records[0].record.molecule, &shown_options);
        assert!(
            with_h.len() > drawn.len(),
            "showing hydrogens must draw more: {} vs {}",
            with_h.len(),
            drawn.len()
        );
    }

    fn smiles_view(name: &str, content: &str) -> RecordsView {
        let mut mem = silva_viz_core::MemSource::new();
        let id = mem.add(name, content.as_bytes().to_vec());
        let source: silva_viz_core::SharedSource = std::rc::Rc::new(std::cell::RefCell::new(mem));
        let blob = Blob::open(source, id).expect("opening the blob");
        RecordsView::new(blob, Format::Smiles, "SMILES")
    }

    #[test]
    fn test_a_smiles_record_gets_its_coordinates_from_the_first_draw() {
        // SMILES carries no layout. Nothing generates one at open, so the
        // molecule must arrive bare and be laid out by the time it is painted.
        let mut view = smiles_view("lib.smi", "CC(=O)Oc1ccccc1C(=O)O aspirin\n");
        assert_eq!(view.records.len(), 1);
        assert!(
            !view.records[0].record.molecule.has_coords(),
            "nothing should lay out a molecule at open"
        );

        assert!(render(&mut view) > 0, "the frame painted nothing");
        assert!(
            view.records[0].record.molecule.has_coords(),
            "one frame should have laid it out"
        );
    }

    #[test]
    fn test_stepping_lays_out_the_record_stepped_onto() {
        let mut view = smiles_view("lib.smi", "CCO one\nc1ccccc1 two\nCCCC three\n");
        assert_eq!(view.records.len(), 3);
        render(&mut view);
        assert!(view.records[0].record.molecule.has_coords());
        assert!(
            !view.records[2].record.molecule.has_coords(),
            "a record never shown should not have been laid out"
        );

        view.selected = 2;
        render(&mut view);
        assert!(view.records[2].record.molecule.has_coords());
    }

    #[test]
    fn test_a_smiles_file_reports_a_bad_line_by_its_line_number() {
        // End to end through the view, not just the helpers: a comment, a
        // blank, two molecules and a broken one in between.
        let mut view = smiles_view(
            "lib.smi",
            "# a header\n\nCCO ethanol\nC1CC broken\nc1ccccc1 benzene\n",
        );
        assert_eq!(view.records.len(), 2);
        assert_eq!(view.skipped.len(), 1);
        assert_eq!(view.skipped[0].position, 4);
        // The offending token is kept for SMILES, unlike SDF.
        assert_eq!(view.skipped[0].input, "C1CC");
        assert_eq!(
            view.records.iter().map(|s| s.position).collect::<Vec<_>>(),
            [3, 5]
        );
        assert!(render(&mut view) > 0);
    }

    #[test]
    fn test_a_vendor_header_shows_up_as_a_failure_on_line_one() {
        let mut view = smiles_view(
            "vendor.smi",
            "smiles name activity\nCCO ethanol 1.5\nc1ccccc1 benzene 2.5\n",
        );
        assert_eq!(view.records.len(), 2, "the data must still be read");
        assert_eq!(view.skipped.len(), 1);
        assert_eq!(view.skipped[0].position, 1);
        assert_eq!(view.skipped[0].input, "smiles");
        assert!(render(&mut view) > 0);
    }

    #[test]
    fn test_smiles_positions_are_line_numbers_and_survive_blanks_and_comments() {
        // The defect this story exists to fix. `read_smiles` takes its position
        // from `lines().enumerate()` before deciding to skip a blank or a
        // comment, so those lines consume numbers. Inferring a kept record's
        // position from `1..=total` put every SMILES failure at the wrong line.
        let content = "# a header comment\n\nCCO ethanol\nC1CC broken\nc1ccccc1 benzene\n";
        let positions = record_positions(content, Format::Smiles);
        assert_eq!(
            positions,
            [3, 4, 5],
            "lines 1 and 2 are a comment and a blank"
        );

        let outcome = reader::read(content, Format::Smiles);
        assert_eq!(outcome.records.len(), 2);
        assert_eq!(outcome.skipped.len(), 1);
        assert_eq!(outcome.skipped[0].position, 4, "the bad line is line 4");

        let shown = attach_positions(outcome.records, &outcome.skipped, &positions);
        assert_eq!(
            shown.iter().map(|s| s.position).collect::<Vec<_>>(),
            [3, 5],
            "the kept molecules are on lines 3 and 5"
        );
    }

    #[test]
    fn test_a_kept_smiles_position_agrees_with_the_name_chem_gave_it() {
        // `read_smiles` names an unnamed record `Molecule_{line}`, so the two
        // can be checked against each other rather than both being trusted.
        let content = "\n\n# comment\nCCO\nCCC\n";
        let outcome = reader::read(content, Format::Smiles);
        let shown = attach_positions(
            outcome.records,
            &outcome.skipped,
            &record_positions(content, Format::Smiles),
        );
        for s in &shown {
            assert_eq!(
                s.record.name,
                format!("Molecule_{}", s.position),
                "position {} disagrees with the name chem assigned",
                s.position
            );
        }
        assert_eq!(shown.len(), 2);
    }

    #[test]
    fn test_sdf_positions_are_still_record_ordinals() {
        // Step 1 must not have changed SDF, where positions count `$$$$`.
        let content = sdf_records(3);
        assert_eq!(record_positions(&content, Format::Sdf), [1, 2, 3]);
        // A trailing block with no terminator counts as one more.
        let trailing = format!(
            "{}mol\n  chem\n\n  1  0  0  0  0  0  0  0  0  0999 V2000\nM  END\n",
            sdf_records(2)
        );
        assert_eq!(record_positions(&trailing, Format::Sdf), [1, 2, 3]);
    }

    #[test]
    fn test_the_failure_noun_matches_how_the_format_counts() {
        assert_eq!(position_noun(Format::Sdf), "record");
        assert_eq!(position_noun(Format::Smiles), "line");
    }

    #[test]
    fn test_a_single_record_without_a_terminator_still_counts_as_one() {
        // The commonest structure file there is: one molecule, no `$$$$`.
        let one = "mol\n  chem\n\n  1  0  0  0  0  0  0  0  0  0999 V2000\nM  END\n";
        let (text, held) = truncate(one, Format::Sdf, MAX_RECORDS);
        assert_eq!(held, 1);
        assert_eq!(text, one);
    }

    #[test]
    fn test_records_beyond_the_cap_are_cut_and_the_file_total_is_reported() {
        let content = sdf_records(5);
        let (text, held) = truncate(&content, Format::Sdf, 2);
        assert_eq!(held, 5, "the whole file is counted even though it is cut");
        assert_eq!(text.matches("$$$$").count(), 2);
        // The cut lands after a terminator, so the kept text is whole records.
        assert!(text.trim_end().ends_with("$$$$"));
    }

    #[test]
    fn test_a_file_under_the_cap_is_passed_through_untouched() {
        let content = sdf_records(3);
        let (text, held) = truncate(&content, Format::Sdf, MAX_RECORDS);
        assert_eq!(held, 3);
        assert_eq!(text, content);
    }

    #[test]
    fn test_smiles_counts_data_lines_and_ignores_blanks_and_comments() {
        // The same lines `reader::read_smiles` counts, so the cap means the
        // same thing in both formats.
        let content = "# a comment\nCCO ethanol\n\n\nc1ccccc1 benzene\n# trailing\n";
        let (_, held) = truncate(content, Format::Smiles, MAX_RECORDS);
        assert_eq!(held, 2);
    }

    #[test]
    fn test_a_kept_record_gets_the_ordinal_the_failure_list_does_not_use() {
        // chem reports positions only for what it skipped, so a kept record's
        // ordinal is inferred. If this is wrong the stepper and the failure
        // list count in different units, which is worse than either alone.
        let records = vec![
            Record {
                molecule: chem::io::smiles::parse_smiles("CCO").unwrap(),
                name: "first".into(),
                smiles: None,
            },
            Record {
                molecule: chem::io::smiles::parse_smiles("CCC").unwrap(),
                name: "second".into(),
                smiles: None,
            },
        ];
        let skipped = [
            Skipped {
                position: 2,
                input: String::new(),
                error: "bad".into(),
            },
            Skipped {
                position: 4,
                input: String::new(),
                error: "bad".into(),
            },
        ];
        let shown = attach_positions(records, &skipped, &[1, 2, 3, 4]);
        assert_eq!(
            shown.iter().map(|s| s.position).collect::<Vec<_>>(),
            [1, 3],
            "records 2 and 4 failed, so the kept ones are 1 and 3"
        );
    }

    #[test]
    fn test_data_fields_are_sorted_so_they_do_not_shuffle_between_openings() {
        // `Molecule::properties` is a HashMap, whose iteration order changes
        // per process. Two openings of one file must not disagree.
        let mut molecule = chem::io::smiles::parse_smiles("CCO").unwrap();
        for key in ["zeta", "alpha", "mu"] {
            molecule.set_property(key.to_string(), format!("{key}-value"));
        }
        let records = vec![Record {
            molecule,
            name: "m".into(),
            smiles: None,
        }];
        let shown = attach_positions(records, &[], &[1]);
        let keys: Vec<&str> = shown[0]
            .properties
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        assert_eq!(keys, ["alpha", "mu", "zeta"]);
    }
}
