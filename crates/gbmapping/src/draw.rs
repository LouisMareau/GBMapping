//! The Draw tab: the selected metatile, a pixel at a time. And what drawing a pixel at a
//! time takes, wherever it's done (the frames of animations are drawn on the same way).
//!
//! One of a metatile's two layers is drawn on: 16x16 pixels, each one of the 15 colours
//! of the palette its quarter is drawn in, or nothing. What's drawn goes into the tiles
//! the quarters are made of (see `gbmapping_core::pixels`): a quarter that shares its
//! tile with others gets a copy first, unless the tile itself is what's to change.

use crate::app::Settings;
use crate::doc::Doc;
use crate::map_view::snap;
use crate::side::{self, QUARTERS, WARNING};
use egui::{vec2, Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
use gbmapping_core::gfx::{Palette, TileEntry, METATILE, TILE};
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

/// What drawing keeps from one frame to the next
pub struct Draw {
    /// The metatile's layer drawn on (0: bottom, 1: top)
    pub layer: usize,
    pub pen: Pen,
    /// The pencil's colour: its number in the palette of what's drawn on (1 to 15)
    pub color: u8,
    /// The metatile's quarter last pointed at: the palette shown is its
    pub quarter: usize,
    /// The metatile's other layer is shown too
    pub both: bool,
    /// Where the stroke under way last was
    last: Option<(usize, usize)>,
}

impl Default for Draw {
    fn default() -> Self {
        Self { layer: 0, pen: Pen::Pencil, color: 1, quarter: 0, both: false, last: None }
    }
}

/// A pixel: the colour's number in its palette (0: nothing), and that palette
pub type Pixel = (u8, u8);

/// A picture to draw on, and what's shown with it
pub struct Layers<'a> {
    pub width: usize,
    pub height: usize,
    /// What's behind it
    pub under: Option<&'a [Pixel]>,
    /// The pixels drawn on
    pub pixels: &'a [Pixel],
    /// What's over it, and how much it hides (0 to 1): another layer, or a ghost of another frame
    pub over: Option<(&'a [Pixel], f32)>,
}

/// The pointer on a picture
pub struct Pointed {
    /// The pixel it's over
    pub over: Option<(usize, usize)>,
    /// The pixel it's on with a button pressed there
    pub held: Option<(usize, usize)>,
    pub primary: bool,
    pub secondary: bool,
}

/// A pixel's colour on the screen: `None` where there's nothing
fn shown(pixel: Pixel, palettes: &[Palette]) -> Option<Color32> {
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
fn area(layers: &Layers, palettes: &[Palette], at: (usize, usize)) -> Vec<(usize, usize)> {
    let (width, height) = (layers.width, layers.height);
    let look = |(x, y): (usize, usize)| shown(layers.pixels[y * width + x], palettes);
    let wanted = look(at);
    let mut seen = vec![false; width * height];
    seen[at.1 * width + at.0] = true;
    let mut found = vec![at];
    let mut next = 0;
    while let Some(&(x, y)) = found.get(next) {
        next += 1;
        for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
            if nx < width && ny < height && !seen[ny * width + nx] && look((nx, ny)) == wanted {
                seen[ny * width + nx] = true;
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

/// Shows a picture large, `cell` points a pixel, with its pixels' grid and its tiles',
/// and tells where the pointer is on it
pub fn canvas(ui: &mut Ui, layers: &Layers, palettes: &[Palette], cell: f32) -> Pointed {
    let ppp = ui.pixels_per_point();
    let (width, height) = (layers.width, layers.height);
    let size = vec2(width as f32, height as f32) * cell;
    let (response, painter) = ui.allocate_painter(size, Sense::click_and_drag());
    let origin = snap(response.rect.min, ppp);
    let whole = Rect::from_min_size(origin, size);
    let place = |x: usize, y: usize| Rect::from_min_size(origin + vec2(x as f32, y as f32) * cell, Vec2::splat(cell));
    let mut mesh = Mesh::default();
    for (i, &pixel) in layers.pixels.iter().enumerate().take(width * height) {
        let rect = place(i % width, i / width);
        nothing(&mut mesh, rect);
        for color in [layers.under.and_then(|under| shown(under[i], palettes)), shown(pixel, palettes)].into_iter().flatten() {
            mesh.add_colored_rect(rect, color);
        }
        // what's over is see-through: what's drawn under it shows
        if let Some(color) = layers.over.and_then(|(over, hides)| shown(over[i], palettes).map(|color| color.gamma_multiply(hides))) {
            mesh.add_colored_rect(rect, color);
        }
    }
    painter.add(Shape::mesh(mesh));
    // the pixels' grid when they're large enough for one, and the tiles'
    let (faint, strong) = (Stroke::new(1.0, Color32::from_black_alpha(45)), Stroke::new(1.0, Color32::from_white_alpha(120)));
    for x in (1..width).filter(|x| cell >= 6.0 || x % TILE == 0) {
        painter.vline(origin.x + x as f32 * cell, whole.y_range(), if x % TILE == 0 { strong } else { faint });
    }
    for y in (1..height).filter(|y| cell >= 6.0 || y % TILE == 0) {
        painter.hline(whole.x_range(), origin.y + y as f32 * cell, if y % TILE == 0 { strong } else { faint });
    }
    painter.rect_stroke(whole, 0.0, Stroke::new(1.0, Color32::from_gray(90)), StrokeKind::Outside);

    let (pointer, primary, secondary) = ui.input(|i| (i.pointer.latest_pos(), i.pointer.primary_down(), i.pointer.secondary_down()));
    let pixel_at = |p: Pos2| {
        let on = |v: f32, most: usize| (v / cell).floor().clamp(0.0, most as f32 - 1.0) as usize;
        (on(p.x - origin.x, width), on(p.y - origin.y, height))
    };
    let over = response.hover_pos().map(pixel_at);
    if let Some((x, y)) = over {
        painter.rect_stroke(place(x, y), 0.0, Stroke::new(1.5, Color32::WHITE), StrokeKind::Inside);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    Pointed { over, held: pointer.filter(|_| response.is_pointer_button_down_on()).map(pixel_at), primary, secondary }
}

/// What the pointer draws this frame: the pixels of a stroke since it last was looked
/// at, each with the pen's colour. The right button takes what's under it instead: a
/// colour, or nothing.
pub fn stroke(draw: &mut Draw, pointed: &Pointed, layers: &Layers, palettes: &[Palette]) -> Vec<(usize, usize, u8)> {
    match pointed.held {
        Some((x, y)) if pointed.secondary => {
            match layers.pixels[y * layers.width + x].0 {
                0 => draw.pen = Pen::Eraser,
                color => {
                    draw.color = color;
                    draw.pen = if draw.pen == Pen::Eraser { Pen::Pencil } else { draw.pen };
                }
            }
            Vec::new()
        }
        Some(at) if pointed.primary => {
            let color = if draw.pen == Pen::Eraser { 0 } else { draw.color };
            let points = match (draw.pen, draw.last) {
                (Pen::Fill, None) => area(layers, palettes, at),
                (Pen::Fill, Some(_)) => Vec::new(),
                (_, last) => line(last.unwrap_or(at), at),
            };
            draw.last = Some(at);
            points.into_iter().map(|(x, y)| (x, y, color)).collect()
        }
        _ => {
            draw.last = None;
            Vec::new()
        }
    }
}

/// The pens to choose from
pub fn pens(ui: &mut Ui, pen: &mut Pen) {
    ui.horizontal(|ui| {
        let all = [
            (Pen::Pencil, "Pencil", "Draws in the selected colour. The right button takes the colour under it."),
            (Pen::Eraser, "Eraser", "Leaves nothing where it goes: what's under shows through, or what's behind the map."),
            (Pen::Fill, "Fill", "Gives the selected colour to the pixels that look like the one clicked and touch it."),
        ];
        for (one, name, hint) in all {
            ui.selectable_value(pen, one, name).on_hover_text(hint);
        }
    });
}

/// A palette's 15 colours and nothing, to choose the pencil's from
pub fn swatches(ui: &mut Ui, draw: &mut Draw, palettes: &[Palette], palette: usize) {
    let ppp = ui.pixels_per_point();
    let swatch = ((ui.available_width() * ppp / 16.0).floor() / ppp).clamp(4.0, 20.0);
    let (response, painter) = ui.allocate_painter(vec2(swatch * 16.0, swatch), Sense::click());
    let origin = snap(response.rect.min, ppp);
    let place = |i: usize| Rect::from_min_size(origin + vec2(i as f32 * swatch, 0.0), Vec2::splat(swatch));
    let mut mesh = Mesh::default();
    nothing(&mut mesh, place(0));
    for i in 1..16 {
        if let Some(color) = shown((i as u8, palette as u8), palettes) {
            mesh.add_colored_rect(place(i), color);
        }
    }
    painter.add(Shape::mesh(mesh));
    let selected = if draw.pen == Pen::Eraser { 0 } else { draw.color as usize & 0xF };
    painter.rect_stroke(place(selected), 0.0, Stroke::new(2.0, ACCENT), StrokeKind::Inside);
    painter.rect_stroke(place(selected), 0.0, Stroke::new(1.0, Color32::BLACK), StrokeKind::Outside);
    if let Some(p) = response.interact_pointer_pos().filter(|_| response.clicked()) {
        match (((p.x - origin.x) / swatch).max(0.0) as usize).min(15) {
            0 => draw.pen = Pen::Eraser,
            i => {
                draw.color = i as u8;
                draw.pen = if draw.pen == Pen::Eraser { Pen::Pencil } else { draw.pen };
            }
        }
    }
}

/// The pencil's colour, to change in the palette itself
pub fn color_button(ui: &mut Ui, doc: &mut Doc, pair: Pair, palettes: &[Palette], palette: usize) {
    ui.horizontal(|ui| {
        let index = doc.draw.color as usize & 0xF;
        let mut rgb = palettes.get(palette).map_or([0; 3], |colors| colors[index]);
        ui.label(egui::RichText::new(format!("Colour {index} of palette {palette}")).small());
        let edit = ui.color_edit_button_srgb(&mut rgb).on_hover_text(format!(
            "Changes the colour itself: everything drawn in palette {palette} with it changes, on every map of the tileset the palette is of."
        ));
        if edit.changed() {
            let color = (rgb[0] as u16 >> 3) | ((rgb[1] as u16 >> 3) << 5) | ((rgb[2] as u16 >> 3) << 10);
            doc.game.set_color(pair, palette, index, color);
        }
    });
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
    pens(ui, &mut doc.draw.pen);
    ui.checkbox(&mut settings.own_tiles, "Only this metatile").on_hover_text(
        "A metatile is made of tiles that other metatiles use too.\nOn: a quarter that shares its tile gets a copy of it when it's drawn on, new in this map's secondary tileset: nothing else changes.\nOff: the tiles themselves are drawn on, and every metatile that has them shows it.",
    );
    let layer = doc.draw.layer.min(1);

    // -- the layer, large: over the bottom one being drawn on, the top one is see-through ---
    let pixels = doc.game.layer_pixels(pair, id, layer);
    let other = doc.draw.both.then(|| doc.game.layer_pixels(pair, id, 1 - layer));
    let palettes = doc.game.palettes(pair);
    let layers = Layers {
        width: METATILE,
        height: METATILE,
        under: other.as_ref().filter(|_| layer == 1).map(|other| &other[..]),
        pixels: &pixels,
        over: other.as_ref().filter(|_| layer == 0).map(|other| (&other[..], 0.45)),
    };
    let cell = ((ui.available_width() * ppp / METATILE as f32).floor() / ppp).clamp(4.0, 20.0);
    let pointed = canvas(ui, &layers, &palettes, cell);
    if let Some((x, y)) = pointed.held.or(pointed.over) {
        doc.draw.quarter = (y / TILE) * 2 + x / TILE;
    }
    let drawn = stroke(&mut doc.draw, &pointed, &layers, &palettes);
    if pointed.held.is_some() && pointed.primary {
        match doc.game.draw_on_metatile(pair, id, layer, &drawn, settings.own_tiles) {
            Ok(true) => ui.ctx().request_repaint(),
            Ok(false) => {}
            Err(e) => doc.notice = Some(e.to_string()),
        }
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
    swatches(ui, &mut doc.draw, &palettes, palette);
    color_button(ui, doc, pair, &palettes, palette);

    // -- what drawing there changes ------------------------------------------------------------
    let users = doc.tile_users(pair, entry.tile);
    let shared = users > 1;
    let (free, in_primary) = (doc.game.free_tiles(pair), (id as usize) < format.primary_metatiles);
    ui.label(
        small(format!("Its tile, 0x{:03X}, is in {users} quarter{} of metatiles", entry.tile, if users == 1 { "" } else { "s" })).weak(),
    );
    match (shared, settings.own_tiles) {
        (true, true) if in_primary => {
            let why = "This metatile is the primary tileset's: it can't have a tile of its own. Duplicate it and draw on the copy, or turn \"Only this metatile\" off.";
            ui.label(small(why.to_string()).color(WARNING));
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
