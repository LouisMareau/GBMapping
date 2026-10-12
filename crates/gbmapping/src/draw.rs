//! The Draw tab: the selected metatile, a pixel at a time.
//!
//! One of its two layers is drawn on: 16x16 pixels, each one of the 15 colours of the
//! palette its quarter is drawn in, or nothing. What's drawn goes into the tiles the
//! quarters are made of (see `gbmapping_core::pixels`): a quarter that shares its tile
//! with others gets a copy first, unless the tile itself is what's to change.

use crate::app::Settings;
use crate::doc::Doc;
use crate::map_view::snap;
use crate::side::{self, QUARTERS, WARNING};
use egui::{vec2, Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
use gbmapping_core::gfx::{Palette, TileEntry, METATILE, TILE};
use gbmapping_core::pixels::LayerPixels;
use gbmapping_core::Pair;

const ACCENT: Color32 = Color32::from_rgb(76, 175, 80);
/// What shows where a pixel is nothing: a checker of two greys
const NOTHING: [Color32; 2] = [Color32::from_gray(48), Color32::from_gray(70)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pen {
    Pencil,
    Eraser,
    Fill,
}

/// What the tab keeps from one frame to the next
pub struct Draw {
    /// The layer drawn on (0: bottom, 1: top)
    pub layer: usize,
    pub pen: Pen,
    /// The pencil's colour: its number in the palette of the quarter drawn on (1 to 15)
    pub color: u8,
    /// The quarter last pointed at: the palette shown is its
    pub quarter: usize,
    /// The other layer is shown too
    pub both: bool,
    /// Where the stroke under way last was
    last: Option<(usize, usize)>,
}

impl Default for Draw {
    fn default() -> Self {
        Self { layer: 0, pen: Pen::Pencil, color: 1, quarter: 0, both: false, last: None }
    }
}

/// A pixel's colour on the screen: `None` where there's nothing
fn shown(pixel: (u8, u8), palettes: &[Palette]) -> Option<Color32> {
    let [r, g, b] = palettes.get(pixel.1 as usize).map_or([255, 0, 255], |palette| palette[pixel.0 as usize & 0xF]);
    (pixel.0 != 0).then(|| Color32::from_rgb(r, g, b))
}

/// The pixels from one to another: a fast stroke leaves no gap
fn line(from: (usize, usize), to: (usize, usize)) -> Vec<(usize, usize)> {
    let (dx, dy) = (to.0 as f32 - from.0 as f32, to.1 as f32 - from.1 as f32);
    let steps = dx.abs().max(dy.abs()) as usize;
    let along = |i: usize| i as f32 / steps.max(1) as f32;
    (0..=steps).map(|i| ((from.0 as f32 + dx * along(i)).round() as usize, (from.1 as f32 + dy * along(i)).round() as usize)).collect()
}

/// The pixels that look like the one at `at` and touch it, one after the other
fn area(pixels: &LayerPixels, palettes: &[Palette], at: (usize, usize)) -> Vec<(usize, usize)> {
    let look = |(x, y): (usize, usize)| shown(pixels[y * METATILE + x], palettes);
    let wanted = look(at);
    let mut seen = [false; METATILE * METATILE];
    seen[at.1 * METATILE + at.0] = true;
    let mut found = vec![at];
    let mut next = 0;
    while let Some(&(x, y)) = found.get(next) {
        next += 1;
        for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
            if nx < METATILE && ny < METATILE && !seen[ny * METATILE + nx] && look((nx, ny)) == wanted {
                seen[ny * METATILE + nx] = true;
                found.push((nx, ny));
            }
        }
    }
    found
}

/// A checker where there's nothing to see
fn nothing(mesh: &mut Mesh, rect: Rect) {
    let half = rect.size() / 2.0;
    for q in 0..4 {
        let at = rect.min + vec2((q % 2) as f32 * half.x, (q / 2) as f32 * half.y);
        mesh.add_colored_rect(Rect::from_min_size(at, half), NOTHING[(q % 2 + q / 2) % 2]);
    }
}

pub fn show(ui: &mut Ui, doc: &mut Doc, settings: &mut Settings, pair: Pair) {
    let ppp = ui.pixels_per_point();
    let id = doc.stamp.first();
    let format = doc.game.format;
    let Some((tileset, local)) = doc.game.locate(pair, id) else {
        ui.label(format!("Metatile 0x{id:03X} holds nothing in these tilesets."));
        ui.weak("Pick another one in the Metatiles tab.");
        return;
    };
    let small = |text: String| egui::RichText::new(text).small();
    ui.label(egui::RichText::new(format!("Metatile 0x{id:03X}")).strong());
    let on_maps = match doc.metatile_uses(tileset.offset, local) {
        0 => "On no map".to_string(),
        n => format!("On the maps {n} times: they change with it"),
    };
    ui.label(small(on_maps).weak());
    let mut add = None;
    ui.horizontal(|ui| add = side::new_buttons(ui, id));
    ui.add_space(2.0);

    // -- which layer, with what -------------------------------------------------------------
    ui.horizontal(|ui| {
        for (layer, name) in ["Bottom layer", "Top layer"].into_iter().enumerate() {
            ui.selectable_value(&mut doc.draw.layer, layer, name);
        }
        ui.checkbox(&mut doc.draw.both, "Show both")
            .on_hover_text("The other layer too, as the game draws them: the top one over the bottom one.");
    });
    ui.horizontal(|ui| {
        let pens = [
            (Pen::Pencil, "Pencil", "Draws in the selected colour. The right button takes the colour under it."),
            (Pen::Eraser, "Eraser", "Leaves nothing where it goes: the layer under shows through, or what's behind the map."),
            (Pen::Fill, "Fill", "Gives the selected colour to the pixels that look like the one clicked and touch it."),
        ];
        for (pen, name, hint) in pens {
            ui.selectable_value(&mut doc.draw.pen, pen, name).on_hover_text(hint);
        }
    });
    ui.checkbox(&mut settings.own_tiles, "Only this metatile").on_hover_text(
        "A metatile is made of tiles that other metatiles use too.\nOn: a quarter that shares its tile gets a copy of it when it's drawn on, new in this map's secondary tileset: nothing else changes.\nOff: the tiles themselves are drawn on, and every metatile that has them shows it.",
    );
    let layer = doc.draw.layer.min(1);

    // -- the layer, large ---------------------------------------------------------------------
    let pixels = doc.game.layer_pixels(pair, id, layer);
    let other = doc.draw.both.then(|| doc.game.layer_pixels(pair, id, 1 - layer));
    let palettes = doc.game.palettes(pair);
    let cell = ((ui.available_width() * ppp / METATILE as f32).floor() / ppp).clamp(4.0, 20.0);
    let (response, painter) = ui.allocate_painter(Vec2::splat(cell * METATILE as f32), Sense::click_and_drag());
    let origin = snap(response.rect.min, ppp);
    let whole = Rect::from_min_size(origin, Vec2::splat(cell * METATILE as f32));
    let place = |x: usize, y: usize| Rect::from_min_size(origin + vec2(x as f32, y as f32) * cell, Vec2::splat(cell));
    let mut mesh = Mesh::default();
    for (i, &pixel) in pixels.iter().enumerate() {
        let rect = place(i % METATILE, i / METATILE);
        nothing(&mut mesh, rect);
        let (bottom, top) = match (&other, layer) {
            (Some(other), 0) => (pixel, Some(other[i])),
            (Some(other), _) => (other[i], Some(pixel)),
            (None, _) => (pixel, None),
        };
        if let Some(color) = shown(bottom, &palettes) {
            mesh.add_colored_rect(rect, color);
        }
        // over the layer that's drawn on, the top one is see-through: what's drawn under it shows
        if let Some(color) = top.and_then(|top| shown(top, &palettes)) {
            mesh.add_colored_rect(rect, if layer == 0 { color.gamma_multiply(0.45) } else { color });
        }
    }
    painter.add(Shape::mesh(mesh));
    // the pixels' grid, and the four tiles'
    let (faint, strong) = (Stroke::new(1.0, Color32::from_black_alpha(45)), Stroke::new(1.0, Color32::from_white_alpha(120)));
    for i in 1..METATILE {
        let stroke = if i == TILE { strong } else { faint };
        painter.vline(origin.x + i as f32 * cell, whole.y_range(), stroke);
        painter.hline(whole.x_range(), origin.y + i as f32 * cell, stroke);
    }
    painter.rect_stroke(whole, 0.0, Stroke::new(1.0, Color32::from_gray(90)), StrokeKind::Outside);

    // -- the pointer on it -----------------------------------------------------------------------
    let (pointer, primary, secondary) = ui.input(|i| (i.pointer.latest_pos(), i.pointer.primary_down(), i.pointer.secondary_down()));
    let pixel_at = |p: Pos2| {
        let on = |v: f32| (v / cell).floor().clamp(0.0, METATILE as f32 - 1.0) as usize;
        (on(p.x - origin.x), on(p.y - origin.y))
    };
    let over = response.hover_pos().map(pixel_at);
    let held = pointer.filter(|_| response.is_pointer_button_down_on()).map(pixel_at);
    if let Some((x, y)) = held.or(over) {
        doc.draw.quarter = (y / TILE) * 2 + x / TILE;
    }
    match held {
        // the right button takes what's under it: a colour, or nothing
        Some((x, y)) if secondary => match pixels[y * METATILE + x].0 {
            0 => doc.draw.pen = Pen::Eraser,
            color => {
                doc.draw.color = color;
                doc.draw.pen = if doc.draw.pen == Pen::Eraser { Pen::Pencil } else { doc.draw.pen };
            }
        },
        Some(at) if primary => {
            let color = if doc.draw.pen == Pen::Eraser { 0 } else { doc.draw.color };
            let points = match (doc.draw.pen, doc.draw.last) {
                (Pen::Fill, None) => area(&pixels, &palettes, at),
                (Pen::Fill, Some(_)) => Vec::new(),
                (_, last) => line(last.unwrap_or(at), at),
            };
            doc.draw.last = Some(at);
            let drawn: Vec<(usize, usize, u8)> = points.into_iter().map(|(x, y)| (x, y, color)).collect();
            match doc.game.draw_on_metatile(pair, id, layer, &drawn, settings.own_tiles) {
                Ok(true) => ui.ctx().request_repaint(),
                Ok(false) => {}
                Err(e) => doc.notice = Some(e.to_string()),
            }
        }
        _ => doc.draw.last = None,
    }
    if let Some((x, y)) = over {
        painter.rect_stroke(place(x, y), 0.0, Stroke::new(1.5, Color32::WHITE), StrokeKind::Inside);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }

    // -- the colours: the palette of the quarter pointed at -----------------------------------
    ui.add_space(4.0);
    let quarter = doc.draw.quarter.min(3);
    let mut entries = doc.game.entries(pair, id);
    let mut entry = TileEntry::unpack(entries[layer * 4 + quarter]);
    ui.horizontal(|ui| {
        ui.label(small(format!("The {} quarter is drawn in palette", QUARTERS[quarter])));
        ui.add(egui::DragValue::new(&mut entry.palette).range(0..=format.total_palettes as u8 - 1).speed(0.05))
            .on_hover_text("Each quarter of a layer is drawn in one of the tilesets' palettes: 15 colours, and nothing.");
    });
    if entry.pack() != entries[layer * 4 + quarter] {
        entries[layer * 4 + quarter] = entry.pack();
        doc.game.set_entries(pair, id, &entries);
    }
    let palette = entry.palette as usize;
    let swatch = ((ui.available_width() * ppp / 16.0).floor() / ppp).clamp(4.0, cell);
    let (response, painter) = ui.allocate_painter(vec2(swatch * 16.0, swatch), Sense::click());
    let origin = snap(response.rect.min, ppp);
    let place = |i: usize| Rect::from_min_size(origin + vec2(i as f32 * swatch, 0.0), Vec2::splat(swatch));
    let mut mesh = Mesh::default();
    nothing(&mut mesh, place(0));
    for i in 1..16 {
        if let Some(color) = shown((i as u8, palette as u8), &palettes) {
            mesh.add_colored_rect(place(i), color);
        }
    }
    painter.add(Shape::mesh(mesh));
    let selected = if doc.draw.pen == Pen::Eraser { 0 } else { doc.draw.color as usize & 0xF };
    painter.rect_stroke(place(selected), 0.0, Stroke::new(2.0, ACCENT), StrokeKind::Inside);
    painter.rect_stroke(place(selected), 0.0, Stroke::new(1.0, Color32::BLACK), StrokeKind::Outside);
    if let Some(p) = response.interact_pointer_pos().filter(|_| response.clicked()) {
        match (((p.x - origin.x) / swatch).max(0.0) as usize).min(15) {
            0 => doc.draw.pen = Pen::Eraser,
            i => {
                doc.draw.color = i as u8;
                doc.draw.pen = if doc.draw.pen == Pen::Eraser { Pen::Pencil } else { doc.draw.pen };
            }
        }
    }
    ui.horizontal(|ui| {
        let index = doc.draw.color as usize & 0xF;
        let mut rgb = palettes.get(palette).map_or([0; 3], |colors| colors[index]);
        ui.label(small(format!("Colour {index} of palette {palette}")));
        let edit = ui.color_edit_button_srgb(&mut rgb).on_hover_text(format!(
            "Changes the colour itself: everything drawn in palette {palette} with it changes, on every map of the tileset the palette is of."
        ));
        if edit.changed() {
            let color = (rgb[0] as u16 >> 3) | ((rgb[1] as u16 >> 3) << 5) | ((rgb[2] as u16 >> 3) << 10);
            doc.game.set_color(pair, palette, index, color);
        }
    });

    // -- what drawing there changes ------------------------------------------------------------
    let users = doc.tile_users(pair, entry.tile);
    let shared = users > 1;
    let (free, in_primary) = (doc.game.free_tiles(pair), (id as usize) < format.primary_metatiles);
    ui.label(
        small(format!("Its tile, 0x{:03X}, is in {users} quarter{} of metatiles", entry.tile, if users == 1 { "" } else { "s" })).weak(),
    );
    match (shared, settings.own_tiles) {
        (true, true) if in_primary => {
            ui.label(small("This metatile is the primary tileset's: it can't have a tile of its own. Duplicate it and draw on the copy, or turn \"Only this metatile\" off.".to_string()).color(WARNING));
        }
        (true, true) if free == 0 => {
            ui.label(
                small("This map's secondary tileset has no tile left to give: the quarter can't have its own.".to_string()).color(WARNING),
            );
        }
        (true, true) => {
            ui.label(
                small(format!("Drawn on, the quarter gets its own copy of it ({free} tiles left in this map's secondary tileset).")).weak(),
            );
        }
        (true, false) => {
            ui.label(small("Drawn on, they all change.".to_string()).color(WARNING));
        }
        (false, _) => {}
    }

    // a stroke, a colour dragged: one step once the button is released
    if !ui.input(|i| i.pointer.any_down()) && doc.game.commit() {
        doc.tileset_changed();
    }
    if let Some(like) = add {
        side::add_metatile(doc, pair, like);
    }
}
