// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The left panel: a lazily expanded tree over whatever the source offers.
//!
//! It never mentions a path type. Everything it knows about an entry comes back
//! from [`silva_viz_core::FileSource`], which is what lets the identical panel
//! browse a directory on the desktop and a handful of dropped files on the web.

use egui::TextStyle;
use silva_viz_core::{Blob, EntryId, FileSource, SharedSource, ViewerRegistry};
use std::collections::BTreeSet;

/// What the panel wants the app to do. Returned rather than done, because
/// opening a file needs the registry and the workspace rect, and neither
/// belongs in a tree widget.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BrowserAction {
    /// Choose a different root directory (native) or add files (web).
    ChooseSource,
    /// Open `entry`, in `viewer` if the user picked one from "Open in".
    Open {
        entry: EntryId,
        viewer: Option<String>,
    },
}

enum Row {
    Entry {
        depth: usize,
        id: EntryId,
    },
    /// A directory that could not be listed. Shown in place rather than
    /// swallowed: an unreadable folder is something the user needs to see.
    Error {
        depth: usize,
        text: String,
    },
}

#[derive(Default)]
pub struct Browser {
    expanded: BTreeSet<EntryId>,
    filter: String,
    selected: Option<EntryId>,
    rows: Vec<Row>,
    /// Rebuilding the row list walks every expanded directory, so it happens
    /// when something changes rather than on every frame.
    dirty: bool,
}

impl Browser {
    pub fn new() -> Self {
        Self {
            dirty: true,
            ..Default::default()
        }
    }

    /// Forces a rebuild — after a drop, or a new root.
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    fn keep(&self, name: &str, is_dir: bool) -> bool {
        // Directories are never filtered out: hiding the folder would hide the
        // matching files inside it.
        is_dir
            || self.filter.is_empty()
            || name.to_lowercase().contains(&self.filter.to_lowercase())
    }

    fn flatten(&self, src: &mut dyn FileSource, dir: EntryId, depth: usize, out: &mut Vec<Row>) {
        // Collected before recursing so the borrow of `src` ends here; the
        // recursive call needs it back.
        let picked: Vec<(EntryId, bool)> = match src.children(dir) {
            Ok(kids) => kids
                .iter()
                .filter(|e| self.keep(&e.name, e.is_dir))
                .map(|e| (e.id, e.is_dir))
                .collect(),
            Err(e) => {
                out.push(Row::Error {
                    depth,
                    text: e.to_string(),
                });
                return;
            }
        };

        for (id, is_dir) in picked {
            out.push(Row::Entry { depth, id });
            if is_dir && self.expanded.contains(&id) {
                self.flatten(src, id, depth + 1, out);
            }
        }
    }

    fn rebuild(&mut self, src: &SharedSource) {
        let root = src.borrow().root();
        let mut rows = Vec::new();
        self.flatten(&mut *src.borrow_mut(), root, 0, &mut rows);
        self.rows = rows;
        self.dirty = false;
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        src: &SharedSource,
        registry: &ViewerRegistry,
    ) -> Vec<BrowserAction> {
        let mut actions = Vec::new();

        ui.horizontal(|ui| {
            let label = if cfg!(target_arch = "wasm32") {
                "Add files…"
            } else {
                "Open folder…"
            };
            if ui.button(label).clicked() {
                actions.push(BrowserAction::ChooseSource);
            }
            if ui.button("Refresh").clicked() {
                self.dirty = true;
            }
        });

        let root_label = {
            let borrowed = src.borrow();
            borrowed.display_path(borrowed.root())
        };
        ui.add(egui::Label::new(egui::RichText::new(root_label).weak()).truncate());

        if ui.text_edit_singleline(&mut self.filter).changed() {
            self.dirty = true;
        }
        ui.separator();

        if self.dirty {
            self.rebuild(src);
        }

        let row_h = ui.text_style_height(&TextStyle::Body) + 4.0;
        let total = self.rows.len();
        if total == 0 {
            ui.weak("nothing here");
            return actions;
        }

        let mut toggled = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_h, total, |ui, range| {
                // Names for the visible rows only, copied out so that no borrow
                // of the source is alive while the rows are drawn — the "Open
                // in" menu below needs the source mutably, and a live immutable
                // borrow would turn that into a panic.
                let visible: Vec<(usize, Option<EntryId>, bool, String)> = {
                    let borrowed = src.borrow();
                    self.rows[range]
                        .iter()
                        .map(|row| match row {
                            Row::Entry { depth, id } => match borrowed.entry(*id) {
                                Some(e) => (*depth, Some(*id), e.is_dir, e.name.clone()),
                                None => (*depth, None, false, "<gone>".to_string()),
                            },
                            Row::Error { depth, text } => (*depth, None, false, text.clone()),
                        })
                        .collect()
                };

                for (depth, id, is_dir, name) in visible {
                    let Some(id) = id else {
                        ui.horizontal(|ui| {
                            ui.add_space(depth as f32 * 14.0);
                            ui.colored_label(ui.visuals().error_fg_color, name);
                        });
                        continue;
                    };

                    ui.horizontal(|ui| {
                        ui.add_space(depth as f32 * 14.0);
                        let marker = if !is_dir {
                            "  "
                        } else if self.expanded.contains(&id) {
                            "v"
                        } else {
                            ">"
                        };
                        let label = format!("{marker} {name}");
                        let response = ui.selectable_label(self.selected == Some(id), label);

                        if response.clicked() {
                            self.selected = Some(id);
                            if is_dir {
                                toggled = Some(id);
                            }
                        }
                        if !is_dir && response.double_clicked() {
                            actions.push(BrowserAction::Open {
                                entry: id,
                                viewer: None,
                            });
                        }
                        if !is_dir {
                            response.context_menu(|ui| {
                                open_in_menu(ui, src, registry, id, &mut actions);
                            });
                        }
                    });
                }
            });

        if let Some(id) = toggled {
            if !self.expanded.remove(&id) {
                self.expanded.insert(id);
            }
            self.dirty = true;
        }

        // Enter opens whatever is selected, so the tree is usable from the
        // keyboard as well as by double-click.
        if let Some(id) = self.selected {
            let is_dir = src.borrow().entry(id).is_some_and(|e| e.is_dir);
            if !is_dir && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                actions.push(BrowserAction::Open {
                    entry: id,
                    viewer: None,
                });
            }
        }

        actions
    }
}

/// The "Open in" menu: every viewer that bid for this file, best first.
///
/// Built from the registry each time it opens rather than cached, so a file
/// that changed on disk is re-probed rather than judged on a stale head.
fn open_in_menu(
    ui: &mut egui::Ui,
    src: &SharedSource,
    registry: &ViewerRegistry,
    id: EntryId,
    actions: &mut Vec<BrowserAction>,
) {
    let blob = match Blob::open(src.clone(), id) {
        Ok(blob) => blob,
        Err(e) => {
            ui.colored_label(ui.visuals().error_fg_color, e.to_string());
            return;
        }
    };
    let claims = registry.claims_for(&blob.probe());
    if claims.is_empty() {
        ui.weak("no viewer wants this file");
        return;
    }
    ui.label("Open in");
    ui.separator();
    for (viewer, claim) in claims {
        if ui.button(&claim.label).clicked() {
            actions.push(BrowserAction::Open {
                entry: id,
                viewer: Some(viewer.to_string()),
            });
            ui.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use silva_viz_core::MemSource;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn source() -> SharedSource {
        let mut mem = MemSource::new();
        mem.add("alpha.txt", b"a".to_vec());
        mem.add("beta.csv", b"b".to_vec());
        mem.add("gamma.png", b"g".to_vec());
        Rc::new(RefCell::new(mem))
    }

    fn ids(browser: &Browser) -> Vec<EntryId> {
        browser
            .rows
            .iter()
            .filter_map(|r| match r {
                Row::Entry { id, .. } => Some(*id),
                Row::Error { .. } => None,
            })
            .collect()
    }

    #[test]
    fn test_every_file_appears_before_any_filter_is_typed() {
        let mut browser = Browser::new();
        browser.rebuild(&source());
        assert_eq!(ids(&browser).len(), 3);
    }

    #[test]
    fn test_the_filter_is_case_insensitive() {
        let src = source();
        let mut browser = Browser::new();
        browser.filter = "BETA".to_string();
        browser.rebuild(&src);
        assert_eq!(ids(&browser).len(), 1);
    }

    #[test]
    fn test_a_filter_matching_nothing_leaves_an_empty_tree_rather_than_everything() {
        let mut browser = Browser::new();
        browser.filter = "no-such-thing".to_string();
        browser.rebuild(&source());
        assert!(ids(&browser).is_empty());
    }

    #[test]
    fn test_a_directory_survives_a_filter_that_its_name_fails() {
        // Hiding the folder would hide the matching files inside it, which is
        // the one way a filter can make a file unreachable.
        let browser = Browser {
            filter: "zzz".to_string(),
            ..Default::default()
        };
        assert!(browser.keep("src", true));
        assert!(!browser.keep("main.rs", false));
    }

    #[test]
    fn test_a_rebuild_is_only_needed_once_per_change() {
        let mut browser = Browser::new();
        assert!(browser.dirty);
        browser.rebuild(&source());
        assert!(!browser.dirty);
        browser.invalidate();
        assert!(browser.dirty);
    }
}
