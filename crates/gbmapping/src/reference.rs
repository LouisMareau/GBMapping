//! The reference: a second, read-only view next to the map being edited. It shows the
//! map's original when the ROM keeps one (the map as it was before the hack redrew it,
//! on the tilesets it came with), else the same map as it was when the ROM was opened,
//! or the map it comes from in another ROM (the original game, an earlier build). Always
//! with the tilesets that go with what's shown. The work to redo is on one side, the
//! model on the other.

use crate::doc::{Doc, Hover, Sheet};
use crate::files::{Files, Kind, Purpose};
use egui::{RichText, Ui};
use gbmapping_core::{Game, Map, OpenError, Pair, Profile};
use std::collections::HashMap;

pub struct Reference {
    pub game: Game,
    /// The other ROM's file name
    pub label: String,
    /// It's the edited ROM itself, as it was opened
    pub own: bool,
    /// Its maps are shown as they were before the hack redrew them, where the ROM keeps
    /// that (see `MapInfo::original`): on their original tilesets
    pub originals: bool,
    /// Its map keeps to the edited one
    pub follow: bool,
    pub map: Option<Map>,
    /// The edited map its own was last matched to
    followed: Option<usize>,
    sheets: HashMap<Pair, Sheet>,
    pub hover: Option<Hover>,
    /// A right-drag on it: the rectangle being picked
    pub picking: Option<((usize, usize), (usize, usize))>,
    filter: String,
}

/// What the header's controls ask for
pub enum Action {
    /// This ROM as the reference: its maps' originals, or its maps as they were opened
    Own { originals: bool },
}

impl Reference {
    fn new(game: Game, label: String, own: bool) -> Self {
        Self {
            originals: game.maps.iter().any(|m| m.original.is_some()),
            game,
            label,
            own,
            follow: true,
            map: None,
            followed: None,
            sheets: HashMap::new(),
            hover: None,
            picking: None,
            filter: String::new(),
        }
    }

    /// The edited ROM as it was opened: every map before the edits, and before the hack
    /// redrew it when the ROM keeps the maps' originals
    pub fn own(edited: &Game, file_name: &str) -> Option<Self> {
        let game = Game::open(edited.source().to_vec(), file_name, edited.profile.clone()).ok()?;
        Some(Self::new(game, file_name.to_string(), true))
    }

    /// Another ROM. No profile is its own: its maps go by the names it can tell.
    pub fn other(bytes: Vec<u8>, name: String) -> Result<Self, OpenError> {
        Ok(Self::new(Game::open(bytes, &name, Profile::default())?, name, false))
    }

    /// Shows the map that goes with the edited one, when it's following
    pub fn follow_map(&mut self, edited: &Game, index: usize) {
        if !self.follow || self.followed == Some(index) {
            return;
        }
        self.followed = Some(index);
        // the same ROM has the same maps in the same order
        let counterpart = if self.own { Some(index) } else { self.game.counterpart(edited, index) };
        self.show(counterpart);
    }

    /// Shows one of its maps: its original when that's what's wanted and the ROM keeps
    /// one, else the map as it is in the ROM. Either way with the layout's own tilesets.
    fn show(&mut self, index: Option<usize>) {
        let original = |i: usize| if self.originals { self.game.load_original(i) } else { None };
        self.map = index.and_then(|i| original(i).or_else(|| self.game.load_map(i)));
        self.hover = None;
        self.picking = None;
    }

    /// Originals or not: the map shown stays, in its other state
    pub fn set_originals(&mut self, originals: bool) {
        self.originals = originals;
        self.show(self.map.as_ref().map(|m| m.index));
    }

    /// Is what's shown a map's original (and not the map as it is in the ROM)?
    pub fn shows_original(&self) -> bool {
        self.map.as_ref().is_some_and(|m| self.game.maps.get(m.index).is_some_and(|info| info.layout.offset != m.layout.offset))
    }

    /// Matches its map again (after the ROM being edited changed)
    pub fn rematch(&mut self) {
        self.followed = None;
    }

    /// Shows a map of its own choosing, and stays there
    fn open_map(&mut self, index: usize) {
        self.follow = false;
        self.show(Some(index));
    }

    pub fn sheet(&mut self, ctx: &egui::Context, pair: Pair) -> &Sheet {
        let game = &self.game;
        self.sheets.entry(pair).or_insert_with(|| Sheet::new(ctx, game, pair))
    }

    /// A pair's sheet, if it has been drawn
    pub fn drawn(&self, pair: Pair) -> Option<&Sheet> {
        self.sheets.get(&pair)
    }

    /// What it is, in a few words
    pub fn source(&self) -> String {
        match (self.own, self.originals) {
            (true, true) => OWN_ORIGINALS.to_string(),
            (true, false) => OWN_OPENED.to_string(),
            _ => self.label.clone(),
        }
    }
}

const OWN_ORIGINALS: &str = "This ROM: the maps' originals";
const OWN_OPENED: &str = "This ROM, as it was opened";

/// The reference pane's two lines of controls: where it comes from, which map it shows
pub fn header(ui: &mut Ui, doc: &mut Doc, files: &Files) -> Option<Action> {
    let keeps_originals = doc.game.maps.iter().any(|m| m.original.is_some());
    let reference = doc.reference.as_mut()?;
    let mut action = None;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Reference").strong()).on_hover_text(
            "Read-only: what the map looked like, to work from. Scrolls and zooms with the map.\nRight-click or right-drag it to take what's there as the stamp.",
        );
        egui::ComboBox::from_id_salt("reference source").selected_text(reference.source()).width(ui.available_width() - 8.0).truncate().show_ui(ui, |ui| {
            if keeps_originals
                && ui
                    .selectable_label(reference.own && reference.originals, OWN_ORIGINALS)
                    .on_hover_text("Each map as it was before it was redrawn with this game's tilesets, on the tilesets it came with. A map without an original is shown as it was opened.")
                    .clicked()
            {
                action = Some(Action::Own { originals: true });
            }
            if ui.selectable_label(reference.own && !reference.originals, OWN_OPENED).on_hover_text("Every map before your edits").clicked() {
                action = Some(Action::Own { originals: false });
            }
            if !reference.own {
                let _ = ui.selectable_label(true, &reference.label);
            }
            if ui
                .selectable_label(false, "Another ROM…")
                .on_hover_text("The original game, or an earlier build: its maps are shown with its own tilesets, next to yours.\nA Game Boy Color game (.gbc) can be the reference of an Advance one: its blocks are shown as the four tiles they are.")
                .clicked()
            {
                files.open(Kind::Rom, Purpose::Reference);
            }
        });
    });
    ui.horizontal(|ui| {
        let shown = match &reference.map {
            Some(map) if reference.shows_original() => format!("{}  ({})  ·  original", map.name, map.id),
            Some(map) if reference.own && reference.originals => format!("{}  ({})  ·  no original: as opened", map.name, map.id),
            Some(map) => format!("{}  ({})", map.name, map.id),
            None => "No map like this one: choose…".to_string(),
        };
        if ui
            .checkbox(&mut reference.follow, "Follow")
            .on_hover_text("Shows the map that goes with the one being edited: the same name, or the same place and size")
            .changed()
            && reference.follow
        {
            reference.rematch();
        }
        egui::ComboBox::from_id_salt("reference map")
            .selected_text(shown)
            .width(ui.available_width() - 8.0)
            .height(360.0)
            .truncate()
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show_ui(ui, |ui| {
                ui.add(egui::TextEdit::singleline(&mut reference.filter).hint_text("Search maps…").desired_width(f32::INFINITY));
                let query = reference.filter.trim().to_lowercase();
                let found: Vec<usize> = (0..reference.game.maps.len())
                    .filter(|&i| {
                        let m = &reference.game.maps[i];
                        query.is_empty()
                            || m.name.to_lowercase().contains(&query)
                            || m.id.to_string().starts_with(&query)
                            || m.section.as_ref().is_some_and(|s| s.to_lowercase().contains(&query))
                    })
                    .collect();
                let current = reference.map.as_ref().map(|m| m.index);
                let mut chosen = None;
                let row_height = ui.spacing().interact_size.y;
                egui::ScrollArea::vertical().max_height(300.0).show_rows(ui, row_height, found.len(), |ui, range| {
                    for &i in &found[range] {
                        let m = &reference.game.maps[i];
                        let label = format!("{}  ({})  {}×{}", m.name, m.id, m.layout.width, m.layout.height);
                        if ui.selectable_label(current == Some(i), label).clicked() {
                            chosen = Some(i);
                        }
                    }
                });
                if let Some(i) = chosen {
                    reference.open_map(i);
                    ui.close();
                }
            });
    });
    ui.add_space(2.0);
    action
}
