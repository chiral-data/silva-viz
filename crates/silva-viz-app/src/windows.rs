// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The floating windows, and where each one opens.
//!
//! Every open file is a window; there is no fixed set of them and no View menu
//! written by hand. That is the difference between this workspace and a tabbed
//! one, and it is what makes "open this file in three viewers at once" fall out
//! rather than need a feature.

use egui::{Rect, vec2};
use silva_viz_core::View;

/// Beyond this, the oldest window is closed to make room.
///
/// A cap rather than a refusal: a user opening the thirteenth file wants to see
/// it, and a dialog saying no would be a worse answer than quietly retiring a
/// window they stopped looking at ten files ago.
pub const MAX_OPEN_VIEWS: usize = 12;

/// How far each new window is offset from the one before, in points.
const CASCADE_STEP: f32 = 28.0;
/// How many windows the cascade runs for before starting again, so a long
/// session does not walk off the bottom-right corner.
const CASCADE_WRAP: u64 = 7;

/// Where the cascade starts, and how big a window opens, as fractions of the
/// workspace — so the layout is the same shape on a laptop and on a monitor.
const ORIGIN: (f32, f32) = (0.06, 0.06);
const SIZE: (f32, f32) = (0.55, 0.62);

pub struct OpenView {
    /// Distinguishes two windows over the same file in the same viewer, and
    /// keys egui's memory for this window's position and size.
    serial: u64,
    pub viewer: String,
    pub path: String,
    view: Box<dyn View>,
    open: bool,
    default_rect: Rect,
}

#[derive(Default)]
pub struct WindowManager {
    views: Vec<OpenView>,
    next_serial: u64,
}

impl WindowManager {
    /// Opens `view` in a new window, evicting the oldest if the cap is reached.
    ///
    /// Returns what was evicted, for the status line — a window vanishing with
    /// no explanation reads as a bug.
    pub fn open(
        &mut self,
        viewer: impl Into<String>,
        path: impl Into<String>,
        view: Box<dyn View>,
        workspace: Rect,
    ) -> Option<String> {
        let evicted = (self.views.len() >= MAX_OPEN_VIEWS)
            .then(|| self.views.remove(0))
            .map(|v| v.view.title());

        let serial = self.next_serial;
        self.next_serial += 1;

        let step = (serial % CASCADE_WRAP) as f32 * CASCADE_STEP;
        let (w, h) = (workspace.width(), workspace.height());
        let origin = workspace.min + vec2(w * ORIGIN.0 + step, h * ORIGIN.1 + step);
        let default_rect = Rect::from_min_size(origin, vec2(w * SIZE.0, h * SIZE.1));

        self.views.push(OpenView {
            serial,
            viewer: viewer.into(),
            path: path.into(),
            view,
            open: true,
            default_rect,
        });
        evicted
    }

    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }

    pub fn len(&self) -> usize {
        self.views.len()
    }

    /// What to persist: enough to reopen each window, and nothing about where
    /// it sits. Geometry is egui's, saved against the window id in its own
    /// memory, and duplicating it here is how the two get to disagree.
    pub fn descriptors(&self) -> Vec<(String, String)> {
        self.views
            .iter()
            .map(|v| (v.viewer.clone(), v.path.clone()))
            .collect()
    }

    /// The View menu: one checkbox per open window.
    pub fn menu(&mut self, ui: &mut egui::Ui) {
        if self.views.is_empty() {
            ui.weak("no open windows");
            return;
        }
        for view in &mut self.views {
            ui.checkbox(&mut view.open, view.view.title());
        }
        ui.separator();
        if ui.button("Close all").clicked() {
            self.views.clear();
            ui.close();
        }
    }

    /// Draws every open window, then forgets the ones that were closed.
    pub fn show(&mut self, ctx: &egui::Context) {
        for entry in &mut self.views {
            let OpenView {
                serial,
                view,
                open,
                default_rect,
                ..
            } = entry;
            egui::Window::new(view.title())
                .id(egui::Id::new(("silva-viz-window", *serial)))
                .open(open)
                .default_rect(*default_rect)
                .resizable(true)
                .collapsible(true)
                // Keeps a window reachable when the viewport is smaller than
                // the cascade assumes — a narrow browser canvas, or a laptop
                // screen rather than the size the native build asks for.
                .constrain(true)
                .show(ctx, |ui| view.ui(ui));
        }
        self.views.retain(|v| v.open);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    struct Stub(&'static str);
    impl View for Stub {
        fn title(&self) -> String {
            self.0.to_string()
        }
        fn ui(&mut self, _ui: &mut egui::Ui) {}
    }

    fn workspace() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0))
    }

    fn manager(n: usize) -> WindowManager {
        let mut windows = WindowManager::default();
        for i in 0..n {
            windows.open("text", format!("/f{i}"), Box::new(Stub("f")), workspace());
        }
        windows
    }

    #[test]
    fn test_windows_cascade_rather_than_stacking_exactly() {
        let windows = manager(3);
        let corners: Vec<_> = windows.views.iter().map(|v| v.default_rect.min).collect();
        assert!(corners[1].x > corners[0].x && corners[1].y > corners[0].y);
        assert!(corners[2].x > corners[1].x);
    }

    #[test]
    fn test_the_cascade_wraps_so_a_long_session_stays_on_screen() {
        let windows = manager(MAX_OPEN_VIEWS);
        let first = windows.views[0].default_rect.min;
        let wrapped = windows
            .views
            .iter()
            .find(|v| v.serial == CASCADE_WRAP)
            .expect("the wrapping window should still be open");
        assert_eq!(wrapped.default_rect.min, first);
    }

    #[test]
    fn test_the_cap_evicts_the_oldest_and_says_which() {
        let mut windows = manager(MAX_OPEN_VIEWS);
        let evicted = windows.open("hex", "/extra", Box::new(Stub("extra")), workspace());

        assert_eq!(evicted.as_deref(), Some("f"));
        assert_eq!(windows.len(), MAX_OPEN_VIEWS);
        assert_eq!(windows.descriptors()[0].1, "/f1");
    }

    #[test]
    fn test_the_same_file_can_be_open_in_two_viewers_at_once() {
        let mut windows = WindowManager::default();
        windows.open("text", "/a", Box::new(Stub("a as text")), workspace());
        windows.open("hex", "/a", Box::new(Stub("a as hex")), workspace());

        assert_eq!(windows.len(), 2);
        // Distinct ids, or egui would give them one shared position.
        assert_ne!(windows.views[0].serial, windows.views[1].serial);
    }

    #[test]
    fn test_descriptors_carry_the_viewer_as_well_as_the_path() {
        let mut windows = WindowManager::default();
        windows.open("hex", "/a", Box::new(Stub("a")), workspace());
        // Restoring the path alone would reopen the file in whichever viewer
        // happened to bid highest, which is not the window the user left.
        assert_eq!(
            windows.descriptors(),
            [("hex".to_string(), "/a".to_string())]
        );
    }
}
