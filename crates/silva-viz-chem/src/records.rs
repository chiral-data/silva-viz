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
    /// Shown in the list's own column. Precomputed for the same reason as
    /// [`Self::search`]: `Molecule::formula` allocates and costs 0.41 µs, which
    /// is nothing once and 2 ms across five thousand records.
    formula: String,
    /// What the filter matches: the name and the formula, lowercased.
    ///
    /// Built once, so filtering is a substring scan rather than a few thousand
    /// `formula()` calls per keystroke — measured at 2.03 ms for five
    /// thousand records, which is 12% of a frame, against 16 µs for the scan.
    ///
    /// Deliberately **not** the SMILES, though the list shows it. A substring
    /// match over SMILES text is not substructure search: SMILES is not
    /// canonical, so `c1ccccc1` would find some benzene-containing molecules
    /// and quietly miss others written differently. That needs SMARTS, and the
    /// filter's hint text says what it really does in the meantime.
    search: String,
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
    /// What is in the filter box.
    query: String,
    /// The query [`Self::visible`] was last built from.
    ///
    /// `Response::changed()` alone is not a safe trigger: its own
    /// documentation says it can be `true` when the text did not change — type
    /// and erase a character in one frame — and it does *not* fire when the
    /// code sets `query` itself. Comparing against this is what keeps a
    /// twenty-thousand-record rebuild off a frame that only moved a cursor.
    applied: String,
    /// Indices into [`Self::records`], in file order, that the filter admits.
    visible: Vec<usize>,
    /// A row to bring into view on the next frame, then forget.
    ///
    /// Needed because reconciling the selection can move it somewhere the user
    /// cannot see. The table only wants this requested on one frame; the
    /// scroll animates itself from there.
    pending_scroll: Option<usize>,
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
            query: String::new(),
            applied: String::new(),
            visible: Vec::new(),
            pending_scroll: None,
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
        // No query yet, so everything is visible. Set after the zero-atom
        // partition above, or the indices would point at records that moved.
        view.visible = (0..view.records.len()).collect();

        view
    }

    /// Rebuilds [`Self::visible`] from [`Self::query`], and keeps the selection
    /// pointing at something sensible.
    ///
    /// A substring scan over the precomputed [`Shown::search`] strings, so this
    /// stays in the tens of microseconds even at the record cap. Called when
    /// the query actually changed, never per frame.
    fn refilter(&mut self) {
        let needle = self.query.trim().to_lowercase();
        self.visible = if needle.is_empty() {
            (0..self.records.len()).collect()
        } else {
            (0..self.records.len())
                .filter(|&i| self.records[i].search.contains(&needle))
                .collect()
        };
        self.applied = self.query.clone();

        // The selection is an identity and the filter is a lens over it, so a
        // record filtered out of view stays selected and stays drawn. Only when
        // it is gone *and* something else matches does the selection move —
        // and then the list has to scroll, or it moves out of sight.
        if !self.visible.is_empty() && !self.visible.contains(&self.selected) {
            self.selected = self.visible[0];
        }
        self.pending_scroll = self.visible.iter().position(|&i| i == self.selected);
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

    /// The filter box. Returns whether the query changed this frame.
    fn filter_box(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("filter by name or formula")
                    .desired_width(ui.available_width() - 90.0),
            );
            // Guarded against `applied` rather than trusting `changed()`: it
            // can fire when the text did not change, and does not fire when
            // something else sets `query`.
            if response.changed() && self.query != self.applied {
                self.refilter();
            }
            if self.query.trim().is_empty() {
                ui.weak(format!("{}", self.records.len()));
            } else {
                ui.weak(format!("{} of {}", self.visible.len(), self.records.len()));
            }
        });
    }

    /// The record list: one row per visible record, virtualised.
    fn list(&mut self, ui: &mut egui::Ui) {
        use egui_extras::{Column, TableBuilder};

        if self.records.is_empty() {
            ui.weak("no records");
            return;
        }

        let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
        let noun = position_noun(self.format);
        // Row clicks cannot be returned out of `body`, whose closure yields
        // `()`, so they are captured here instead.
        let mut clicked = None;

        let mut table = TableBuilder::new(ui)
            .id_salt("records")
            // Without this the default is `Sense::hover()`, rows never report
            // a click, and the whole list is inert.
            .sense(egui::Sense::click())
            .striped(true)
            .column(Column::auto())
            .column(Column::auto().at_least(70.0).clip(true))
            .column(Column::auto().at_least(60.0).clip(true))
            // Remainder, so a click lands anywhere across the row rather than
            // only where a cell happens to be.
            .column(Column::remainder().clip(true));

        if let Some(row) = self.pending_scroll.take() {
            table = table.scroll_to_row(row, Some(egui::Align::Center));
        }

        table
            .header(row_h, |mut header| {
                for label in [noun, "name", "formula", "smiles"] {
                    header.col(|ui| {
                        ui.strong(label);
                    });
                }
            })
            .body(|body| {
                body.rows(row_h, self.visible.len(), |mut row| {
                    let shown = &self.records[self.visible[row.index()]];
                    // Before the first `col`: it only affects cells added after.
                    row.set_selected(self.visible[row.index()] == self.selected);
                    row.col(|ui| {
                        ui.weak(format!("{}", shown.position));
                    });
                    row.col(|ui| {
                        ui.label(&shown.record.name);
                    });
                    row.col(|ui| {
                        ui.label(&shown.formula);
                    });
                    row.col(|ui| {
                        ui.monospace(shown.record.smiles.as_deref().unwrap_or(""));
                    });
                    // After the last `col`: it panics if no cell exists yet.
                    if row.response().clicked() {
                        clicked = Some(self.visible[row.index()]);
                    }
                });
            });

        if let Some(index) = clicked {
            self.selected = index;
        }
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

        // Panel order is fixed: top, then side, then central last.
        // Ids come from `ui.id()`, which is unique per window because the shell
        // gives each viewer window its own serial-based `Id` — panel state is
        // keyed on the id alone, so two windows over one file would otherwise
        // share a divider position.
        egui::TopBottomPanel::top(ui.id().with("filter"))
            .resizable(false)
            .show_inside(ui, |ui| {
                self.filter_box(ui);
            });

        egui::SidePanel::left(ui.id().with("records"))
            .resizable(true)
            .default_width(300.0)
            .width_range(180.0..=560.0)
            .show_inside(ui, |ui| {
                self.list(ui);
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if self.visible.is_empty() && !self.query.trim().is_empty() {
                ui.weak(format!("nothing matches {:?}", self.query.trim()));
                ui.add_space(8.0);
            }

            // After the panels, so a record clicked this frame is laid out
            // before it is drawn rather than one frame late.
            self.lay_out_selected();

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
                    if let Some(shown) = self.records.get(self.selected) {
                        self.details(ui, shown);
                        ui.add_space(8.0);
                    }
                    self.failures(ui);
                });
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
            let formula = record.molecule.formula();
            let search = format!("{} {}", record.name, formula).to_lowercase();
            Shown {
                position: kept.next().unwrap_or(0),
                record,
                properties,
                formula,
                search,
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

    /// Four named molecules, so name and formula matching can be told apart.
    fn library() -> RecordsView {
        smiles_view(
            "lib.smi",
            "CC(=O)Oc1ccccc1C(=O)O aspirin\nCn1cnc2c1c(=O)n(C)c(=O)n2C caffeine\nCC(C)Cc1ccc(cc1)C(C)C(=O)O ibuprofen\nCC(=O)Nc1ccc(O)cc1 paracetamol\n",
        )
    }

    #[test]
    fn test_a_file_opens_with_every_record_visible() {
        let view = library();
        assert_eq!(view.records.len(), 4);
        assert_eq!(view.visible, [0, 1, 2, 3]);
        assert!(view.query.is_empty());
    }

    #[test]
    fn test_the_filter_narrows_by_name_and_is_case_insensitive() {
        let mut view = library();
        for query in ["caffeine", "CAFFEINE", "  Caffeine  "] {
            view.query = query.to_string();
            view.refilter();
            assert_eq!(view.visible.len(), 1, "{query:?}");
            assert_eq!(view.records[view.visible[0]].record.name, "caffeine");
        }
    }

    #[test]
    fn test_the_filter_matches_formula_as_well_as_name() {
        // The one chemical query a text box can answer correctly.
        let mut view = library();
        view.query = "c8h10n4o2".to_string();
        view.refilter();
        assert_eq!(view.visible.len(), 1);
        assert_eq!(view.records[view.visible[0]].record.name, "caffeine");
        assert_eq!(view.records[view.visible[0]].formula, "C8H10N4O2");
    }

    #[test]
    fn test_the_filter_does_not_match_smiles_though_the_list_shows_it() {
        // Pins the decision rather than leaving it to be re-litigated. A
        // substring match over SMILES is not substructure search — SMILES is
        // not canonical — so the filter deliberately cannot see it.
        let mut view = library();
        for query in ["Cn1cnc", "c1ccccc1", "CC(=O)"] {
            view.query = query.to_string();
            view.refilter();
            assert!(
                view.visible.is_empty(),
                "{query:?} matched {} records; the filter must not read SMILES",
                view.visible.len()
            );
        }
        // But the SMILES is on screen, which is why the hint text has to say so.
        assert_eq!(
            view.records[1].record.smiles.as_deref(),
            Some("Cn1cnc2c1c(=O)n(C)c(=O)n2C")
        );
    }

    #[test]
    fn test_the_search_string_holds_name_and_formula_and_no_smiles() {
        let view = library();
        let search = &view.records[1].search;
        assert!(search.contains("caffeine"), "{search:?}");
        assert!(search.contains("c8h10n4o2"), "{search:?}");
        assert!(!search.contains("cn1cnc"), "{search:?}");
        assert_eq!(search, &search.to_lowercase(), "must be prelowered");
    }

    #[test]
    fn test_filtering_out_the_selection_moves_it_to_the_first_match() {
        let mut view = library();
        view.selected = 3; // paracetamol
        view.query = "caffeine".to_string();
        view.refilter();
        assert_eq!(view.selected, 1, "selection should follow the filter");
        assert_eq!(view.pending_scroll, Some(0), "and the list should scroll");
    }

    #[test]
    fn test_a_selection_still_in_the_filtered_view_is_left_alone() {
        let mut view = library();
        view.selected = 1; // caffeine
        view.query = "caffeine".to_string();
        view.refilter();
        assert_eq!(view.selected, 1, "no reason to move it");
    }

    #[test]
    fn test_a_query_matching_nothing_leaves_the_selection_and_still_renders() {
        // The right pane says so rather than blanking, and the previously
        // selected structure stays drawn.
        let mut view = library();
        view.selected = 2;
        view.query = "zzzz".to_string();
        view.refilter();
        assert!(view.visible.is_empty());
        assert_eq!(view.selected, 2, "a filter is a lens, not a deselection");
        assert!(render(&mut view) > 0, "an empty filter must still paint");
    }

    #[test]
    fn test_clearing_the_filter_restores_every_record() {
        let mut view = library();
        view.query = "caffeine".to_string();
        view.refilter();
        assert_eq!(view.visible.len(), 1);
        view.query = String::new();
        view.refilter();
        assert_eq!(view.visible, [0, 1, 2, 3]);
    }

    #[test]
    fn test_the_browser_renders_a_frame_with_and_without_a_filter() {
        // The list, the filter box, the split panels and the details all run
        // here; nothing else in this crate paints.
        let mut view = library();
        assert!(render(&mut view) > 0);
        view.query = "aspirin".to_string();
        view.refilter();
        assert!(render(&mut view) > 0);
        assert!(render(&mut view) > 0, "a second frame, in case of caching");
    }

    #[test]
    fn test_a_single_record_file_still_lists_and_draws() {
        // The case story A was built around must not look broken now.
        let mut view = smiles_view("one.smi", "CCO ethanol\n");
        assert_eq!(view.visible, [0]);
        assert!(render(&mut view) > 0);
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
