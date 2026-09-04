// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Viewers for molecular structure files.
//!
//! This is the first crate downstream of the [`silva_viz_core`] seam, and it
//! has no privileged access to the shell: [`register`] makes the same
//! [`silva_viz_core::ViewerRegistry::register`] calls a third-party crate
//! would. If anything here ever needs something the traits cannot express,
//! `docs/viewers.md` is where it should show up first.
//!
//! Everything chemical comes from [`chem`] — molecule types, file reading, and
//! a depiction layer that describes a structure as drawable primitives. What
//! this crate adds is the two things `chem` deliberately does not have: an egui
//! backend that paints those primitives ([`structure`]), and factories that
//! decide whether a given file is a structure file at all ([`sdf`]).

pub mod records;
pub mod sdf;
pub mod structure;

pub use records::RecordsView;
pub use sdf::SdfFactory;
pub use structure::{StructureView, theme_from_visuals};

/// Registers every viewer this crate provides.
///
/// One call site rather than a list the app has to grow with each new format,
/// which is the only reason this function exists rather than the factories
/// being registered individually.
pub fn register(
    registry: &mut silva_viz_core::ViewerRegistry,
) -> &mut silva_viz_core::ViewerRegistry {
    registry.register(Box::new(SdfFactory))
}
