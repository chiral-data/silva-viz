// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The shell: a browser on the left, floating viewers over the rest.

use crate::browser::{Browser, BrowserAction};
use crate::dnd::{self, Dropped};
use crate::views;
use crate::windows::WindowManager;
#[cfg(target_arch = "wasm32")]
use silva_viz_core::MemSource;
use silva_viz_core::{Blob, SharedSource, Task, ViewerRegistry};
use std::cell::RefCell;
use std::rc::Rc;

/// Bumped only when the shape below changes incompatibly. eframe silently
/// discards state it cannot deserialise, so a stale key costs a user their
/// layout rather than crashing — which is exactly why it should not change for
/// a compatible edit.
const STORAGE_KEY: &str = "silva_viz_v1";

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Persisted {
    root: Option<String>,
    /// `(viewer id, path)` per open window. Geometry is deliberately absent —
    /// egui persists that itself, against the window id.
    views: Vec<(String, String)>,
}

/// The outcome of a file dialog.
enum Picked {
    /// A new root directory. Native only — a browser's picker returns contents,
    /// never a location, so the web build can never produce this.
    #[cfg(not(target_arch = "wasm32"))]
    Root(String),
    /// Files with their contents, because the platform has no paths.
    #[cfg(target_arch = "wasm32")]
    Files(Vec<(String, Vec<u8>)>),
}

/// The viewers this app ships with.
///
/// Registration order settles ties, and there are none among these five — but
/// it is also the order the "Open in" menu falls back to, so it is written
/// worst-bid-first to read the way the menu does.
pub fn default_registry() -> ViewerRegistry {
    let mut registry = ViewerRegistry::new();
    registry
        .register(Box::new(views::meta::MetaFactory))
        .register(Box::new(views::hex::HexFactory))
        .register(Box::new(views::text::TextFactory))
        .register(Box::new(views::table::TableFactory))
        .register(Box::new(views::image::ImageFactory));
    registry
}

pub struct SilvaVizApp {
    source: SharedSource,
    /// The same allocation as `source` on the web, kept at its concrete type so
    /// dropped bytes have somewhere to go. There is no filesystem there, so
    /// this *is* the file system.
    #[cfg(target_arch = "wasm32")]
    mem: Rc<RefCell<MemSource>>,
    registry: ViewerRegistry,
    browser: Browser,
    windows: WindowManager,
    picker: Task<Option<Picked>>,
    status: String,
}

impl SilvaVizApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Without this the image viewer paints an empty rectangle and says
        // nothing about why.
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let persisted: Persisted = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, STORAGE_KEY))
            .unwrap_or_default();

        #[cfg(target_arch = "wasm32")]
        let (source, mem) = {
            let mem = Rc::new(RefCell::new(MemSource::new()));
            let source: SharedSource = mem.clone();
            (source, mem)
        };

        #[cfg(not(target_arch = "wasm32"))]
        let source: SharedSource = {
            let root = persisted
                .root
                .as_deref()
                .map(std::path::PathBuf::from)
                .filter(|p| p.is_dir())
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            Rc::new(RefCell::new(silva_viz_core::FsSource::new(root)))
        };

        let mut app = Self {
            source,
            #[cfg(target_arch = "wasm32")]
            mem,
            registry: default_registry(),
            browser: Browser::new(),
            windows: WindowManager::default(),
            picker: Task::new(),
            status: String::new(),
        };

        // Restored against the screen rather than the workspace: the panels
        // have not been drawn yet, so the real workspace is a frame away, and a
        // window's saved geometry usually replaces this rect anyway.
        let workspace = cc.egui_ctx.content_rect();
        for (viewer, path) in persisted.views {
            // A file that has moved since the last session is skipped rather
            // than reported: a missing window is a quieter answer than five
            // error dialogs at startup.
            app.open_path(&viewer, &path, workspace);
        }
        app
    }

    fn open_path(&mut self, viewer: &str, path: &str, workspace: egui::Rect) -> Option<()> {
        let id = self.source.borrow_mut().resolve(path)?;
        self.open(id, Some(viewer.to_string()), workspace);
        Some(())
    }

    fn open(
        &mut self,
        entry: silva_viz_core::EntryId,
        viewer: Option<String>,
        workspace: egui::Rect,
    ) {
        let blob = match Blob::open(self.source.clone(), entry) {
            Ok(blob) => blob,
            Err(e) => {
                self.status = e.to_string();
                return;
            }
        };
        let viewer = match viewer.or_else(|| {
            self.registry
                .best_for(&blob.probe())
                .map(ToString::to_string)
        }) {
            Some(viewer) => viewer,
            None => {
                self.status = format!("no viewer wants {}", blob.name());
                return;
            }
        };
        let path = blob.path().to_string();
        let Some(view) = self.registry.open(&viewer, blob) else {
            self.status = format!("no viewer called `{viewer}`");
            return;
        };
        if let Some(evicted) = self.windows.open(&viewer, path, view, workspace) {
            self.status = format!("closed `{evicted}` to stay within the window limit");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn choose_source(&mut self, ctx: &egui::Context) {
        self.picker.start(ctx, async {
            rfd::AsyncFileDialog::new()
                .pick_folder()
                .await
                .map(|handle| Picked::Root(handle.path().to_string_lossy().into_owned()))
        });
    }

    #[cfg(target_arch = "wasm32")]
    fn choose_source(&mut self, ctx: &egui::Context) {
        self.picker.start(ctx, async {
            let handles = rfd::AsyncFileDialog::new().pick_files().await?;
            let mut files = Vec::new();
            for handle in handles {
                files.push((handle.file_name(), handle.read().await));
            }
            Some(Picked::Files(files))
        });
    }

    fn apply(&mut self, picked: Picked) {
        match picked {
            #[cfg(not(target_arch = "wasm32"))]
            Picked::Root(path) => self.set_root(&path),
            #[cfg(target_arch = "wasm32")]
            Picked::Files(files) => {
                for (name, bytes) in files {
                    self.add_bytes(name, bytes);
                }
                self.browser.invalidate();
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn set_root(&mut self, path: &str) {
        // A fresh source rather than a re-root: windows already open hold their
        // own reference to the old one and keep working, which is what makes
        // changing folders a navigation rather than a reset.
        self.source = Rc::new(RefCell::new(silva_viz_core::FsSource::new(path)));
        self.browser.invalidate();
        self.status = format!("root: {path}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn add_bytes(&mut self, name: String, _bytes: Vec<u8>) {
        // The native build browses the filesystem, so bytes without a path have
        // nowhere to live. Reachable only if a platform stops reporting paths.
        self.status = format!("`{name}` arrived without a path and was not added");
    }

    #[cfg(target_arch = "wasm32")]
    fn add_bytes(&mut self, name: String, bytes: Vec<u8>) {
        self.mem.borrow_mut().add(name, bytes);
    }

    fn apply_dropped(&mut self, dropped: Vec<Dropped>, workspace: egui::Rect) {
        for item in dropped {
            match item {
                Dropped::Bytes { name, bytes } => {
                    self.add_bytes(name, bytes);
                    self.browser.invalidate();
                }
                Dropped::Path(path) => self.drop_path(&path, workspace),
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn drop_path(&mut self, path: &str, workspace: egui::Rect) {
        let path = std::path::PathBuf::from(path);
        if path.is_dir() {
            self.set_root(&path.to_string_lossy());
            return;
        }
        // Dropping a file means "show me this", so the root moves to its folder
        // and the file opens — rather than the file being browsable but shut.
        if let Some(parent) = path.parent() {
            self.set_root(&parent.to_string_lossy());
        }
        let resolved = self.source.borrow_mut().resolve(&path.to_string_lossy());
        if let Some(id) = resolved {
            self.open(id, None, workspace);
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn drop_path(&mut self, _path: &str, _workspace: egui::Rect) {
        // Unreachable: a browser never reports a path for a dropped file.
    }
}

impl eframe::App for SilvaVizApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let persisted = Persisted {
            root: Some(
                self.source
                    .borrow()
                    .display_path(self.source.borrow().root()),
            ),
            views: self.windows.descriptors(),
        };
        eframe::set_value(storage, STORAGE_KEY, &persisted);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // The outer `Some` is "the dialog closed"; the inner is "with a
        // choice". A cancelled dialog is the flattened `None`, and must not be
        // mistaken for one that is still open.
        if let Some(Some(picked)) = self.picker.poll() {
            self.apply(picked);
        }

        let mut actions = Vec::new();
        let dropped = dnd::take(ctx);

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    let label = if cfg!(target_arch = "wasm32") {
                        "Add files…"
                    } else {
                        "Open folder…"
                    };
                    if ui.button(label).clicked() {
                        actions.push(BrowserAction::ChooseSource);
                        ui.close();
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("View", |ui| self.windows.menu(ui));
                ui.separator();
                ui.weak(format!("{} open", self.windows.len()));
            });
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if dnd::hovering(ctx) {
                    ui.strong("drop to open");
                } else if self.status.is_empty() {
                    ui.weak("double-click a file to open it; right-click for other viewers");
                } else {
                    ui.weak(&self.status);
                }
            });
        });

        egui::SidePanel::left("browser")
            .resizable(true)
            .default_width(280.0)
            .show(ctx, |ui| {
                actions.extend(self.browser.ui(ui, &self.source, &self.registry));
            });

        // Read *after* the panels, or every window would be laid out under the
        // browser and only reachable by dragging it out.
        let workspace = ctx.available_rect();

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.windows.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.weak("no open viewers");
                });
            }
        });

        if !dropped.is_empty() {
            self.apply_dropped(dropped, workspace);
        }

        for action in actions {
            match action {
                BrowserAction::ChooseSource => self.choose_source(ctx),
                BrowserAction::Open { entry, viewer } => self.open(entry, viewer, workspace),
            }
        }

        self.windows.show(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use silva_viz_core::FileProbe;

    #[test]
    fn test_a_text_file_opens_as_text_rather_than_as_hex() {
        let registry = default_registry();
        let probe = FileProbe::new("notes.txt", b"hello\nworld\n", 12);
        assert_eq!(registry.best_for(&probe), Some("text"));
    }

    #[test]
    fn test_a_csv_beats_text_so_a_double_click_shows_columns() {
        let registry = default_registry();
        let probe = FileProbe::new("d.csv", b"a,b,c\n1,2,3\n4,5,6\n", 18);
        assert_eq!(registry.best_for(&probe), Some("table"));
    }

    #[test]
    fn test_a_binary_falls_through_to_hex_and_metadata_only() {
        let registry = default_registry();
        let probe = FileProbe::new("blob.bin", &[0u8, 1, 2, 255], 4);
        let ids: Vec<_> = registry
            .claims_for(&probe)
            .into_iter()
            .map(|c| c.0)
            .collect();
        assert_eq!(ids, ["hex", "meta"]);
    }

    #[test]
    fn test_a_png_named_txt_opens_as_an_image_and_text_is_not_offered() {
        // The whole sniffing design in one assertion.
        let registry = default_registry();
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x01\x00";
        let probe = FileProbe::new("pretend.txt", png, png.len() as u64);

        assert_eq!(registry.best_for(&probe), Some("image"));
        let ids: Vec<_> = registry
            .claims_for(&probe)
            .into_iter()
            .map(|c| c.0)
            .collect();
        assert_eq!(ids, ["image", "hex", "meta"]);
    }

    #[test]
    fn test_every_file_has_at_least_two_viewers_so_none_is_unopenable() {
        let registry = default_registry();
        for (name, head) in [
            ("a.txt", &b"hi"[..]),
            ("b.bin", &[0u8, 0, 0][..]),
            ("", &[][..]),
        ] {
            let probe = FileProbe::new(name, head, head.len() as u64);
            assert!(registry.claims_for(&probe).len() >= 2, "{name}");
        }
    }
}
