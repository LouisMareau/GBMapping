//! The editor's window: a toolbar, the maps' list, the map, the side panel, a status
//! line. A ROM comes in by the dialog or a drop and leaves by "Save ROM".

use crate::doc::{Doc, Tool};
use crate::files::{Files, Incoming, Kind, Purpose};
use crate::reference::{self, Reference};
use crate::{map_view, side};
use egui::{Color32, Key, KeyboardShortcut, Modifiers, RichText, Ui};
use gbmapping_core::format::block;
use gbmapping_core::{Edits, Game, Profile};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum Tab {
    Metatiles,
    Tileset,
}

/// What's kept from one session to the next
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Settings {
    pub zoom: u32,
    pub tool: Tool,
    pub elevation: u8,
    /// Painting from the picker also sets a metatile's usual collision and elevation
    pub auto_collision: bool,
    pub show_grid: bool,
    pub show_collision: bool,
    pub show_elevation: bool,
    /// The read-only view next to the map
    pub show_reference: bool,
    /// The tileset tab draws each tile in the palette it's used with (else all in the selected quarter's)
    pub usual_colors: bool,
    /// The tileset tab lists every tileset's tiles, not only the map's two
    pub all_tilesets: bool,
    pub tab: Tab,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            zoom: 2,
            tool: Tool::Paint,
            elevation: 3,
            auto_collision: true,
            show_grid: false,
            show_collision: false,
            show_elevation: false,
            show_reference: false,
            usual_colors: true,
            all_tilesets: false,
            tab: Tab::Metatiles,
        }
    }
}

const SETTINGS_KEY: &str = "settings";
/// Above this, the edits aren't kept in the browser's storage (it holds about 5 MB)
const MAX_STORED_EDITS: usize = 4_000_000;

const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const REDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
const REDO_SHIFT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);

struct Notice {
    text: String,
    error: bool,
}

/// A message that waits for an answer
enum Dialog {
    /// Something went wrong, or needs telling at length
    Message { title: String, text: String },
    /// Edits for this ROM were found in the storage
    StoredEdits(Edits),
}

pub struct App {
    pub files: Files,
    settings: Settings,
    saved_settings: Settings,
    /// The profile ROMs are opened with (the last one dropped)
    profile: Profile,
    doc: Option<Doc>,
    search: String,
    notice: Option<Notice>,
    dialog: Option<Dialog>,
    /// The edits can't be kept in the storage (too many): said once
    storage_full: bool,
    /// The window's (or the tab's) title, as last set
    title: String,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings: Settings = cc.storage.and_then(|s| eframe::get_value(s, SETTINGS_KEY)).unwrap_or_default();
        cc.egui_ctx.options_mut(|o| {
            // dark whatever the system's theme: the maps are what's to be seen
            o.theme_preference = egui::ThemePreference::Dark;
            // a browser tab has no window to pass the theme to (eframe says so in the console)
            o.sync_window_theme = !cfg!(target_arch = "wasm32");
        });
        Self {
            files: Files::new(&cc.egui_ctx),
            saved_settings: settings.clone(),
            settings: Settings { zoom: settings.zoom.clamp(1, map_view::MAX_ZOOM), ..settings },
            profile: Profile::default(),
            doc: None,
            search: String::new(),
            notice: None,
            dialog: None,
            storage_full: false,
            title: String::new(),
        }
    }

    /// The map and the ROM in the window's title, or the tab's
    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match self.doc.as_ref() {
            Some(doc) => match doc.map.as_ref() {
                Some(map) => format!("{} · {} — GBMapping", map.name, doc.file_name),
                None => format!("{} — GBMapping", doc.file_name),
            },
            None => "GBMapping".to_string(),
        };
        if title != self.title {
            crate::files::set_title(ctx, &title);
            self.title = title;
        }
    }

    fn say(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice { text: text.into(), error: false });
    }

    fn fail(&mut self, title: &str, text: impl Into<String>) {
        self.dialog = Some(Dialog::Message { title: title.to_string(), text: text.into() });
    }

    // -- files in ---------------------------------------------------------------------

    fn receive(&mut self, frame: &mut eframe::Frame) {
        self.files.take_dropped();
        let incoming = self.files.take();
        if incoming.is_empty() {
            return;
        }
        // Profiles first, so that a ROM dropped with its profile opens with it; edits last
        let (mut profiles, mut roms, mut references, mut edits) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for Incoming { name, bytes, purpose } in incoming {
            let bytes = match bytes {
                Ok(bytes) => bytes,
                Err(e) => {
                    self.fail("That file can't be read", format!("{name}: {e}"));
                    continue;
                }
            };
            let json = name.to_ascii_lowercase().ends_with(".json") || bytes.first() == Some(&b'{');
            match (json, std::str::from_utf8(&bytes)) {
                (true, Ok(text)) if Edits::looks_like(text) => edits.push((name, text.to_string())),
                (true, Ok(text)) => profiles.push((name, text.to_string())),
                (true, Err(_)) => self.fail("That file can't be read", format!("{name} isn't text.")),
                (false, _) if purpose == Purpose::Reference => references.push((name, bytes)),
                (false, _) => roms.push((name, bytes)),
            }
        }
        for (name, text) in profiles {
            match Profile::from_json(&text) {
                Ok(profile) => {
                    self.profile = profile;
                    self.say(format!("Profile {name}: the next ROM opens with it"));
                    if roms.is_empty() {
                        self.reopen_with_profile();
                    }
                }
                Err(e) => self.fail("That isn't a profile", format!("{name}: {e}")),
            }
        }
        if let Some((name, bytes)) = roms.pop() {
            self.open_rom(name, bytes, frame);
        }
        for (name, text) in edits {
            self.import_edits(&name, &text);
        }
        if let Some((name, bytes)) = references.pop() {
            self.open_reference(name, bytes);
        }
    }

    /// Another ROM to look at next to the one being edited
    fn open_reference(&mut self, name: String, bytes: Vec<u8>) {
        let Some(doc) = self.doc.as_mut() else {
            self.fail("Open a ROM first", "The reference is shown next to the ROM being edited.");
            return;
        };
        match Reference::other(bytes, name.clone()) {
            Ok(reference) => {
                let maps = reference.game.maps.len();
                doc.reference = Some(reference);
                self.settings.show_reference = true;
                self.say(format!("Reference: {name}, {maps} maps"));
            }
            Err(e) => self.fail("That ROM can't be the reference", format!("{name}: {e}")),
        }
    }

    fn open_rom(&mut self, name: String, bytes: Vec<u8>, frame: &mut eframe::Frame) {
        match Game::open(bytes, &name, self.profile.clone()) {
            Ok(game) => {
                let stored = frame
                    .storage()
                    .and_then(|s| s.get_string(&storage_key(&game)))
                    .and_then(|json| Edits::from_json(&json).ok())
                    .filter(|e| !e.is_empty());
                self.say(format!("{name}: {} maps", game.maps.len()));
                let reference = self.doc.take().and_then(|d| d.reference).filter(|r| !r.own);
                let mut doc = Doc::new(game, name);
                doc.reference = reference;
                self.doc = Some(doc);
                self.search.clear();
                self.storage_full = false;
                self.dialog = stored.map(Dialog::StoredEdits);
            }
            Err(e) => self.fail("That ROM can't be opened", format!("{name}\n\n{e}")),
        }
    }

    /// A profile dropped on an open ROM: the ROM again, its names changed, its edits kept
    fn reopen_with_profile(&mut self) {
        let Some(doc) = self.doc.as_mut() else { return };
        doc.end_stroke();
        let edits = doc.game.collect_edits();
        let map = doc.map.as_ref().map(|m| m.id);
        match Game::open(doc.game.original().bytes().to_vec(), &doc.file_name, self.profile.clone()) {
            Ok(mut game) => {
                game.apply_edits(&edits);
                game.commit();
                let mut reopened = Doc::new(game, doc.file_name.clone());
                if let Some(index) = map.and_then(|id| reopened.game.map_index(id)) {
                    reopened.open_map(index);
                }
                reopened.reload();
                reopened.unsaved_edits = false;
                reopened.reference = doc.reference.take().filter(|r| !r.own);
                self.doc = Some(reopened);
                self.say("The ROM is shown with the profile's names");
            }
            Err(e) => self.fail("The profile doesn't fit this ROM", e.to_string()),
        }
    }

    fn import_edits(&mut self, name: &str, json: &str) {
        let Some(doc) = self.doc.as_mut() else {
            self.fail("Open a ROM first", format!("{name} holds edits: they're applied to the ROM that's open."));
            return;
        };
        match Edits::from_json(json) {
            Ok(edits) => {
                doc.end_stroke();
                let applied = doc.game.apply_edits(&edits);
                doc.commit();
                let summary = format!("{name}: {} map(s) and {} tileset(s) changed", applied.maps, applied.tilesets);
                if applied.skipped.is_empty() {
                    self.say(summary);
                } else {
                    self.fail("Some edits don't fit this ROM", format!("{summary}.\n\nLeft out:\n• {}", applied.skipped.join("\n• ")));
                }
            }
            Err(e) => self.fail("Those edits can't be read", format!("{name}: {e}")),
        }
    }

    // -- files out --------------------------------------------------------------------

    fn save_rom(&mut self) {
        let Some(doc) = self.doc.as_mut() else { return };
        doc.end_stroke();
        let stem = doc.file_name.rsplit_once('.').map_or(doc.file_name.as_str(), |(stem, _)| stem);
        let name = if stem.ends_with("-edited") { format!("{stem}.gba") } else { format!("{stem}-edited.gba") };
        let saved = self.files.save(&name, Kind::Rom, doc.game.rom.bytes());
        self.saved("The ROM", saved);
    }

    fn export_edits(&mut self) {
        let Some(doc) = self.doc.as_mut() else { return };
        doc.end_stroke();
        let edits = doc.game.collect_edits();
        if edits.is_empty() {
            self.say("Nothing was changed: there are no edits to export");
            return;
        }
        let title = doc.game.rom.header().title.trim().to_ascii_lowercase().replace(' ', "-");
        let saved = self.files.save(&format!("{title}-edits.json"), Kind::Json, edits.to_json().as_bytes());
        self.saved("The edits", saved);
    }

    fn saved(&mut self, what: &str, result: Result<Option<String>, String>) {
        match result {
            Ok(Some(path)) => self.say(format!("{what}: saved as {path}")),
            Ok(None) => {}
            Err(e) => self.fail("That couldn't be saved", e),
        }
    }

    /// The edits made to a ROM are kept (in the browser's storage, in the app's data on
    /// the desktop) and offered again when the ROM comes back
    fn keep_edits(&mut self, frame: &mut eframe::Frame) {
        let Some(doc) = self.doc.as_mut().filter(|d| d.unsaved_edits && d.stroke.is_none()) else { return };
        if matches!(self.dialog, Some(Dialog::StoredEdits(_))) {
            return; // what's stored is still to be answered for
        }
        doc.unsaved_edits = false;
        let Some(storage) = frame.storage_mut() else { return };
        let key = storage_key(&doc.game);
        let edits = doc.game.collect_edits();
        if edits.is_empty() {
            storage.remove_string(&key);
            return;
        }
        let json = edits.to_json();
        if json.len() > MAX_STORED_EDITS {
            if !self.storage_full {
                self.storage_full = true;
                self.notice = Some(Notice {
                    text: "So many edits that they're no longer kept between sessions: save the ROM or export the edits".to_string(),
                    error: true,
                });
            }
            return;
        }
        storage.set_string(&key, json);
        storage.flush();
    }

    // -- the window --------------------------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let pressed = |shortcut: &KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(shortcut));
        if pressed(&REDO_SHIFT) || pressed(&REDO) {
            self.redo();
        } else if pressed(&UNDO) {
            self.undo();
        }
        if pressed(&OPEN) {
            self.files.open(Kind::Any, Purpose::Open);
        }
        if pressed(&SAVE) {
            self.save_rom();
        }
        // plain keys, unless something is being typed
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let key = |key: Key| ctx.input(|i| i.modifiers.is_none() && i.key_pressed(key));
        let s = &mut self.settings;
        for (k, tool) in [(Key::P, Tool::Paint), (Key::F, Tool::Fill), (Key::K, Tool::Collision), (Key::L, Tool::Elevation)] {
            if key(k) {
                s.tool = tool;
            }
        }
        s.show_grid ^= key(Key::G);
        s.show_collision ^= key(Key::C);
        s.show_elevation ^= key(Key::E);
        s.show_reference ^= key(Key::R);
        if key(Key::Plus) || key(Key::Equals) {
            s.zoom = (s.zoom + 1).min(map_view::MAX_ZOOM);
        }
        if key(Key::Minus) {
            s.zoom = s.zoom.saturating_sub(1).max(1);
        }
    }

    fn undo(&mut self) {
        if let Some(doc) = self.doc.as_mut() {
            if !doc.undo() {
                self.say("Nothing to undo");
            }
        }
    }

    fn redo(&mut self) {
        if let Some(doc) = self.doc.as_mut() {
            if !doc.redo() {
                self.say("Nothing to redo");
            }
        }
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(RichText::new("GBMapping").strong());
            ui.separator();
            if ui.button("Open…").on_hover_text("A ROM (.gba), a profile or an edits file (.json). Dropping them on the window works too.").clicked() {
                self.files.open(Kind::Any, Purpose::Open);
            }
            let open = self.doc.is_some();
            let modified = self.doc.as_ref().is_some_and(|d| d.game.modified());
            let save = egui::Button::new(RichText::new("Save ROM").color(if modified { Color32::WHITE } else { ui.visuals().text_color() })).fill(if modified { Color32::from_rgb(56, 142, 60) } else { ui.visuals().widgets.inactive.weak_bg_fill });
            if ui.add_enabled(open, save).on_hover_text("The ROM with your edits, as a new file").clicked() {
                self.save_rom();
            }
            if ui.add_enabled(modified, egui::Button::new("Export edits")).on_hover_text("Only what changed, as a small .json: to share, or to apply to another build of the game").clicked() {
                self.export_edits();
            }
            if ui.add_enabled(open, egui::Button::new("Import edits…")).on_hover_text("Applies an edits .json to this ROM").clicked() {
                self.files.open(Kind::Json, Purpose::Open);
            }
            if !open {
                return;
            }
            ui.separator();
            let (can_undo, can_redo) = self.doc.as_ref().map_or((false, false), |d| (d.game.can_undo(), d.game.can_redo()));
            if ui.add_enabled(can_undo, egui::Button::new("Undo")).on_hover_text("Ctrl+Z").clicked() {
                self.undo();
            }
            if ui.add_enabled(can_redo, egui::Button::new("Redo")).on_hover_text("Ctrl+Y").clicked() {
                self.redo();
            }
            ui.separator();
            let s = &mut self.settings;
            for tool in Tool::ALL {
                ui.selectable_value(&mut s.tool, tool, tool.label()).on_hover_text(tool.hint());
            }
            match s.tool {
                Tool::Elevation => {
                    ui.add(egui::DragValue::new(&mut s.elevation).range(0..=15).speed(0.1).prefix("level ")).on_hover_text("The elevation to set: 3 is the usual ground, 1 is water, 0 and 15 connect levels");
                }
                Tool::Paint | Tool::Fill => {
                    ui.checkbox(&mut s.auto_collision, "Usual collision").on_hover_text(
                        "A metatile taken in the picker comes with the collision and elevation it usually has on this game's maps: a tree blocks, water is at its level. Off: only the picture changes.\nWhat's picked on the map (right-click) always comes exactly as it was.",
                    );
                }
                Tool::Collision => {}
            }
            ui.separator();
            ui.label("Zoom");
            if ui.small_button("−").clicked() {
                s.zoom = s.zoom.saturating_sub(1).max(1);
            }
            ui.label(format!("{}×", s.zoom));
            if ui.small_button("+").clicked() {
                s.zoom = (s.zoom + 1).min(map_view::MAX_ZOOM);
            }
            ui.separator();
            ui.checkbox(&mut s.show_grid, "Grid").on_hover_text("G");
            ui.checkbox(&mut s.show_collision, "Collision").on_hover_text("C: impassable tiles in red");
            ui.checkbox(&mut s.show_elevation, "Elevation").on_hover_text("E: each tile's elevation");
            ui.separator();
            ui.checkbox(&mut s.show_reference, "Reference").on_hover_text(
                "R: a second, read-only view next to the map. It shows this ROM as it was opened, or another ROM (the original game, an earlier build): the model to work from.",
            );
        });
    }

    fn map_list(&mut self, ui: &mut Ui) {
        let Some(doc) = self.doc.as_mut() else { return };
        ui.add_space(4.0);
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search maps…").desired_width(f32::INFINITY));
        ui.add_space(2.0);

        // The rows: a group's name, then its maps
        let query = self.search.trim().to_lowercase();
        let mut rows: Vec<Result<usize, &str>> = Vec::new();
        let mut group = None;
        for (index, m) in doc.game.maps.iter().enumerate() {
            let matches = query.is_empty()
                || m.name.to_lowercase().contains(&query)
                || m.id.to_string().starts_with(&query)
                || m.section.as_ref().is_some_and(|s| s.to_lowercase().contains(&query));
            if !matches {
                continue;
            }
            if group != Some(m.id.group) {
                group = Some(m.id.group);
                rows.push(Err(m.group_name.as_str()));
            }
            rows.push(Ok(index));
        }
        if rows.is_empty() {
            ui.weak("No map by that name");
            return;
        }
        let current = doc.map.as_ref().map(|m| m.index);
        let mut open = None;
        let row_height = ui.spacing().interact_size.y;
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_height, rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in &rows[range] {
                match *row {
                    Err(group) => {
                        ui.allocate_ui(egui::vec2(ui.available_width(), row_height), |ui| ui.label(RichText::new(group).small().weak()));
                    }
                    Ok(index) => {
                        let m = &doc.game.maps[index];
                        let edited = doc.edited.contains(&m.layout.offset);
                        let mut label = RichText::new(format!("{}{}", m.name, if edited { "  •" } else { "" }));
                        if edited {
                            label = label.color(Color32::from_rgb(129, 199, 132));
                        }
                        let response = ui.add_sized(
                            [ui.available_width(), row_height],
                            egui::Button::selectable(current == Some(index), label)
                                .truncate()
                                .right_text(RichText::new(m.id.to_string()).small().weak()),
                        );
                        if response.clicked() {
                            open = Some(index);
                        }
                        response.on_hover_ui(|ui| {
                            ui.label(RichText::new(&m.name).strong());
                            if let Some(section) = m.section.as_ref().filter(|s| **s != m.name) {
                                ui.label(section);
                            }
                            ui.label(format!("Map {} · {}×{} tiles", m.id, m.layout.width, m.layout.height));
                            ui.label(format!("Tilesets 0x{:06X} + 0x{:06X}", m.layout.primary, m.layout.secondary));
                            let shared = doc.game.layout_users(m.layout.offset).len();
                            if shared > 1 {
                                ui.label(format!("Drawn from a layout {shared} maps share: they change together"));
                            }
                        });
                    }
                }
            }
        });
        if let Some(index) = open {
            doc.open_map(index);
        }
    }

    fn status(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let Some(doc) = self.doc.as_ref() else {
                ui.weak("No ROM open");
                return;
            };
            if let Some(notice) = &self.notice {
                let color = if notice.error { Color32::from_rgb(239, 154, 154) } else { ui.visuals().text_color() };
                ui.label(RichText::new(&notice.text).small().color(color));
                ui.separator();
            }
            let Some(map) = doc.map.as_ref() else { return };
            let over_reference =
                doc.reference.as_ref().filter(|_| self.settings.show_reference).and_then(|r| Some((r, r.map.as_ref()?, r.hover?)));
            let text = match doc.hover {
                _ if over_reference.is_some() => {
                    let Some((reference, shown, h)) = over_reference else { return };
                    let passable = if block::collision(h.block) == 0 { "passable" } else { "impassable" };
                    format!(
                        "Reference ({}, {})  ·  metatile {}  ·  {passable}  ·  elevation {}",
                        h.x,
                        h.y,
                        side::describe(&reference.game, shown.pair, block::metatile(h.block)),
                        block::elevation(h.block)
                    )
                }
                Some(h) => {
                    let id = block::metatile(h.block);
                    let passable = if block::collision(h.block) == 0 { "passable" } else { "impassable" };
                    format!(
                        "({}, {})  ·  metatile {}  ·  {passable}  ·  elevation {}",
                        h.x,
                        h.y,
                        side::describe(&doc.game, map.pair, id),
                        block::elevation(h.block)
                    )
                }
                None => {
                    let shared = doc.game.layout_users(map.layout.offset).len();
                    let shared = if shared > 1 { format!("  ·  layout shared by {shared} maps") } else { String::new() };
                    format!("{}  ·  map {}  ·  {}×{}  ·  {}{shared}", map.name, map.id, map.width, map.height, doc.game.name())
                }
            };
            ui.label(RichText::new(text).small());
        });
    }

    /// The reference, between the map and the side panel: by default this ROM as it was opened
    fn reference_pane(&mut self, ui: &mut Ui) {
        let Some(doc) = self.doc.as_mut().filter(|_| self.settings.show_reference) else { return };
        if doc.reference.is_none() {
            doc.reference = Reference::own(&doc.game, &doc.file_name);
        }
        if let (Some(reference), Some(map)) = (doc.reference.as_mut(), doc.map.as_ref()) {
            reference.follow_map(&doc.game, map.index);
        }
        // half of what the map has, to begin with
        let room = ui.available_width();
        let pane = egui::Panel::right("reference")
            .resizable(true)
            .default_size((room / 2.0).max(200.0))
            .size_range(160.0..=(room - 160.0).max(160.0));
        pane.frame(egui::Frame::new().fill(Color32::from_gray(17))).show(ui, |ui| {
            let header = egui::Frame::new().fill(ui.visuals().panel_fill).inner_margin(egui::Margin::symmetric(8, 0));
            let action = header.show(ui, |ui| reference::header(ui, doc, &self.files)).inner;
            if let Some(reference::Action::Own { originals }) = action {
                if !doc.reference.as_ref().is_some_and(|r| r.own) {
                    doc.reference = Reference::own(&doc.game, &doc.file_name);
                }
                if let (Some(reference), Some(map)) = (doc.reference.as_mut(), doc.map.as_ref()) {
                    reference.follow_map(&doc.game, map.index);
                    reference.set_originals(originals);
                }
            }
            map_view::show_reference(ui, doc, &mut self.settings);
        });
    }

    fn welcome(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.22).max(16.0));
            ui.heading("GBMapping");
            ui.label("A map and tileset editor for Game Boy Advance ROMs");
            ui.add_space(16.0);
            if ui.add(egui::Button::new(RichText::new("  Open a ROM…  ").size(18.0))).clicked() {
                self.files.open(Kind::Any, Purpose::Open);
            }
            ui.add_space(6.0);
            ui.label("or drop a .gba anywhere on this window");
            ui.add_space(20.0);
            ui.weak("Pokémon Ruby, Sapphire, Emerald, FireRed and LeafGreen: the original games,");
            ui.weak("builds of their decompilations, and hacks. The maps are found in the ROM itself.");
            ui.add_space(10.0);
            ui.weak("Your ROM stays on your computer: it's read and edited here, and never sent anywhere.");
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let Some(dialog) = &self.dialog else { return };
        let mut answer = None;
        egui::Modal::new(egui::Id::new("dialog")).show(ctx, |ui| {
            ui.set_max_width(460.0);
            match dialog {
                Dialog::Message { title, text } => {
                    ui.heading(title);
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| ui.label(text));
                    ui.add_space(10.0);
                    if ui.button("  OK  ").clicked() || ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Escape)) {
                        answer = Some(false);
                    }
                }
                Dialog::StoredEdits(edits) => {
                    ui.heading("Edits kept for this ROM");
                    ui.add_space(6.0);
                    ui.label(format!(
                        "The last time this ROM was open here, {} map(s) and {} tileset(s) had been changed. Those edits were kept.",
                        edits.maps.len(),
                        edits.tilesets.len()
                    ));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Apply them").clicked() {
                            answer = Some(true);
                        }
                        if ui.button("Discard them").on_hover_text("They're forgotten for good").clicked() {
                            answer = Some(false);
                        }
                    });
                }
            }
        });
        let Some(answer) = answer else { return };
        match (self.dialog.take(), self.doc.as_mut()) {
            (Some(Dialog::StoredEdits(edits)), Some(doc)) if answer => {
                let applied = doc.game.apply_edits(&edits);
                doc.commit();
                doc.unsaved_edits = false;
                self.say(format!("The kept edits are back: {} map(s), {} tileset(s)", applied.maps, applied.tilesets));
            }
            (Some(Dialog::StoredEdits(_)), Some(doc)) => {
                if let Some(storage) = frame.storage_mut() {
                    storage.remove_string(&storage_key(&doc.game));
                    storage.flush();
                }
            }
            _ => {}
        }
    }
}

/// Over the map when the reference is shown: what's being edited, in a strip as tall as
/// the reference's controls (see `Doc::strip`), so that the two maps sit tile facing tile.
/// Painted, not laid out: nothing but the strip's height moves what's under it.
fn map_header(ui: &mut Ui, doc: &Doc) {
    let Some(map) = doc.map.as_ref() else { return };
    let (strip, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), doc.strip), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(strip);
    painter.rect_filled(strip, 0.0, ui.visuals().panel_fill);
    let (row, font) = (ui.spacing().interact_size.y, egui::TextStyle::Body.resolve(ui.style()));
    let first = strip.left_top() + egui::vec2(8.0, 4.0 + row / 2.0);
    let title = painter.text(first, egui::Align2::LEFT_CENTER, "Editing", font.clone(), ui.visuals().strong_text_color());
    let name = format!("{}  ({})", map.name, map.id);
    painter.text(egui::pos2(title.right() + 8.0, first.y), egui::Align2::LEFT_CENTER, name, font.clone(), ui.visuals().text_color());
    let shared = doc.game.layout_users(map.layout.offset).len();
    let shared = if shared > 1 { format!("  ·  layout shared by {shared} maps") } else { String::new() };
    let second = first + egui::vec2(0.0, row + ui.spacing().item_spacing.y);
    let details = format!("{}×{} tiles{shared}", map.width, map.height);
    painter.text(second, egui::Align2::LEFT_CENTER, details, font, ui.visuals().weak_text_color());
}

/// Where a ROM's edits are kept: by game code, title and size (the key the first,
/// JavaScript GBMapping used, so that what it kept is found again)
fn storage_key(game: &Game) -> String {
    let header = game.rom.header();
    format!("GBMapping.{}.{}.{}", header.code, header.title, game.rom.len())
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.receive(frame);
        if self.dialog.is_none() {
            self.shortcuts(&ctx);
        }

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(4.0);
            self.toolbar(ui);
            ui.add_space(4.0);
        });
        egui::Panel::bottom("status").show(ui, |ui| self.status(ui));
        if self.doc.is_some() {
            egui::Panel::left("maps").resizable(true).default_size(230.0).size_range(150.0..=420.0).show(ui, |ui| self.map_list(ui));
            egui::Panel::right("side").resizable(true).default_size(292.0).size_range(292.0..=420.0).show(ui, |ui| {
                if let Some(doc) = self.doc.as_mut() {
                    side::show(ui, doc, &mut self.settings);
                }
            });
        }
        self.reference_pane(ui);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_gray(17))).show(ui, |ui| match self.doc.as_mut() {
            Some(doc) => {
                let beside = self.settings.show_reference && doc.reference.as_ref().is_some_and(|r| r.map.is_some());
                if beside {
                    map_header(ui, doc);
                }
                map_view::show(ui, doc, &mut self.settings);
                // the strip over the map grows or shrinks until both maps begin at the same height
                let off = doc.reference_top - doc.map_top;
                if beside && off.abs() > 0.1 {
                    doc.strip = (doc.strip + off).clamp(0.0, 200.0);
                    ui.ctx().request_repaint();
                }
            }
            None => self.welcome(ui),
        });

        // a file held over the window
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let rect = ctx.content_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("drop")));
            painter.rect_filled(rect, 0.0, Color32::from_black_alpha(160));
            painter.rect_stroke(rect.shrink(8.0), 6.0, egui::Stroke::new(3.0, Color32::from_rgb(76, 175, 80)), egui::StrokeKind::Inside);
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "Drop to open", egui::FontId::proportional(28.0), Color32::WHITE);
        }

        if let Some(text) = self.doc.as_mut().and_then(|d| d.notice.take()) {
            self.say(text);
        }
        self.dialogs(&ctx, frame);
        self.keep_edits(frame);
        self.update_title(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if self.settings != self.saved_settings {
            eframe::set_value(storage, SETTINGS_KEY, &self.settings);
            self.saved_settings = self.settings.clone();
        }
    }

    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(5)
    }
}
