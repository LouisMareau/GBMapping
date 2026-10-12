//! Files in and out, on the desktop and in the browser. Whatever way a file comes in
//! (the dialog, a drop, the command line), it lands in one queue the app empties each
//! frame; nothing is ever sent anywhere.

use std::sync::mpsc::{channel, Receiver, Sender};

pub struct Incoming {
    pub name: String,
    pub bytes: Result<Vec<u8>, String>,
    pub purpose: Purpose,
}

/// What a file was asked for
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// To be opened: the ROM to edit, a profile, edits
    Open,
    /// A ROM to look at next to the one that's edited
    Reference,
}

pub struct Files {
    tx: Sender<Incoming>,
    rx: Receiver<Incoming>,
    ctx: egui::Context,
}

/// What a dialog is for
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A ROM, a profile, an edits file: anything the editor opens
    Any,
    Rom,
    Json,
}

impl Kind {
    fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Any => &["gba", "gbc", "gb", "json"],
            Self::Rom => &["gba", "gbc", "gb"],
            Self::Json => &["json"],
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn label(self) -> &'static str {
        match self {
            Self::Any => "ROMs, profiles and edits",
            Self::Rom => "Game Boy Advance and Game Boy Color ROMs",
            Self::Json => "GBMapping edits and profiles",
        }
    }
}

impl Files {
    pub fn new(ctx: &egui::Context) -> Self {
        let (tx, rx) = channel();
        Self { tx, rx, ctx: ctx.clone() }
    }

    /// The files that arrived since the last call
    pub fn take(&self) -> Vec<Incoming> {
        self.rx.try_iter().collect()
    }

    /// The files dropped on the window this frame, into the queue
    pub fn take_dropped(&self) {
        let dropped = self.ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
        for file in dropped {
            let name = file.path().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "dropped file".to_string());
            #[cfg(not(target_arch = "wasm32"))]
            self.tx.send(Incoming { name, bytes: file.bytes(), purpose: Purpose::Open }).ok();
            #[cfg(target_arch = "wasm32")]
            {
                let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
                wasm_bindgen_futures::spawn_local(async move {
                    let bytes = file.bytes_async().await;
                    tx.send(Incoming { name, bytes, purpose: Purpose::Open }).ok();
                    ctx.request_repaint();
                });
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Files {
    pub fn read_paths(&self, paths: &[std::path::PathBuf], purpose: Purpose) {
        for path in paths {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            self.tx.send(Incoming { name, bytes: std::fs::read(path).map_err(|e| e.to_string()), purpose }).ok();
        }
        self.ctx.request_repaint();
    }

    /// Asks for files to open; they arrive in the queue
    pub fn open(&self, kind: Kind, purpose: Purpose) {
        if let Some(paths) = rfd::FileDialog::new().add_filter(kind.label(), kind.extensions()).pick_files() {
            self.read_paths(&paths, purpose);
        }
    }

    /// Saves a file where the user says. `Ok(None)` when they cancel.
    pub fn save(&self, name: &str, kind: Kind, bytes: &[u8]) -> Result<Option<String>, String> {
        let Some(path) = rfd::FileDialog::new().add_filter(kind.label(), kind.extensions()).set_file_name(name).save_file() else {
            return Ok(None);
        };
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Some(path.display().to_string()))
    }
}

#[cfg(target_arch = "wasm32")]
impl Files {
    /// Asks for files to open (the browser's file chooser); they arrive in the queue
    pub fn open(&self, kind: Kind, purpose: Purpose) {
        use wasm_bindgen::{closure::Closure, JsCast as _};

        let Some(input) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.create_element("input").ok())
            .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
        else {
            return;
        };
        input.set_type("file");
        input.set_multiple(true);
        input.set_accept(&kind.extensions().iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(","));
        let (tx, ctx, chosen) = (self.tx.clone(), self.ctx.clone(), input.clone());
        let on_change = Closure::<dyn FnMut()>::new(move || {
            let Some(files) = chosen.files() else { return };
            for file in (0..files.length()).filter_map(|i| files.get(i)) {
                let (tx, ctx) = (tx.clone(), ctx.clone());
                wasm_bindgen_futures::spawn_local(async move {
                    let bytes = wasm_bindgen_futures::JsFuture::from(file.array_buffer())
                        .await
                        .map(|buffer| js_sys::Uint8Array::new(&buffer).to_vec())
                        .map_err(|e| format!("{e:?}"));
                    tx.send(Incoming { name: file.name(), bytes, purpose }).ok();
                    ctx.request_repaint();
                });
            }
        });
        input.set_onchange(Some(on_change.as_ref().unchecked_ref()));
        // The closure lives as long as the page: a chooser may stay open for long
        on_change.forget();
        input.click();
    }

    /// Hands a file to the browser as a download
    pub fn save(&self, name: &str, kind: Kind, bytes: &[u8]) -> Result<Option<String>, String> {
        use wasm_bindgen::JsCast as _;

        let error = |e: wasm_bindgen::JsValue| format!("{e:?}");
        let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
        let options = web_sys::BlobPropertyBag::new();
        options.set_type(if kind == Kind::Json { "application/json" } else { "application/octet-stream" });
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options).map_err(error)?;
        let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(error)?;
        let document = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
        let link = document.create_element("a").map_err(error)?.dyn_into::<web_sys::HtmlAnchorElement>().map_err(|_| "no link")?;
        link.set_href(&url);
        link.set_download(name);
        link.click();
        // the address is given back once the browser has had time to start the download
        let revoke = wasm_bindgen::closure::Closure::once_into_js(move || {
            web_sys::Url::revoke_object_url(&url).ok();
        });
        if let Some(window) = web_sys::window() {
            window.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 30_000).ok();
        }
        Ok(Some(format!("{name} (in your downloads)")))
    }
}

/// The window's title on the desktop, the tab's in the browser
pub fn set_title(ctx: &egui::Context, title: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.to_string()));
    #[cfg(target_arch = "wasm32")]
    if let Some(document) = web_sys::window().and_then(|w| w.document()) {
        let _ = ctx;
        document.set_title(title);
    }
}
