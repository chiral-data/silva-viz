// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One open window's contents.

/// The contents of a floating viewer window.
///
/// A view is handed a [`crate::Blob`] when it opens and owns whatever it read
/// from it. There is deliberately no route back to the [`crate::FileSource`]
/// for writing: this is a workbench for looking at files.
pub trait View {
    /// Shown in the window's title bar. Owned rather than borrowed because a
    /// view usually builds it from the file name plus something it learned
    /// while loading ("3 columns", "PNG 800x600").
    fn title(&self) -> String;

    fn ui(&mut self, ui: &mut egui::Ui);
}
