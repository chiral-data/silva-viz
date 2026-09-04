// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// `controls` is ported from `structure_option_controls` in
// `crates/chem-app/src/structure_view.rs` of
// https://github.com/chiral-data/rust-chem, which is MIT licensed
// (Copyright (c) 2021 Chiral Ltd.). The MIT licence permits this; its notice
// is retained here because it requires that.

//! How structures are drawn, shared by every window and remembered.
//!
//! # Why this is not a field on the view
//!
//! These options govern every open structure, so they have to be one value
//! rather than a copy per window. But [`silva_viz_core::View`] is deliberately
//! two methods with no channel for application state, and that trait is
//! documented for third-party viewer crates — widening it for one feature
//! would break the thing the crate exists to provide.
//!
//! It is not necessary. A view holds a [`egui::Ui`], and therefore a
//! [`egui::Context`], and egui has a persisted store of its own. The shell
//! already relies on it: window geometry has been kept there since v0.1.0, so
//! this is the existing mechanism rather than a new one.
//!
//! # What that store does and does not promise
//!
//! Values reach disk only because `eframe::App::persist_egui_memory` defaults
//! to `true`, and they ride inside egui's single `Memory` blob — where a
//! decode failure anywhere discards the whole thing, and where the key
//! includes a `TypeId` that is not guaranteed stable across toolchain or
//! dependency changes. So the promise is "remembered between sessions", not
//! "permanent". The same failure resets window geometry, which this app has
//! lived with since v0.1.0, so the exposure is not new.

use chem::draw::{AtomVisualization, ShowCarbons, StructureOptions};

/// Where the shared value lives.
///
/// Namespaced rather than [`egui::Id::NULL`], which egui documents as the
/// idiom for a singleton: `NULL` is a slot any crate in the tree could also
/// claim for a `StructureOptions`, and a collision would be silent.
fn store_id() -> egui::Id {
    egui::Id::new("silva-viz-chem/structure-options")
}

/// What the options are before anyone touches them.
///
/// `chem`'s own default draws hydrogens. An SDF carries them as real atoms and
/// drawing them all buries the skeleton — a benzene ring becomes twelve
/// vertices instead of six — so this starts them off. SMILES leaves hydrogens
/// implicit, so the flag has nothing to hide there and one shared value serves
/// both formats. That is what replaced the per-format branch this module
/// removed.
fn seed() -> StructureOptions {
    StructureOptions {
        explicit_hydrogens: false,
        ..StructureOptions::default()
    }
}

/// The options every structure is currently drawn with.
///
/// `data_mut` and a *persisted* accessor, never `data` or a temp one, and the
/// distinction is not cosmetic. `get_persisted` needs `&mut` because it
/// deserialises on first read and caches; `get_temp` returns `None` for a
/// value that came off disk and has not been promoted yet; and
/// `get_temp_mut_or_insert_with` would *overwrite* the stored value with a
/// temporary one, turning persistence off from then on with no diagnostic.
///
/// Seeding with this module's own starting value rather than
/// `get_persisted_mut_or_default` matters for
/// the same reason: `StructureOptions::default()` draws hydrogens, so a decode
/// failure would silently reverse the choice above.
pub fn shared(ctx: &egui::Context) -> StructureOptions {
    ctx.data_mut(|data| *data.get_persisted_mut_or_insert_with(store_id(), seed))
}

pub fn set_shared(ctx: &egui::Context, options: StructureOptions) {
    ctx.data_mut(|data| data.insert_persisted(store_id(), options));
}

/// Controls for the three options worth exposing.
///
/// Carbons, atom display and hydrogens. `StructureOptions` carries seven more —
/// `bond_spacing_ratio`, `short_bond_length`, `label_margin_ratio`,
/// `font_size_ratio`, `font_size_range` and friends — whose documentation
/// describes collision avoidance and calibration against a reference renderer.
/// A control for those offers one correct value and a range of ways to make the
/// depiction unreadable; `font_size_range` is explicitly a legibility clamp, so
/// exposing it would be exposing the guardrail.
///
/// The value is written back unconditionally, because `StructureOptions` is not
/// `PartialEq` and so "only if it changed" cannot be expressed as a comparison.
/// It is a `Copy` struct of scalars; the write is cheaper than tracking change.
pub fn controls(ui: &mut egui::Ui) {
    let mut options = shared(ui.ctx());

    ui.horizontal_wrapped(|ui| {
        ui.label("Carbons:");
        egui::ComboBox::from_id_salt("show_carbons")
            .selected_text(options.show_carbons.label())
            .show_ui(ui, |ui| {
                for mode in ShowCarbons::ALL {
                    ui.selectable_value(&mut options.show_carbons, mode, mode.label());
                }
            });

        ui.separator();
        ui.label("Atoms:");
        egui::ComboBox::from_id_salt("atom_visualization")
            .selected_text(options.atom_visualization.label())
            .show_ui(ui, |ui| {
                for mode in AtomVisualization::ALL {
                    ui.selectable_value(&mut options.atom_visualization, mode, mode.label());
                }
            });

        ui.separator();
        ui.checkbox(&mut options.explicit_hydrogens, "Hydrogens");
    });

    set_shared(ui.ctx(), options);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_the_seed_keeps_hydrogens_off_so_an_sdf_still_reads_cleanly() {
        // The behaviour `options_for(format)` used to provide. If this ever
        // flips, every SDF gains a hairball of hydrogens.
        assert!(!seed().explicit_hydrogens);
        assert!(
            StructureOptions::default().explicit_hydrogens,
            "and it is a real override, not a restatement of chem's default"
        );
    }

    #[test]
    fn test_a_fresh_context_returns_the_seed_and_remembers_what_is_set() {
        let ctx = egui::Context::default();
        assert!(!shared(&ctx).explicit_hydrogens);

        let mut changed = shared(&ctx);
        changed.explicit_hydrogens = true;
        changed.show_carbons = ShowCarbons::All;
        set_shared(&ctx, changed);

        let read_back = shared(&ctx);
        assert!(read_back.explicit_hydrogens);
        assert_eq!(read_back.show_carbons, ShowCarbons::All);
    }

    #[test]
    fn test_the_value_is_shared_within_a_context_and_not_across_two() {
        // App-wide, which is the point: every window in one app sees one value.
        // Not process-global, which would be a different and worse thing.
        let one = egui::Context::default();
        let other = egui::Context::default();

        let mut options = shared(&one);
        options.show_carbons = ShowCarbons::All;
        set_shared(&one, options);

        assert_eq!(shared(&one).show_carbons, ShowCarbons::All);
        assert_eq!(
            shared(&other).show_carbons,
            StructureOptions::default().show_carbons,
            "a second app must not inherit the first's setting"
        );
    }

    #[test]
    fn test_the_controls_render_and_leave_the_value_intact() {
        let ctx = egui::Context::default();
        let mut options = shared(&ctx);
        options.show_carbons = ShowCarbons::Terminal;
        set_shared(&ctx, options);

        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, controls);
        });
        assert!(!output.shapes.is_empty(), "the controls painted nothing");
        // Drawn without anyone touching them, so the value must be unchanged —
        // the widgets write back every frame, so a bug here would reset it.
        assert_eq!(shared(&ctx).show_carbons, ShowCarbons::Terminal);
    }
}
