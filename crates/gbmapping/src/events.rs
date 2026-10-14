//! The Events mode: the map's people, warps, triggers and signs shown over it, picked
//! and dragged there, and the panel (where the reference otherwise is) that lists them,
//! adds new ones and edits each. See `gbmapping_core::events`.

use crate::app::Settings;
use crate::doc::{texture, Doc};
use crate::side::WARNING;
use egui::{Color32, RichText, TextureHandle, Ui, Vec2};
use gbmapping_core::events::{
    Events, HiddenItem, List, Names, Object, ScriptView, Sign, Trigger, Warp, CLONE, DYNAMIC_MAP, HIDDEN_ITEM, MSGBOX_NPC, MSGBOX_SIGN,
    SECRET_BASE, SIGN_KINDS, TRAINER_TYPES,
};
use gbmapping_core::sprites::Sprites;
use gbmapping_core::{Family, MapId};
use std::collections::HashMap;
use std::rc::Rc;

/// The standard script a trigger's text is said with: a message box, nobody turning
const MSGBOX_DEFAULT: u8 = 4;

/// What the mode keeps from one frame of the screen to the next
#[derive(Default)]
pub struct EventsState {
    /// The events of the map on screen, as last read: (the map, its events)
    shown: Option<(usize, Events)>,
    /// The event worked on
    pub selected: Option<(List, usize)>,
    /// Where a new event goes: the tile last clicked where there's none
    pub add_at: Option<(i16, i16)>,
    /// An event being dragged on the map: it moved
    pub dragging: Option<bool>,
    /// The event picked when the pointer was pressed, if it was picked already: a click on
    /// it picks the next one on its tile
    pub pressed_on: Option<(List, usize)>,
    /// Written, not yet committed
    pending: bool,
    /// The text being written for the selected event's script: (the script it was read from, the text)
    said: Option<(u32, String)>,
    /// The search in the lists of maps and of sprites
    search: String,
    sprites: Option<Rc<Sprites>>,
    textures: HashMap<u16, Option<(TextureHandle, Vec2)>>,
}

impl EventsState {
    /// The ROM changed under the events (undo, another map): they're read again
    pub fn forget(&mut self) {
        self.shown = None;
        self.said = None;
        self.dragging = None;
    }
}

/// The colours the events are shown in, by list (a sign that's a hidden item has its own)
pub const OBJECT: Color32 = Color32::from_rgb(66, 165, 245);
pub const WARP: Color32 = Color32::from_rgb(239, 83, 80);
pub const TRIGGER: Color32 = Color32::from_rgb(102, 187, 106);
pub const SIGN: Color32 = Color32::from_rgb(255, 167, 38);
pub const ITEM: Color32 = Color32::from_rgb(171, 71, 188);

pub fn color(list: List, events: &Events, i: usize) -> Color32 {
    match list {
        List::Objects => OBJECT,
        List::Warps => WARP,
        List::Triggers => TRIGGER,
        List::Signs if events.signs.get(i).is_some_and(|s| s.kind == HIDDEN_ITEM) => ITEM,
        List::Signs => SIGN,
    }
}

/// The letter an event is marked with when it has no sprite
pub fn letter(list: List, events: &Events, i: usize) -> &'static str {
    match list {
        List::Objects => "P",
        List::Warps => "W",
        List::Triggers => "T",
        List::Signs if events.signs.get(i).is_some_and(|s| s.kind == HIDDEN_ITEM) => "I",
        List::Signs => "S",
    }
}

impl Doc {
    /// The events of the map on screen
    pub fn events(&mut self) -> Option<&Events> {
        let index = self.map.as_ref()?.index;
        if self.events.shown.as_ref().is_none_or(|(shown, _)| *shown != index) {
            self.events.shown = Some((index, self.game.events(index)));
            self.events.selected = None;
            self.events.add_at = None;
        }
        self.events.shown.as_ref().map(|(_, events)| events)
    }

    /// Writes the map's events, changed (to be committed once the pointer lets go)
    pub fn write_events(&mut self, events: Events) {
        let Some(index) = self.map.as_ref().map(|m| m.index) else { return };
        match self.game.set_events(index, &events) {
            Ok(()) => {
                self.events.shown = Some((index, events));
                self.events.pending = true;
            }
            Err(e) => self.notice = Some(e.to_string()),
        }
    }

    /// What was written is one step to undo, once nothing is held or typed
    pub fn commit_events(&mut self, busy: bool) {
        if self.events.pending && !busy {
            self.events.pending = false;
            if self.game.commit() {
                self.changed();
            }
        }
    }

    /// A sprite as a texture, and its size in pixels: none for a sprite the game chooses as it runs
    pub fn sprite(&mut self, ctx: &egui::Context, graphics: u16) -> Option<(egui::TextureId, Vec2)> {
        let names = Names::of(self.game.family);
        if Names::find(names.graphics, graphics).is_some_and(|name| name.starts_with("VAR_")) {
            return None;
        }
        let game = &self.game;
        let sprites = self.events.sprites.get_or_insert_with(|| Rc::new(game.sprites())).clone();
        let made = self.events.textures.entry(graphics).or_insert_with(|| {
            let image = game.sprite(&sprites, graphics)?;
            let size = Vec2::new(image.width as f32, image.height as f32);
            Some((texture(ctx, &format!("sprite {graphics}"), &image), size))
        });
        made.as_ref().map(|(texture, size)| (texture.id(), *size))
    }

    /// How many sprites the ROM has (found once)
    fn sprite_count(&mut self) -> usize {
        let game = &self.game;
        self.events.sprites.get_or_insert_with(|| Rc::new(game.sprites())).count()
    }
}

/// What an event is, in a few words
pub fn describe(doc: &Doc, events: &Events, list: List, i: usize) -> String {
    let names = Names::of(doc.game.family);
    match list {
        List::Objects => {
            let Some(o) = events.objects.get(i) else { return String::new() };
            let sprite = Names::find(names.graphics, o.graphics).map_or(format!("sprite {}", o.graphics), str::to_string);
            let clone = if o.kind == CLONE { " (a clone)" } else { "" };
            format!("Person {} · {sprite}{clone}", o.id)
        }
        List::Warps => {
            let Some(w) = events.warps.get(i) else { return String::new() };
            format!("Warp {i} to {}, its warp {}", map_name(doc, w.group, w.num), w.warp)
        }
        List::Triggers => {
            let Some(t) = events.triggers.get(i) else { return String::new() };
            match t.var {
                0 => format!("Trigger {i} · always"),
                var => format!("Trigger {i} · when 0x{var:04X} is {}", t.value),
            }
        }
        List::Signs => {
            let Some(s) = events.signs.get(i) else { return String::new() };
            match s.kind {
                HIDDEN_ITEM => {
                    let item = HiddenItem::of(doc.game.family, s.data).item;
                    format!("Hidden item · {}", Names::find(names.items, item).map_or(format!("item {item}"), str::to_string))
                }
                SECRET_BASE => format!("Secret base {}", s.data),
                kind => format!("Sign {i} · {}", SIGN_KINDS.get(kind as usize).copied().unwrap_or("?").to_lowercase()),
            }
        }
    }
}

/// A map's name by its group and number, or what the game makes of it
fn map_name(doc: &Doc, group: u8, num: u8) -> String {
    if (group, num) == DYNAMIC_MAP {
        return "the map the player came from".to_string();
    }
    let id = MapId { group, num };
    doc.game.map_index(id).map_or(format!("map {id} (none)"), |i| format!("{} ({id})", doc.game.maps[i].name))
}

/// The events at a tile of the map on screen, described: for the status line
pub fn describe_at(doc: &Doc, x: usize, y: usize) -> Option<String> {
    let (index, events) = doc.events.shown.as_ref()?;
    if Some(*index) != doc.map.as_ref().map(|m| m.index) {
        return None;
    }
    let here = events.at(x as i16, y as i16);
    let said: Vec<String> = here.iter().map(|&(list, i)| describe(doc, events, list, i)).collect();
    (!said.is_empty()).then(|| said.join("  ·  "))
}

// -- the panel ---------------------------------------------------------------------------------

pub fn panel(ui: &mut Ui, doc: &mut Doc, settings: &mut Settings) {
    let ctx = ui.ctx().clone();
    let Some(mut events) = doc.events().cloned() else {
        ui.weak("No map");
        return;
    };
    let Some(map) = doc.map.as_ref().map(|m| (m.name.clone(), m.id, m.width, m.height)) else { return };
    let (name, id, width, height) = map;
    let small = |text: String| RichText::new(text).small();
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Events").strong());
        ui.label(format!("{name}  ({id})"));
    });
    ui.label(
        small(format!(
            "{} people, {} warps, {} triggers, {} signs. Click one on the map to work on it, drag it to move it.",
            events.objects.len(),
            events.warps.len(),
            events.triggers.len(),
            events.signs.len()
        ))
        .weak(),
    );

    // -- adding --------------------------------------------------------------------------------
    let at = doc.events.add_at.unwrap_or(((width / 2) as i16, (height / 2) as i16));
    let mut added = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(small("Add".to_string()));
        let tip = format!("At ({}, {}): the tile last clicked where there's no event, else the middle of the map", at.0, at.1);
        if ui.button("Person").on_hover_text(&tip).clicked() {
            let graphics = events.objects.last().map_or(7, |o| o.graphics);
            let movement = Names::of(doc.game.family).movements.iter().find(|(_, n)| *n == "FACE_DOWN").map_or(8, |&(v, _)| v as u8);
            events.objects.push(Object {
                id: events.next_object_id(),
                graphics,
                x: at.0,
                y: at.1,
                elevation: 3,
                movement,
                ..Default::default()
            });
            added = Some((List::Objects, events.objects.len() - 1));
        }
        if ui.button("Warp").on_hover_text(&tip).clicked() {
            events.warps.push(Warp { x: at.0, y: at.1, elevation: 0, warp: 0, group: id.group, num: id.num });
            added = Some((List::Warps, events.warps.len() - 1));
        }
        if ui.button("Trigger").on_hover_text(&tip).clicked() {
            events.triggers.push(Trigger { x: at.0, y: at.1, elevation: 3, ..Default::default() });
            added = Some((List::Triggers, events.triggers.len() - 1));
        }
        if ui.button("Sign").on_hover_text(&tip).clicked() {
            events.signs.push(Sign { x: at.0, y: at.1, elevation: 0, kind: 0, data: 0 });
            added = Some((List::Signs, events.signs.len() - 1));
        }
        if ui.button("Hidden item").on_hover_text(&tip).clicked() {
            let family = doc.game.family;
            let item = HiddenItem { item: 13, flag: doc.game.next_hidden_item_flag(), quantity: 1, underfoot: false };
            events.signs.push(Sign { x: at.0, y: at.1, elevation: 0, kind: HIDDEN_ITEM, data: item.data(family) });
            added = Some((List::Signs, events.signs.len() - 1));
        }
    });
    if let Some(new) = added {
        doc.write_events(events.clone());
        doc.events.selected = Some(new);
        doc.events.said = None;
    }
    ui.separator();

    // -- the lists -----------------------------------------------------------------------------
    let list_height = (ui.available_height() * 0.38).max(120.0);
    egui::ScrollArea::vertical().id_salt("events list").max_height(list_height).auto_shrink([false, true]).show(ui, |ui| {
        for list in List::ALL {
            let count = events.len(list);
            ui.label(small(format!("{} ({count})", list.name())).weak());
            for i in 0..count {
                let (x, y) = events.place(list, i).unwrap_or_default();
                let text = RichText::new(format!("{}  ({x}, {y})", describe(doc, &events, list, i))).color(color(list, &events, i));
                if ui.selectable_label(doc.events.selected == Some((list, i)), text).clicked() {
                    doc.events.selected = Some((list, i));
                    doc.events.said = None;
                }
            }
        }
    });
    ui.separator();

    // -- the one selected ----------------------------------------------------------------------
    let Some((list, i)) = doc.events.selected.filter(|&(list, i)| i < events.len(list)) else {
        ui.weak("No event selected. Click one on the map or in the list, or add one.");
        return;
    };
    let before = events.clone();
    let mut action = None;
    egui::ScrollArea::vertical().id_salt("event").auto_shrink([false, false]).show(ui, |ui| {
        ui.label(RichText::new(describe(doc, &events, list, i)).strong().color(color(list, &events, i)));
        egui::Grid::new("event fields").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            let (x, y) = events.place(list, i).unwrap_or_default();
            let (mut nx, mut ny) = (x, y);
            ui.label("Place");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut nx).range(0..=width as i16 - 1).speed(0.1).prefix("x "));
                ui.add(egui::DragValue::new(&mut ny).range(0..=height as i16 - 1).speed(0.1).prefix("y "));
            });
            ui.end_row();
            if (nx, ny) != (x, y) {
                events.move_to(list, i, nx, ny);
            }
            match list {
                List::Objects => action = object_fields(ui, doc, &ctx, &mut events.objects[i]),
                List::Warps => action = warp_fields(ui, doc, &mut events.warps[i]),
                List::Triggers => trigger_fields(ui, &mut events.triggers[i]),
                List::Signs => sign_fields(ui, doc, &mut events.signs[i]),
            }
        });

        // what it runs
        let script = match list {
            List::Objects if events.objects[i].kind != CLONE => Some(events.objects[i].script),
            List::Triggers => Some(events.triggers[i].script),
            List::Signs => events.signs[i].script(),
            _ => None,
        };
        if let Some(script) = script {
            ui.add_space(6.0);
            let std = match list {
                List::Objects => MSGBOX_NPC,
                List::Signs => MSGBOX_SIGN,
                _ => MSGBOX_DEFAULT,
            };
            if let Some(new) = script_fields(ui, doc, script, std) {
                match list {
                    List::Objects => events.objects[i].script = new,
                    List::Triggers => events.triggers[i].script = new,
                    _ => events.signs[i].data = new,
                }
            }
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Duplicate").on_hover_text("A copy of it, a tile to its right").clicked() {
                action = Some(Action::Duplicate);
            }
            if ui.button("Delete").on_hover_text("Del").clicked() {
                action = Some(Action::Delete);
            }
        });
        if list == List::Objects {
            ui.label(small("Scripts name people by their number: one that moves people by number may need it.".into()).weak());
        }
    });
    let delete = ctx.input(|i| i.key_pressed(egui::Key::Delete)) && !ctx.egui_wants_keyboard_input();
    match action.or(delete.then_some(Action::Delete)) {
        Some(Action::Duplicate) => {
            if let Some(copy) = events.duplicate(list, i) {
                doc.events.selected = Some((list, copy));
            }
        }
        Some(Action::Delete) => {
            events.remove(list, i);
            doc.events.selected = None;
        }
        Some(Action::GoTo(index, warp)) => {
            settings.tool = crate::doc::Tool::Events;
            doc.open_map(index);
            if doc.events().is_some_and(|e| warp < e.warps.len()) {
                doc.events.selected = Some((List::Warps, warp));
            }
            return;
        }
        None => {}
    }
    if events != before {
        doc.write_events(events);
    }
    let busy = ctx.input(|i| i.pointer.any_down()) || ctx.egui_wants_keyboard_input();
    doc.commit_events(busy);
}

enum Action {
    Duplicate,
    Delete,
    /// To a map, and its warp
    GoTo(usize, usize),
}

/// A value from a list of names: the name shown, and the list to choose from
fn choose(ui: &mut Ui, id: &str, value: &mut u16, names: &[(u16, &'static str)], count: usize, search: &mut String) {
    let shown = Names::find(names, *value).map_or(format!("{value}"), |name| format!("{value} · {name}"));
    egui::ComboBox::from_id_salt(id).selected_text(shown).width(200.0).height(360.0).truncate().show_ui(ui, |ui| {
        ui.add(egui::TextEdit::singleline(search).hint_text("Search…").desired_width(180.0));
        let wanted = search.to_lowercase();
        for v in 0..count as u16 {
            let name = Names::find(names, v).unwrap_or("");
            if !wanted.is_empty() && !name.to_lowercase().contains(&wanted) && !v.to_string().starts_with(&wanted) {
                continue;
            }
            if ui.selectable_label(*value == v, format!("{v} · {name}")).clicked() {
                *value = v;
            }
        }
    });
}

fn object_fields(ui: &mut Ui, doc: &mut Doc, ctx: &egui::Context, o: &mut Object) -> Option<Action> {
    let names = Names::of(doc.game.family);
    let mut search = std::mem::take(&mut doc.events.search);
    ui.label("Number");
    ui.add(egui::DragValue::new(&mut o.id).range(0..=255).speed(0.1)).on_hover_text("Scripts name people by it");
    ui.end_row();
    if o.kind == CLONE {
        // a clone shows another map's object: its number, and the map (group, number)
        ui.label("Shows");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut o.elevation).range(0..=255).prefix("person "));
            let (mut num, mut group) = (o.trainer as u8, o.sight as u8);
            ui.add(egui::DragValue::new(&mut group).prefix("map ")).on_hover_text("The group of the map");
            ui.add(egui::DragValue::new(&mut num).prefix(".")).on_hover_text("The map's number in it");
            (o.trainer, o.sight) = (num as u16, group as u16);
        });
        ui.end_row();
        doc.events.search = search;
        return None;
    }
    ui.label("Sprite");
    ui.horizontal(|ui| {
        if let Some((texture, size)) = doc.sprite(ctx, o.graphics) {
            ui.add(egui::Image::new((texture, size)).fit_to_exact_size(size * 2.0));
        }
        let count = doc.sprite_count().max(names.graphics.len());
        choose(ui, "sprite", &mut o.graphics, names.graphics, count, &mut search);
    });
    ui.end_row();
    ui.label("Elevation");
    ui.add(egui::DragValue::new(&mut o.elevation).range(0..=15).speed(0.1));
    ui.end_row();
    ui.label("Movement");
    let mut movement = o.movement as u16;
    let count = names.movements.last().map_or(0x50, |&(v, _)| v as usize + 1);
    choose(ui, "movement", &mut movement, names.movements, count, &mut search);
    o.movement = movement as u8;
    ui.end_row();
    ui.label("Walks");
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut o.range[0]).range(0..=15).speed(0.1).prefix("across "))
            .on_hover_text("How far from its place it walks, left and right");
        ui.add(egui::DragValue::new(&mut o.range[1]).range(0..=15).speed(0.1).prefix("down "))
            .on_hover_text("How far from its place it walks, up and down");
    });
    ui.end_row();
    ui.label("Trainer");
    let mut kind = o.trainer as usize;
    egui::ComboBox::from_id_salt("trainer").selected_text(TRAINER_TYPES.get(kind).copied().unwrap_or("?")).show_ui(ui, |ui| {
        for (k, name) in TRAINER_TYPES.iter().enumerate() {
            ui.selectable_value(&mut kind, k, *name);
        }
    });
    o.trainer = kind as u16;
    ui.end_row();
    let tree = Names::find(names.movements, o.movement as u16).is_some_and(|n| n.contains("BERRY"));
    ui.label(if tree { "Berry tree" } else { "Sight" });
    ui.add(egui::DragValue::new(&mut o.sight).range(0..=255).speed(0.1)).on_hover_text(if tree {
        "Which of the game's berry trees it is"
    } else {
        "How many tiles a trainer sees"
    });
    ui.end_row();
    ui.label("Hidden by");
    ui.add(egui::DragValue::new(&mut o.flag).hexadecimal(4, false, true).speed(0.2))
        .on_hover_text("A flag: once it's set, this person isn't there. 0: always there.");
    ui.end_row();
    doc.events.search = search;
    None
}

fn warp_fields(ui: &mut Ui, doc: &mut Doc, w: &mut Warp) -> Option<Action> {
    let mut action = None;
    ui.label("Elevation");
    ui.add(egui::DragValue::new(&mut w.elevation).range(0..=15).speed(0.1)).on_hover_text("0: any");
    ui.end_row();
    ui.label("To");
    let mut search = std::mem::take(&mut doc.events.search);
    egui::ComboBox::from_id_salt("warp map").selected_text(map_name(doc, w.group, w.num)).width(220.0).height(360.0).truncate().show_ui(
        ui,
        |ui| {
            ui.add(egui::TextEdit::singleline(&mut search).hint_text("Search maps…").desired_width(200.0));
            let wanted = search.to_lowercase();
            if ui.selectable_label((w.group, w.num) == DYNAMIC_MAP, "The map the player came from").clicked() {
                (w.group, w.num) = DYNAMIC_MAP;
            }
            for m in doc
                .game
                .maps
                .iter()
                .filter(|m| wanted.is_empty() || m.name.to_lowercase().contains(&wanted) || m.id.to_string().starts_with(&wanted))
            {
                if ui.selectable_label((w.group, w.num) == (m.id.group, m.id.num), format!("{} · {}", m.id, m.name)).clicked() {
                    (w.group, w.num) = (m.id.group, m.id.num);
                }
            }
        },
    );
    doc.events.search = search;
    ui.end_row();
    ui.label("Its warp");
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut w.warp).range(0..=255).speed(0.1)).on_hover_text("The warp of that map the player arrives at");
        let there = doc.game.map_index(MapId { group: w.group, num: w.num });
        if let Some(index) = there {
            let warps = doc.game.events(index).warps;
            match warps.get(w.warp as usize) {
                Some(arrival) => ui.label(RichText::new(format!("at ({}, {})", arrival.x, arrival.y)).small().weak()),
                None => ui.label(RichText::new(format!("that map has {} warps", warps.len())).small().color(WARNING)),
            };
            if ui
                .button("Go there")
                .on_hover_text("Opens that map, its warp selected (double-click the warp on the map does too)")
                .clicked()
            {
                action = Some(Action::GoTo(index, w.warp as usize));
            }
        }
    });
    ui.end_row();
    action
}

fn trigger_fields(ui: &mut Ui, t: &mut Trigger) {
    ui.label("Elevation");
    ui.add(egui::DragValue::new(&mut t.elevation).range(0..=15).speed(0.1));
    ui.end_row();
    ui.label("When");
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut t.var).hexadecimal(4, false, true).speed(0.2).prefix("variable "))
            .on_hover_text("The game's variable (0x4000 and up) whose value it waits for. 0: it always runs.");
        ui.add(egui::DragValue::new(&mut t.value).speed(0.1).prefix("is "));
    });
    ui.end_row();
}

fn sign_fields(ui: &mut Ui, doc: &mut Doc, s: &mut Sign) {
    let family = doc.game.family;
    ui.label("Elevation");
    ui.add(egui::DragValue::new(&mut s.elevation).range(0..=15).speed(0.1));
    ui.end_row();
    ui.label("Kind");
    let kind_name = |kind: u8| match kind {
        HIDDEN_ITEM => "A hidden item".to_string(),
        SECRET_BASE => "A secret base".to_string(),
        k => SIGN_KINDS.get(k as usize).map_or(format!("Kind {k}"), |n| n.to_string()),
    };
    let mut kind = s.kind;
    egui::ComboBox::from_id_salt("sign kind").selected_text(kind_name(kind)).show_ui(ui, |ui| {
        for k in (0..SIGN_KINDS.len() as u8).chain([HIDDEN_ITEM, SECRET_BASE]) {
            ui.selectable_value(&mut kind, k, kind_name(k));
        }
    });
    // a sign that becomes a hidden item (or back) has nothing to keep of what it held
    if (kind >= HIDDEN_ITEM) != (s.kind >= HIDDEN_ITEM) || (kind == SECRET_BASE) != (s.kind == SECRET_BASE) {
        s.data = match kind {
            HIDDEN_ITEM => HiddenItem { item: 13, flag: doc.game.next_hidden_item_flag(), quantity: 1, underfoot: false }.data(family),
            _ => 0,
        };
    }
    s.kind = kind;
    ui.end_row();
    match s.kind {
        HIDDEN_ITEM => {
            let names = Names::of(family);
            let mut item = HiddenItem::of(family, s.data);
            let mut search = std::mem::take(&mut doc.events.search);
            ui.label("Item");
            let count = names.items.last().map_or(0x180, |&(v, _)| v as usize + 1);
            choose(ui, "item", &mut item.item, names.items, count, &mut search);
            doc.events.search = search;
            ui.end_row();
            ui.label("Found flag");
            ui.horizontal(|ui| {
                let most = if family == Family::FireRedLeafGreen { 0xFF } else { 0xFFFF };
                ui.add(egui::DragValue::new(&mut item.flag).range(0..=most).speed(0.1)).on_hover_text(
                    "Counted from the game's first hidden item flag: each hidden item needs its own, or finding one finds the other",
                );
                if ui.small_button("Next free").on_hover_text("The first that no hidden item of any map uses").clicked() {
                    item.flag = doc.game.next_hidden_item_flag();
                }
            });
            ui.end_row();
            if family == Family::FireRedLeafGreen {
                ui.label("How many");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut item.quantity).range(1..=127).speed(0.1));
                    ui.checkbox(&mut item.underfoot, "Found by stepping on it");
                });
                ui.end_row();
            }
            s.data = item.data(family);
        }
        SECRET_BASE => {
            ui.label("Base");
            ui.add(egui::DragValue::new(&mut s.data).speed(0.2)).on_hover_text("The secret base's number");
            ui.end_row();
        }
        _ => {}
    }
}

/// What a script does, and a text to make it say: returns the new script when one was written
fn script_fields(ui: &mut Ui, doc: &mut Doc, script: u32, std: u8) -> Option<u32> {
    let view = doc.game.script_view(script);
    let small = |text: String| RichText::new(text).small();
    ui.horizontal(|ui| {
        ui.label("Script");
        ui.label(small(if script == 0 { "none".into() } else { format!("0x{script:08X}") }).weak());
        if doc.game.made_script(script).is_some() {
            ui.label(small("written here".into()).weak());
        }
    });
    let (current, only) = match &view {
        ScriptView::Says(says) => (says.text.clone(), says.only),
        ScriptView::None => (String::new(), true),
        ScriptView::Battle { trainer, intro } => {
            ui.label(small(format!("A battle with trainer {trainer}. Before it: “{}”", intro.as_deref().unwrap_or("?"))));
            return None;
        }
        ScriptView::Other => {
            ui.label(small("It does more than say something: the editor doesn't change it. A text written here replaces it with a script that only says it.".into()).color(WARNING));
            (String::new(), false)
        }
    };
    if doc.events.said.as_ref().is_none_or(|(of, _)| *of != script) {
        doc.events.said = Some((script, current.clone()));
    }
    let (_, said) = doc.events.said.as_mut()?;
    let label = match view {
        ScriptView::Says(_) if only => "Says",
        ScriptView::Says(_) => "Says (and does more)",
        _ => "Make it say",
    };
    ui.label(label);
    ui.add(
        egui::TextEdit::multiline(said)
            .desired_rows(4)
            .desired_width(f32::INFINITY)
            .font(egui::TextStyle::Monospace)
            .hint_text("What it says…"),
    );
    ui.label(
        small("A new line goes to the box's next line; an empty line begins a new box. {PLAYER}, {RIVAL}: names the game fills in.".into())
            .weak(),
    );
    let said = said.clone();
    let changed = said != current && !said.trim().is_empty();
    let mut written = None;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(changed, egui::Button::new("Write the text"))
            .on_hover_text("A new script that says it, at the ROM's free end: others that ran the old one keep it")
            .clicked()
        {
            let text = said.clone();
            match doc.game.new_message_script(&text, std) {
                Ok(new) => {
                    doc.events.said = Some((new, text));
                    written = Some(new);
                }
                Err(e) => doc.notice = Some(e.to_string()),
            }
        }
        if changed && ui.small_button("Undo the text").clicked() {
            doc.events.said = Some((script, current.clone()));
        }
    });
    written
}
