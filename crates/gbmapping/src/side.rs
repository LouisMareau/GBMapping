//! The side panel: the metatile picker (and the map's border), and the tileset tab
//! where a metatile's tiles and attributes are edited.

use crate::app::{Settings, Tab};
use crate::doc::{texture, Doc, LayerTextures, Stamp, TilesTexture, PICKER_COLUMNS};
use crate::map_view::{scale_for, snap};
use egui::{pos2, vec2, Align, Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
use gbmapping_core::format::{block, AttrField};
use gbmapping_core::gfx::{TileEntry, METATILE, TILE};
use gbmapping_core::Pair;

const ACCENT: Color32 = Color32::from_rgb(76, 175, 80);
const GROUND: Color32 = Color32::from_gray(17);
const WHOLE: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
const TILE_COLUMNS: usize = 16;

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
        Tab::Tileset => tileset(ui, doc, pair),
    }
}

/// "0x00D TALL_GRASS"
pub fn describe(doc: &Doc, pair: Pair, id: u16) -> String {
    if doc.game.locate(pair, id).is_none() {
        return format!("0x{id:03X} (nothing)");
    }
    let behavior = doc.game.behavior(pair, id);
    match doc.game.format.behavior_name(behavior) {
        Some(name) => format!("0x{id:03X} {name}"),
        None => format!("0x{id:03X} behavior 0x{behavior:02X}"),
    }
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
    ui.label(egui::RichText::new(format!("{}{size}", describe(doc, pair, shown))).small());

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
            ui.label(
                egui::RichText::new("Border: what's drawn around the map.\nClick a tile to set it from the selection.").small().weak(),
            );
        });
    }
    ui.separator();

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
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
                doc.stamp = Stamp::from_picker(start.0, start.1, at.0, at.1);
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

fn tileset(ui: &mut Ui, doc: &mut Doc, pair: Pair) {
    let ctx = ui.ctx().clone();
    let ppp = ui.pixels_per_point();
    let id = doc.stamp.first();
    let format = doc.game.format;
    let Some((tileset, local)) = doc.game.locate(pair, id) else {
        ui.label(format!("Metatile 0x{id:03X} holds nothing in these tilesets."));
        ui.weak("Pick another one in the Metatiles tab.");
        return;
    };
    ui.label(egui::RichText::new(format!("Metatile 0x{id:03X}")).strong());
    let which = if (id as usize) < format.primary_metatiles { "primary" } else { "secondary" };
    ui.label(egui::RichText::new(format!("number {local} of the {which} tileset at 0x{:06X}", tileset.offset)).small().weak());
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
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("Both").small());
            let (response, painter) = ui.allocate_painter(Vec2::splat(big), Sense::hover());
            let rect = Rect::from_min_size(snap(response.rect.min, ppp), Vec2::splat(big));
            painter.image(sheet_texture, rect, crate::doc::sheet_uv(id as usize, sheet_rows), Color32::WHITE);
        });
        for (layer, name) in ["Bottom", "Top"].into_iter().enumerate() {
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
    let quarter = ["top left", "top right", "bottom left", "bottom right"][doc.entry % 4];
    ui.label(egui::RichText::new(format!("{} layer, {quarter}:", if doc.entry < 4 { "Bottom" } else { "Top" })).small());
    ui.horizontal(|ui| {
        ui.label("Tile");
        ui.add(egui::DragValue::new(&mut entry.tile).range(0..=format.total_tiles as u16 - 1).speed(0.25));
        ui.label("Palette");
        ui.add(egui::DragValue::new(&mut entry.palette).range(0..=15).speed(0.1));
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut entry.hflip, "Flip ↔");
        ui.checkbox(&mut entry.vflip, "Flip ↕");
    });

    // -- the metatile's attributes ---------------------------------------------------------
    ui.separator();
    let mut behavior = format.behavior.get(attribute);
    ui.horizontal(|ui| {
        ui.label("Behavior");
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
    for field in std::iter::once(&format.layer_type).chain(format.extra_fields) {
        attribute = field_editor(ui, field, attribute);
    }

    // -- the tiles: click one to put it in the selected quarter -----------------------------
    ui.separator();
    let palette = entry.palette as usize;
    if doc.tiles.as_ref().is_none_or(|t| (t.pair, t.palette) != (pair, palette)) {
        let image = doc.game.render_tiles(pair, palette, TILE_COLUMNS);
        doc.tiles = Some(TilesTexture { pair, palette, texture: texture(&ctx, "tiles", &image), count: doc.game.gfx(pair).tile_count() });
    }
    let Some((tiles_texture, tile_count)) = doc.tiles.as_ref().map(|t| (t.texture.id(), t.count)) else { return };
    let tile_rows = tile_count.div_ceil(TILE_COLUMNS).max(1);
    let cell = TILE as f32 * scale_for(2, ppp);
    let mut hovered_tile = None;
    let hint = ui.label(egui::RichText::new(format!("Tiles, in palette {palette}")).small());
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let size = vec2(TILE_COLUMNS as f32, tile_rows as f32) * cell;
        let (response, painter) = ui.allocate_painter(size, Sense::click());
        let origin = snap(response.rect.min, ppp);
        let rect = Rect::from_min_size(origin, size);
        painter.rect_filled(rect, 0.0, GROUND);
        painter.image(tiles_texture, rect, WHOLE, Color32::WHITE);
        let tile_at = |p: Pos2| {
            ((p.y - origin.y) / cell).max(0.0) as usize * TILE_COLUMNS + (((p.x - origin.x) / cell).max(0.0) as usize).min(TILE_COLUMNS - 1)
        };
        hovered_tile = response.hover_pos().map(tile_at).filter(|&t| t < tile_count);
        if let Some(t) = response.interact_pointer_pos().filter(|_| response.clicked()).map(tile_at).filter(|&t| t < tile_count) {
            entry.tile = t as u16;
        }
        let t = entry.tile as usize;
        let selected = Rect::from_min_size(origin + vec2((t % TILE_COLUMNS) as f32, (t / TILE_COLUMNS) as f32) * cell, Vec2::splat(cell));
        painter.rect_stroke(selected, 0.0, Stroke::new(1.5, ACCENT), StrokeKind::Outside);
    });
    if let Some(t) = hovered_tile {
        hint.on_hover_text(format!("tile {t}"));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // -- into the ROM --------------------------------------------------------------------------
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
