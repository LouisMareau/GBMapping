//! The side panel: the metatile picker (and the map's border), and the tileset tab
//! where a metatile's tiles and attributes are edited.

use crate::app::{Settings, Tab};
use crate::doc::{texture, Doc, LayerTextures, PICKER_COLUMNS, TILE_COLUMNS};
use crate::map_view::{scale_for, snap};
use egui::{pos2, vec2, Align, Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
use gbmapping_core::format::{block, AttrField};
use gbmapping_core::gbc::block_of;
use gbmapping_core::gfx::{TileEntry, METATILE, TILE};
use gbmapping_core::tiles::{Borrowed, Colors};
use gbmapping_core::{Game, Pair};

const ACCENT: Color32 = Color32::from_rgb(76, 175, 80);
const GROUND: Color32 = Color32::from_gray(17);
const WARNING: Color32 = Color32::from_rgb(255, 202, 40);
const WHOLE: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));

pub fn show(ui: &mut Ui, doc: &mut Doc, settings: &mut Settings) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut settings.tab, Tab::Metatiles, "Metatiles").on_hover_text("Pick what to paint");
        ui.selectable_value(&mut settings.tab, Tab::Tileset, "Tileset")
            .on_hover_text("Edit the selected metatile: its tiles, its behavior");
    });
    ui.separator();
    let Some(pair) = doc.map.as_ref().map(|m| m.pair) else { return };
    match settings.tab {
        Tab::Metatiles => picker(ui, doc, pair),
        Tab::Tileset => tileset(ui, doc, settings, pair),
    }
}

const QUARTERS: [&str; 4] = ["top left", "top right", "bottom left", "bottom right"];

/// "0x00D TALL_GRASS"; in a game of blocks, "block 0x1A, top left: TALL_GRASS"
pub fn describe(game: &Game, pair: Pair, id: u16) -> String {
    if game.locate(pair, id).is_none() {
        return format!("0x{id:03X} (nothing)");
    }
    let behavior = game.behavior(pair, id);
    if game.format.block_size() > 1 {
        let (block, qx, qy) = block_of(id);
        let collision = game.format.behavior_name(behavior).map_or_else(|| format!("collision 0x{behavior:02X}"), str::to_string);
        return format!("block 0x{block:02X}, {}: {collision}", QUARTERS[qy * 2 + qx]);
    }
    match game.format.behavior_name(behavior) {
        Some(name) => format!("0x{id:03X} {name}"),
        None => format!("0x{id:03X} behavior 0x{behavior:02X}"),
    }
}

/// What a map has on a tile, for the status line
pub fn describe_tile(game: &Game, pair: Pair, b: u16) -> String {
    let what = describe(game, pair, block::metatile(b));
    // a map of blocks has no collision or elevation of its own: its tilesets hold them
    if game.format.block_size() > 1 {
        return what;
    }
    let passable = if block::collision(b) == 0 { "passable" } else { "impassable" };
    format!("metatile {what}  ·  {passable}  ·  elevation {}", block::elevation(b))
}

fn picker(ui: &mut Ui, doc: &mut Doc, pair: Pair) {
    let ctx = ui.ctx().clone();
    let ppp = ui.pixels_per_point();
    let cell = METATILE as f32 * scale_for(2, ppp);
    let (sheet_texture, sheet_count, rows) = {
        let sheet = doc.sheet(&ctx, pair);
        (sheet.texture.id(), sheet.count, sheet.rows)
    };

    let shown = doc.picker_hover.unwrap_or_else(|| doc.stamp.first());
    let size = if doc.stamp.width * doc.stamp.height > 1 {
        format!("  ·  {}×{} selected", doc.stamp.width, doc.stamp.height)
    } else {
        String::new()
    };
    ui.label(egui::RichText::new(format!("{}{size}", describe(&doc.game, pair, shown))).small());

    // the border: the tiles the game repeats around the map
    if let Some((bw, bh, border)) = doc.map.as_ref().map(|m| (m.border_width, m.border_height, m.border.clone())) {
        ui.horizontal(|ui| {
            let (response, painter) = ui.allocate_painter(vec2(bw as f32, bh as f32) * cell, Sense::click());
            let origin = snap(response.rect.min, ppp);
            painter.rect_filled(response.rect, 0.0, GROUND);
            let mut mesh = Mesh::with_texture(sheet_texture);
            for (i, &b) in border.iter().enumerate() {
                let id = block::metatile(b) as usize;
                if id < sheet_count {
                    let r = Rect::from_min_size(origin + vec2((i % bw) as f32, (i / bw) as f32) * cell, Vec2::splat(cell));
                    mesh.add_rect_with_uv(r, crate::doc::sheet_uv(id, rows), Color32::WHITE);
                }
            }
            painter.add(Shape::mesh(mesh));
            if let Some(p) = response.interact_pointer_pos().filter(|_| response.clicked()) {
                doc.paint_border(((p.x - origin.x) / cell) as usize, ((p.y - origin.y) / cell) as usize);
            }
            let how = if doc.grain() > 1 { "Click it to set it from the selection." } else { "Click a tile to set it from the selection." };
            ui.label(egui::RichText::new(format!("Border: what's drawn around the map.\n{how}")).small().weak());
        });
    }
    ui.separator();

    egui::ScrollArea::vertical().id_salt("metatiles").auto_shrink(false).show(ui, |ui| {
        let size = vec2(PICKER_COLUMNS as f32, rows as f32) * cell;
        let (response, painter) = ui.allocate_painter(size, Sense::click_and_drag());
        let origin = snap(response.rect.min, ppp);
        let rect = Rect::from_min_size(origin, size);
        painter.rect_filled(rect, 0.0, GROUND);
        painter.image(sheet_texture, rect, WHOLE, Color32::WHITE);
        let cell_at = |p: Pos2| {
            (
                (((p.x - origin.x) / cell).floor().max(0.0) as usize).min(PICKER_COLUMNS - 1),
                (((p.y - origin.y) / cell).floor().max(0.0) as usize).min(rows.max(1) - 1),
            )
        };

        let (pointer, primary) = ui.input(|i| (i.pointer.latest_pos(), i.pointer.primary_down()));
        match pointer {
            Some(p) if response.is_pointer_button_down_on() && primary => {
                let at = cell_at(p);
                let start = *doc.picker_drag.get_or_insert(at);
                doc.pick_metatiles(start, at);
            }
            _ => doc.picker_drag = None,
        }
        doc.picker_hover = response.hover_pos().map(cell_at).map(|c| (c.1 * PICKER_COLUMNS + c.0) as u16);

        if let Some((x, y)) = doc.stamp.picker {
            let selected =
                Rect::from_min_size(origin + vec2(x as f32, y as f32) * cell, vec2(doc.stamp.width as f32, doc.stamp.height as f32) * cell);
            painter.rect_stroke(selected, 0.0, Stroke::new(2.0, ACCENT), StrokeKind::Inside);
            if doc.scroll_picker {
                ui.scroll_to_rect(selected.expand(cell), Some(Align::Center));
            }
        }
        doc.scroll_picker = false;
    });
}

fn tileset(ui: &mut Ui, doc: &mut Doc, settings: &mut Settings, pair: Pair) {
    let ctx = ui.ctx().clone();
    let ppp = ui.pixels_per_point();
    let id = doc.stamp.first();
    let format = doc.game.format;
    let Some((tileset, local)) = doc.game.locate(pair, id) else {
        ui.label(format!("Metatile 0x{id:03X} holds nothing in these tilesets."));
        ui.weak("Pick another one in the Metatiles tab.");
        return;
    };
    // a Game Boy Color game: the metatile is a quarter of a block, whose tiles alone are
    // set (a tile's palette goes with the tile, and there's one layer)
    let blocks = format.block_size() > 1;
    let in_primary = (id as usize) < format.primary_metatiles && !blocks;
    if blocks {
        let (block, qx, qy) = block_of(id);
        ui.label(egui::RichText::new(format!("Block 0x{block:02X}")).strong());
        let quarter = QUARTERS[qy * 2 + qx];
        ui.label(egui::RichText::new(format!("its {quarter} quarter: a block is 4×4 tiles, edited a quarter at a time")).small().weak());
        doc.entry %= 4;
    } else {
        ui.label(egui::RichText::new(format!("Metatile 0x{id:03X}")).strong());
        let which = if in_primary { "primary" } else { "secondary" };
        ui.label(egui::RichText::new(format!("number {local} of the {which} tileset at 0x{:06X}", tileset.offset)).small().weak());
    }
    // what changing it changes: the maps it's on (a script may put it on others as the game runs)
    let on_maps = match doc.metatile_uses(tileset.offset, local) {
        0 => "On no map".to_string(),
        n => format!("On the maps {n} times: they change with it"),
    };
    ui.label(egui::RichText::new(on_maps).small().weak());
    // a metatile to build without changing any map: a new one, or the copy of this one
    let mut add: Option<Option<u16>> = None;
    if !blocks {
        ui.horizontal(|ui| {
            let new = ui.small_button("New").on_hover_text(
                "A new, empty metatile at the end of this map's secondary tileset: nothing in the game uses it yet.",
            );
            let duplicate = ui.small_button("Duplicate").on_hover_text(
                "A copy of this metatile, as a new one of this map's secondary tileset.\nEdit the copy: the maps the original is on stay as they are.",
            );
            if new.clicked() || duplicate.clicked() {
                add = Some(duplicate.clicked().then_some(id));
            }
            let next = ui.small_button("Next free").on_hover_text(
                "The next metatile of the secondary tileset that's on no map, to rebuild.\nA script may still put it on a map as the game runs: a new one is used by nothing.",
            );
            if next.clicked() {
                match doc.next_free_metatile(pair, id) {
                    Some(free) => doc.select_metatile(free),
                    None => doc.notice = Some("Every metatile of this map's secondary tileset is on a map".to_string()),
                }
            }
        });
    }
    ui.add_space(4.0);

    let mut entries = doc.game.entries(pair, id);
    let mut attribute = doc.game.attribute(pair, id);
    let before = (entries, attribute);

    // -- the metatile, and its two layers: click a quarter to edit its tile -------------
    let (sheet_texture, sheet_rows) = {
        let sheet = doc.sheet(&ctx, pair);
        (sheet.texture.id(), sheet.rows)
    };
    if doc.layers.as_ref().is_none_or(|l| (l.pair, l.entries) != (pair, entries)) {
        let layer = |l: usize| texture(&ctx, "layer", &doc.game.render_layer(pair, &entries, l));
        doc.layers = Some(LayerTextures { pair, entries, textures: [layer(0), layer(1)] });
    }
    let layer_textures = doc.layers.as_ref().map(|l| [l.textures[0].id(), l.textures[1].id()]).unwrap_or_default();
    let big = METATILE as f32 * scale_for(4, ppp);
    let layers: &[&str] = if blocks { &["Tiles"] } else { &["Bottom", "Top"] };
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(if blocks { "Quarter" } else { "Both" }).small());
            let (response, painter) = ui.allocate_painter(Vec2::splat(big), Sense::hover());
            let rect = Rect::from_min_size(snap(response.rect.min, ppp), Vec2::splat(big));
            painter.image(sheet_texture, rect, crate::doc::sheet_uv(id as usize, sheet_rows), Color32::WHITE);
        });
        for (layer, &name) in layers.iter().enumerate() {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(name).small());
                let (response, painter) = ui.allocate_painter(Vec2::splat(big), Sense::click());
                let rect = Rect::from_min_size(snap(response.rect.min, ppp), Vec2::splat(big));
                painter.rect_filled(rect, 0.0, GROUND);
                painter.image(layer_textures[layer], rect, WHOLE, Color32::WHITE);
                painter.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_gray(70)), StrokeKind::Outside);
                if let Some(p) = response.interact_pointer_pos().filter(|_| response.clicked()) {
                    let quarter = ((p.x - rect.min.x) / (big / 2.0)).clamp(0.0, 1.0) as usize
                        + ((p.y - rect.min.y) / (big / 2.0)).clamp(0.0, 1.0) as usize * 2;
                    doc.entry = layer * 4 + quarter;
                }
                if doc.entry / 4 == layer {
                    let q = doc.entry % 4;
                    let r = Rect::from_min_size(rect.min + vec2((q % 2) as f32, (q / 2) as f32) * (big / 2.0), Vec2::splat(big / 2.0));
                    painter.rect_stroke(r, 0.0, Stroke::new(2.0, ACCENT), StrokeKind::Inside);
                }
            });
        }
    });

    // -- the selected quarter's tile -----------------------------------------------------
    let mut entry = TileEntry::unpack(entries[doc.entry]);
    let quarter = QUARTERS[doc.entry % 4];
    let which = match (blocks, doc.entry < 4) {
        (true, _) => format!("The {quarter} tile:"),
        (false, true) => format!("Bottom layer, {quarter}:"),
        (false, false) => format!("Top layer, {quarter}:"),
    };
    ui.label(egui::RichText::new(which).small());
    ui.horizontal(|ui| {
        ui.label("Tile");
        let most = if blocks { 0xFF } else { format.total_tiles as u16 - 1 };
        ui.add(egui::DragValue::new(&mut entry.tile).range(0..=most).speed(0.25));
        ui.label("Palette");
        ui.add_enabled(!blocks, egui::DragValue::new(&mut entry.palette).range(0..=15).speed(0.1))
            .on_disabled_hover_text("In this game a tile's palette goes with the tile, whatever block it's in");
    });
    if !blocks {
        ui.horizontal(|ui| {
            ui.checkbox(&mut entry.hflip, "Flip ↔");
            ui.checkbox(&mut entry.vflip, "Flip ↕");
        });
    }

    // -- the metatile's attributes ---------------------------------------------------------
    ui.separator();
    let mut behavior = format.behavior.get(attribute);
    ui.horizontal(|ui| {
        ui.label(format.behavior.name);
        ui.add(egui::DragValue::new(&mut behavior).range(0..=format.behavior.max()).hexadecimal(2, false, true).speed(0.1));
        if !format.behaviors.is_empty() {
            let name = format.behavior_name(behavior).unwrap_or("(unnamed)");
            egui::ComboBox::from_id_salt("behavior").selected_text(name).width(150.0).show_ui(ui, |ui| {
                for &(value, name) in format.behaviors {
                    ui.selectable_value(&mut behavior, value as u32, format!("{value:02X}  {name}"));
                }
            });
        }
    });
    attribute = format.behavior.set(attribute, behavior);
    for field in std::iter::once(&format.layer_type).chain(format.extra_fields).filter(|_| !blocks) {
        attribute = field_editor(ui, field, attribute);
    }

    // -- the tiles: click one to put it in the selected quarter -----------------------------
    ui.separator();
    ui.horizontal(|ui| {
        // one tileset, whose tiles have their colours: nothing to choose
        if blocks {
            return;
        }
        ui.checkbox(&mut settings.usual_colors, "Usual colours").on_hover_text(
            "A tile has no colours of its own: each metatile draws it in one of the palettes.\nOn: each tile in the palette it's used with the most, and a click takes the tile with that palette.\nOff: every tile in the selected quarter's palette, to try a tile in another one.",
        );
        ui.checkbox(&mut settings.all_tilesets, "All tilesets").on_hover_text(
            "Lists the tiles of every tileset, not only this map's two.\nA metatile can only be made of its own tilesets' tiles: a click on a tile of another tileset copies it into this map's secondary tileset, with its colours.",
        );
    });
    // said here, where the tiles are: a click that does nothing needs its reason in sight
    if in_primary {
        let first = format.primary_metatiles;
        let why = format!(
            "Metatile 0x{id:03X} is the primary tileset's, on every map that has it: it takes that tileset's tiles only. Duplicate copies it into the secondary tileset (0x{first:03X} and up), where it takes any tile."
        );
        ui.label(egui::RichText::new(why).small().color(WARNING));
    }
    let hint = if doc.tile_hint.is_empty() { "Tiles: click one to put it in the selected quarter" } else { doc.tile_hint.as_str() };
    ui.add(egui::Label::new(egui::RichText::new(hint).small()).truncate());
    let forced = (!settings.usual_colors && !blocks).then_some(entry.palette);
    let sections = doc.tile_sections(pair, settings.all_tilesets && !blocks);
    let cell = TILE as f32 * scale_for(2, ppp);
    let (mut hovered, mut borrow) = (String::new(), None);
    // another metatile or another quarter: the list goes to the quarter's tile
    let shown = (pair, id, doc.entry);
    let find = doc.tiles_shown.replace(shown) != Some(shown);
    egui::ScrollArea::vertical().id_salt("tiles").auto_shrink(false).show(ui, |ui| {
        for section in sections.iter() {
            // a metatile of the primary tileset is the same on every map: it can't count on
            // this map's secondary tiles, or on another tileset's
            let refused = in_primary && section.base != Some(0);
            match refused {
                true => ui.label(egui::RichText::new(format!("{} · not for a primary metatile", section.label)).small().color(WARNING)),
                false => ui.label(egui::RichText::new(&section.label).small().weak()),
            };
            let rows = section.count.div_ceil(TILE_COLUMNS).max(1);
            let size = vec2(TILE_COLUMNS as f32, rows as f32) * cell;
            let (response, painter) = ui.allocate_painter(size, Sense::click());
            let origin = snap(response.rect.min, ppp);
            let place = |t: usize| {
                Rect::from_min_size(origin + vec2((t % TILE_COLUMNS) as f32, (t / TILE_COLUMNS) as f32) * cell, Vec2::splat(cell))
            };
            let selected = section.base.and_then(|base| (entry.tile as usize).checked_sub(base as usize)).filter(|&t| t < section.count);
            if let Some(t) = selected.filter(|_| find) {
                ui.scroll_to_rect(place(t).expand(cell * 4.0), Some(Align::Center));
            }
            // the tilesets out of sight aren't drawn: there can be dozens
            if !ui.is_rect_visible(response.rect) {
                continue;
            }
            let sheet = doc.tile_sheet(&ctx, section.pair, section.secondary, forced);
            let rect = Rect::from_min_size(origin, size);
            painter.rect_filled(rect, 0.0, GROUND);
            // of another tileset, the rows down to its last tile that draws something
            let drawn = rows as f32 / sheet.palettes.len().div_ceil(TILE_COLUMNS).max(1) as f32;
            let tint = if refused { Color32::from_white_alpha(70) } else { Color32::WHITE };
            painter.image(sheet.texture.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, drawn.min(1.0))), tint);
            let tile_at = |p: Pos2| {
                ((p.y - origin.y) / cell).max(0.0) as usize * TILE_COLUMNS
                    + (((p.x - origin.x) / cell).max(0.0) as usize).min(TILE_COLUMNS - 1)
            };
            let palette_of = |t: usize| sheet.palettes.get(t).copied().unwrap_or(0);
            if let Some(t) = response.hover_pos().map(tile_at).filter(|&t| t < section.count) {
                hovered = match section.base {
                    _ if refused => "Not for a primary metatile: Duplicate it, and edit the copy".to_string(),
                    Some(base) => format!("Tile 0x{:03X} (palette {})", base as usize + t, palette_of(t)),
                    None => format!("Tile {t} there (palette {}): a click copies it into this map's tileset", palette_of(t)),
                };
                if refused {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
                } else {
                    painter.rect_stroke(place(t), 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Outside);
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            }
            if let Some(t) = response.interact_pointer_pos().filter(|_| response.clicked()).map(tile_at).filter(|&t| t < section.count) {
                match section.base {
                    _ if refused => doc.notice = Some(primary_only(id)),
                    Some(base) => {
                        entry.tile = base + t as u16;
                        if settings.usual_colors || blocks {
                            entry.palette = palette_of(t);
                        }
                    }
                    None => borrow = Some((section.pair, section.secondary, t, palette_of(t))),
                }
            }
            if let Some(t) = selected {
                painter.rect_stroke(place(t), 0.0, Stroke::new(1.5, ACCENT), StrokeKind::Outside);
            }
        }
    });
    doc.tile_hint = hovered;

    // a tile of another tileset: copied into this map's secondary tileset, in colours the pair has or can take
    if let Some((from, secondary, tile, palette)) = borrow {
        if in_primary {
            doc.notice = Some(primary_only(id));
        } else {
            match doc.game.borrow_tile(pair, from, secondary, tile, palette) {
                Ok(borrowed) => {
                    (entry.tile, entry.palette) = (borrowed.tile, borrowed.palette);
                    doc.notice = Some(borrowed_as(&borrowed));
                    doc.tileset_changed();
                }
                Err(e) => doc.notice = Some(e.to_string()),
            }
        }
    }

    // -- into the ROM --------------------------------------------------------------------------
    // in a game of blocks, a tile comes with its palette
    let fixed = if blocks { doc.game.tile_palette(tileset.offset, entry.tile as usize) } else { None };
    entry.palette = fixed.unwrap_or(entry.palette);
    entries[doc.entry] = entry.pack();
    if (entries, attribute) != before {
        doc.game.set_entries(pair, id, &entries);
        doc.game.set_attribute(pair, id, attribute);
        doc.tileset_changed();
    }
    // a value dragged is one step once the button is released, not one per frame
    if !ui.input(|i| i.pointer.any_down()) {
        doc.game.commit();
    }
    // the new metatile, once this one is as it was left: a step of its own, and the one in hand
    if let Some(like) = add {
        match doc.game.add_metatile(pair, like) {
            Ok(new) => {
                doc.game.commit();
                doc.tileset_changed();
                doc.select_metatile(new);
                doc.notice = Some(match like {
                    Some(original) => format!(
                        "Metatile 0x{new:03X} is a copy of 0x{original:03X}, new in this map's secondary tileset: no map has it yet"
                    ),
                    None => format!("Metatile 0x{new:03X} is new in this map's secondary tileset: empty, and on no map yet"),
                });
            }
            Err(e) => doc.notice = Some(e.to_string()),
        }
    }
}

fn primary_only(id: u16) -> String {
    format!("Metatile 0x{id:03X} is in the primary tileset, which every map shares: it's made of the primary tileset's tiles only. Duplicate it: the copy is in the secondary tileset, and takes any tile.")
}

/// What became of a tile taken in another tileset
fn borrowed_as(borrowed: &Borrowed) -> String {
    let tile = if borrowed.existing {
        format!("This map's tilesets had that tile: 0x{:03X}", borrowed.tile)
    } else {
        format!("Copied into this map's secondary tileset as tile 0x{:03X}", borrowed.tile)
    };
    match borrowed.colors {
        Colors::Found => format!("{tile}, in palette {}", borrowed.palette),
        Colors::Added => format!("{tile}; its colours were written to palette {}, which no metatile used", borrowed.palette),
        Colors::Approximated => {
            format!("{tile}; no palette has its colours and none is free: it's in the closest ones of palette {}", borrowed.palette)
        }
    }
}

/// A combo box for a field whose values have names, a number for the others
fn field_editor(ui: &mut Ui, field: &AttrField, attribute: u32) -> u32 {
    let mut value = field.get(attribute);
    ui.horizontal(|ui| {
        ui.label(field.name);
        if field.values.is_empty() {
            ui.add(egui::DragValue::new(&mut value).range(0..=field.max()).speed(0.1));
        } else {
            let name = |v: u32| field.values.get(v as usize).map_or_else(|| v.to_string(), |n| n.to_string());
            egui::ComboBox::from_id_salt(field.name).selected_text(name(value)).width(170.0).show_ui(ui, |ui| {
                for v in 0..=(field.values.len() as u32 - 1).max(value).min(field.max()) {
                    ui.selectable_value(&mut value, v, name(v));
                }
            });
        }
    });
    field.set(attribute, value)
}
