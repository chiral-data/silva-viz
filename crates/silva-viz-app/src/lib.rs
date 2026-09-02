// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A file browser for scientific applications: browse on the left, open each
//! file in its own floating viewer.
//!
//! A results directory holds unlike things — a structure file, a run log, a
//! table, a figure. Rather than knowing any of their formats, this crate hosts
//! viewers that register themselves and bid for a file by looking at its bytes.

mod app;
mod browser;
mod dnd;
mod views;
mod windows;

pub use app::{SilvaVizApp, default_registry};

/// The canvas the web build mounts on, matching `index.html`.
pub const CANVAS_ID: &str = "silva_viz_canvas";
