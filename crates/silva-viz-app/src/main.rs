// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_drag_and_drop(true)
            .with_title("silva-viz"),
        ..Default::default()
    };

    eframe::run_native(
        "silva-viz",
        options,
        Box::new(|cc| Ok(Box::new(silva_viz_app::SilvaVizApp::new(cc)))),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast as _;

    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    console_error_panic_hook::set_once();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("no window")
            .document()
            .expect("no document");

        let canvas = document
            .get_element_by_id(silva_viz_app::CANVAS_ID)
            .expect("the canvas named by CANVAS_ID is missing from index.html")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("the element named by CANVAS_ID is not a canvas");

        let started = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(silva_viz_app::SilvaVizApp::new(cc)))),
            )
            .await;

        // The plain HTML overlay has been up since page load, well before the
        // wasm bundle finished downloading. Removing it here rather than on a
        // timer means the page is never blank and never lies about being ready.
        let overlay = document.get_element_by_id("loading_overlay");
        match started {
            Ok(()) => {
                if let Some(overlay) = overlay {
                    overlay.remove();
                }
            }
            Err(e) => {
                log::error!("failed to start eframe: {e:?}");
                if let Some(overlay) = overlay {
                    overlay.set_text_content(Some(&format!("Failed to start silva-viz: {e:?}")));
                }
            }
        }
    });
}
