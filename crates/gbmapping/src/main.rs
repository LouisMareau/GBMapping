//! GBMapping: a map and tileset editor for Game Boy Advance ROMs. One program for the
//! browser (WebAssembly, installable as an app) and for the desktop.

// No console window behind the desktop app on Windows
#![cfg_attr(all(not(debug_assertions), not(target_arch = "wasm32")), windows_subsystem = "windows")]

mod app;
mod doc;
mod files;
mod map_view;
mod reference;
mod side;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("GBMapping")
            .with_inner_size([1360.0, 840.0])
            .with_min_inner_size([720.0, 420.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    // Files named on the command line are opened at once: gbmapping game.gba profile.json
    let arguments: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    eframe::run_native(
        "GBMapping",
        options,
        Box::new(move |cc| {
            let app = app::App::new(cc);
            app.files.read_paths(&arguments, files::Purpose::Open);
            Ok(Box::new(app))
        }),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast as _;

    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window().and_then(|w| w.document()).expect("no document");
        let canvas = document
            .get_element_by_id("gbmapping")
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .expect("no canvas with the id gbmapping");
        let started =
            eframe::WebRunner::new().start(canvas, eframe::WebOptions::default(), Box::new(|cc| Ok(Box::new(app::App::new(cc))))).await;
        // The page shows a loading text until then
        if let Some(loading) = document.get_element_by_id("loading") {
            match &started {
                Ok(()) => loading.remove(),
                Err(_) => {
                    loading.set_inner_html("<p>GBMapping couldn't start: this browser may lack WebGL 2. The console has the details.</p>")
                }
            }
        }
        if let Err(e) = started {
            log::error!("eframe didn't start: {e:?}");
        }
    });
}
