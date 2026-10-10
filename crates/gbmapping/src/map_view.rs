//! The map: drawn from the metatile sheet, painted with the pointer. And the reference
//! next to it: another map drawn the same way, only looked at and picked from.

use crate::app::Settings;
use crate::doc::{sheet_uv, Doc, Hover, Sheet, Stamp, Tool};
use egui::{pos2, vec2, Align2, Color32, FontId, Mesh, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, TextureId, Ui, Vec2};
use gbmapping_core::format::block;
use gbmapping_core::gfx::METATILE;
use gbmapping_core::Map;

pub const MAX_ZOOM: u32 = 8;
const ACCENT: Color32 = Color32::from_rgb(76, 175, 80);
const MISSING: Color32 = Color32::from_rgb(255, 0, 255);

/// Points per game pixel at a zoom level, so that a game pixel covers whole screen pixels
pub fn scale_for(zoom: u32, pixels_per_point: f32) -> f32 {
    (zoom as f32 * pixels_per_point).round().max(1.0) / pixels_per_point
}

/// A position on the screen's pixel grid: tiles drawn there have no seams
pub fn snap(p: Pos2, pixels_per_point: f32) -> Pos2 {
    pos2((p.x * pixels_per_point).round() / pixels_per_point, (p.y * pixels_per_point).round() / pixels_per_point)
}

/// Where a map is on the screen
struct Canvas {
    origin: Pos2,
    cell: f32,
    width: usize,
    height: usize,
}

impl Canvas {
    fn rect(&self) -> Rect {
        Rect::from_min_size(self.origin, vec2(self.width as f32, self.height as f32) * self.cell)
    }

    /// The tile at a point of the screen (which may be outside the map)
    fn tile_at(&self, p: Pos2) -> (isize, isize) {
        (((p.x - self.origin.x) / self.cell).floor() as isize, ((p.y - self.origin.y) / self.cell).floor() as isize)
    }

    fn holds(&self, c: (isize, isize)) -> bool {
        c.0 >= 0 && c.1 >= 0 && (c.0 as usize) < self.width && (c.1 as usize) < self.height
    }

    fn clamped(&self, c: (isize, isize)) -> (usize, usize) {
        (c.0.clamp(0, self.width as isize - 1) as usize, c.1.clamp(0, self.height as isize - 1) as usize)
    }

    fn tile(&self, x: usize, y: usize) -> Rect {
        Rect::from_min_size(self.origin + vec2(x as f32, y as f32) * self.cell, Vec2::splat(self.cell))
    }

    /// The rectangle of tiles from one corner to the other
    fn tiles(&self, a: (usize, usize), b: (usize, usize)) -> Rect {
        self.tile(a.0.min(b.0), a.1.min(b.1)).union(self.tile(a.0.max(b.0), a.1.max(b.1))).intersect(self.rect())
    }
}

/// What drawing a map takes from its tilesets' sheet
#[derive(Clone, Copy)]
struct Tiles {
    texture: TextureId,
    count: usize,
    rows: usize,
}

impl Tiles {
    fn of(sheet: &Sheet) -> Self {
        Self { texture: sheet.texture.id(), count: sheet.count, rows: sheet.rows }
    }
}

/// The pointer over a map's pane this frame
struct Pointer {
    /// The tile it's over, when it's over the pane (or holds a button pressed there)
    over: Option<(isize, isize)>,
    pressed: bool,
    primary: bool,
    secondary: bool,
}

/// Reads the pointer for a pane, and does what both panes do with it: the middle button
/// drags the view, Ctrl + wheel (or a pinch) zooms a level per notch
fn pointer(ui: &Ui, response: &egui::Response, canvas: &Canvas, zoom: &mut u32, wheel: &mut f32) -> Pointer {
    let (position, primary, secondary, middle, delta, zoom_delta) = ui.input(|i| {
        (
            i.pointer.latest_pos(),
            i.pointer.primary_down(),
            i.pointer.secondary_down(),
            i.pointer.middle_down(),
            i.pointer.delta(),
            i.zoom_delta(),
        )
    });
    let pressed = response.is_pointer_button_down_on();
    if pressed && middle {
        ui.scroll_with_delta(delta);
    }
    if response.contains_pointer() && zoom_delta != 1.0 {
        *wheel += zoom_delta.ln();
        if wheel.abs() > 0.2 {
            *zoom = (*zoom as i32 + wheel.signum() as i32).clamp(1, MAX_ZOOM as i32) as u32;
            *wheel = 0.0;
        }
    }
    Pointer { over: position.filter(|_| response.contains_pointer() || pressed).map(|p| canvas.tile_at(p)), pressed, primary, secondary }
}

/// The tiles in view, with the overlays that are on
fn draw(painter: &Painter, canvas: &Canvas, map: &Map, tiles: Tiles, settings: &Settings) {
    let (origin, cell, width) = (canvas.origin, canvas.cell, canvas.width);
    let view = painter.clip_rect().intersect(canvas.rect());
    if view.is_negative() {
        return;
    }
    let x0 = ((view.min.x - origin.x) / cell).floor().max(0.0) as usize;
    let y0 = ((view.min.y - origin.y) / cell).floor().max(0.0) as usize;
    let x1 = (((view.max.x - origin.x) / cell).ceil() as usize).min(canvas.width);
    let y1 = (((view.max.y - origin.y) / cell).ceil() as usize).min(canvas.height);

    let mut mesh = Mesh::with_texture(tiles.texture);
    let mut overlay = Mesh::default();
    for y in y0..y1 {
        for x in x0..x1 {
            let b = map.blocks[y * width + x];
            let id = block::metatile(b) as usize;
            if id < tiles.count {
                mesh.add_rect_with_uv(canvas.tile(x, y), sheet_uv(id, tiles.rows), Color32::WHITE);
            } else {
                overlay.add_colored_rect(canvas.tile(x, y), MISSING);
            }
            if settings.show_collision && block::collision(b) != 0 {
                overlay.add_colored_rect(canvas.tile(x, y), Color32::from_rgba_unmultiplied(229, 60, 45, 115));
            }
        }
    }
    painter.add(Shape::mesh(mesh));
    painter.add(Shape::mesh(overlay));

    if settings.show_elevation {
        let font = FontId::monospace((cell * 0.45).clamp(7.0, 14.0));
        let tag = Vec2::splat((cell * 0.5).clamp(8.0, 18.0));
        for y in y0..y1 {
            for x in x0..x1 {
                let e = block::elevation(map.blocks[y * width + x]);
                let r = Rect::from_min_size(canvas.tile(x, y).min, tag);
                painter.rect_filled(r, 0.0, Color32::from_black_alpha(150));
                // 0 and 15 are the elevations of passages between levels
                let color = if e == 0 || e == 15 { Color32::YELLOW } else { Color32::WHITE };
                painter.text(r.center(), Align2::CENTER_CENTER, format!("{e:X}"), font.clone(), color);
            }
        }
    }
    if settings.show_grid {
        let stroke = Stroke::new(1.0, Color32::from_white_alpha(50));
        for x in x0..=x1 {
            painter.vline(origin.x + x as f32 * cell, view.y_range(), stroke);
        }
        for y in y0..=y1 {
            painter.hline(view.x_range(), origin.y + y as f32 * cell, stroke);
        }
    }
    painter.rect_stroke(canvas.rect(), 0.0, Stroke::new(1.0, Color32::from_white_alpha(40)), StrokeKind::Outside);
}

/// The other pane's pointer, shown here on the same tile
fn draw_cursor(painter: &Painter, canvas: &Canvas, at: (usize, usize)) {
    if at.0 < canvas.width && at.1 < canvas.height {
        let r = canvas.tile(at.0, at.1);
        painter.rect_stroke(r, 0.0, Stroke::new(3.0, Color32::from_black_alpha(160)), StrokeKind::Inside);
        painter.rect_stroke(r, 0.0, Stroke::new(1.5, Color32::WHITE), StrokeKind::Inside);
    }
}

/// A pane's scroll area. With the reference shown, both panes are held to one scroll
/// position; whichever was scrolled gives its own to the other.
/// Returns what the contents return, and where the view begins on the screen.
fn scrolled<R>(ui: &mut Ui, id: &str, scroll: &mut Vec2, synced: bool, contents: impl FnOnce(&mut Ui) -> R) -> (R, f32) {
    let mut area = egui::ScrollArea::both().id_salt(id).auto_shrink(false);
    if synced {
        area = area.scroll_offset(scroll.max(Vec2::ZERO));
    }
    let out = area.show(ui, contents);
    // How far this pane can scroll. Held to a position past its own end, a pane stops at
    // its end: that's no scrolling of its own. (And a pane whose content is smaller than
    // its view reports an offset below zero: no scrolling either.)
    let reach = (out.content_size - out.inner_rect.size()).max(Vec2::ZERO);
    let offset = out.state.offset.min(reach).max(Vec2::ZERO);
    let held = if synced { scroll.min(reach).max(Vec2::ZERO) } else { *scroll };
    if (offset - held).length() > 0.5 {
        *scroll = offset;
        if synced {
            ui.ctx().request_repaint();
        }
    }
    (out.inner, out.inner_rect.top())
}

pub fn show(ui: &mut Ui, doc: &mut Doc, settings: &mut Settings) {
    let Some((width, height, pair)) = doc.map.as_ref().map(|m| (m.width, m.height, m.pair)) else {
        ui.centered_and_justified(|ui| ui.weak("No map"));
        return;
    };
    let ppp = ui.pixels_per_point();
    let cell = METATILE as f32 * scale_for(settings.zoom, ppp);
    let size = vec2(width as f32, height as f32) * cell;
    let ctx = ui.ctx().clone();
    let tiles = Tiles::of(doc.sheet(&ctx, pair));
    let synced = settings.show_reference && doc.reference.is_some();
    let mut scroll = doc.scroll;

    let (_, top) = scrolled(ui, "map", &mut scroll, synced, |ui| {
        // a map smaller than the view sits in its middle
        let margin = ((ui.available_size() - size) / 2.0).max(Vec2::splat(12.0)).floor();
        let (response, painter) = ui.allocate_painter(size + margin * 2.0, Sense::click_and_drag());
        let canvas = Canvas { origin: snap(response.rect.min + margin, ppp), cell, width, height };
        let pointer = pointer(ui, &response, &canvas, &mut settings.zoom, &mut doc.zoom_wheel);

        match pointer.over {
            Some(at) if pointer.pressed && pointer.primary => doc.stroke_to(at, settings.tool, settings.elevation, settings.auto_collision),
            _ => doc.end_stroke(),
        }
        match pointer.over {
            Some(at) if pointer.pressed && pointer.secondary => {
                let at = canvas.clamped(at);
                doc.picking = Some((doc.picking.map_or(at, |(start, _)| start), at));
            }
            _ => {
                if let (Some((a, b)), Some(map)) = (doc.picking.take(), doc.map.as_ref()) {
                    doc.stamp = Stamp::from_map(map, a, b);
                    doc.scroll_picker = true;
                }
            }
        }
        let Some(map) = doc.map.as_ref() else { return };
        doc.hover = pointer.over.filter(|&c| canvas.holds(c)).map(|c| Hover {
            x: c.0 as usize,
            y: c.1 as usize,
            block: map.blocks[c.1 as usize * width + c.0 as usize],
        });
        match doc.hover {
            Some(h) => {
                doc.cursor = Some((false, h.x, h.y));
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            None if matches!(doc.cursor, Some((false, ..))) => doc.cursor = None,
            None => {}
        }

        draw(&painter, &canvas, map, tiles, settings);

        // -- what the pointer would do ----------------------------------------------
        let outline = |a: (usize, usize), b: (usize, usize)| {
            painter.rect_stroke(canvas.tiles(a, b), 0.0, Stroke::new(2.0, ACCENT), StrokeKind::Inside);
        };
        if let Some((a, b)) = doc.picking {
            outline(a, b);
        } else if let (Some(h), None) = (doc.hover, doc.stroke.as_ref()) {
            match settings.tool {
                Tool::Paint => {
                    // the stamp, see-through, where it would land
                    let mut ghost = Mesh::with_texture(tiles.texture);
                    for dy in 0..doc.stamp.height {
                        for dx in 0..doc.stamp.width {
                            let id = doc.stamp.cell(dx as isize, dy as isize).metatile as usize;
                            if h.x + dx < width && h.y + dy < height && id < tiles.count {
                                ghost.add_rect_with_uv(
                                    canvas.tile(h.x + dx, h.y + dy),
                                    sheet_uv(id, tiles.rows),
                                    Color32::from_white_alpha(170),
                                );
                            }
                        }
                    }
                    painter.add(Shape::mesh(ghost));
                    outline((h.x, h.y), ((h.x + doc.stamp.width - 1).min(width - 1), (h.y + doc.stamp.height - 1).min(height - 1)));
                }
                _ => outline((h.x, h.y), (h.x, h.y)),
            }
        }
        if let (true, Some((true, x, y))) = (synced, doc.cursor) {
            draw_cursor(&painter, &canvas, (x, y));
        }
    });
    doc.scroll = scroll;
    doc.map_top = top;
}

/// The reference's map: the same view of another map, read-only. A right-click or a
/// right-drag takes what's there as the stamp.
pub fn show_reference(ui: &mut Ui, doc: &mut Doc, settings: &mut Settings) {
    let ctx = ui.ctx().clone();
    let ppp = ui.pixels_per_point();
    let cell = METATILE as f32 * scale_for(settings.zoom, ppp);
    let Some(reference) = doc.reference.as_mut() else { return };
    let Some((width, height, pair)) = reference.map.as_ref().map(|m| (m.width, m.height, m.pair)) else {
        reference.hover = None;
        ui.centered_and_justified(|ui| ui.weak("No map of the reference goes with this one.\nChoose one above."));
        return;
    };
    let size = vec2(width as f32, height as f32) * cell;
    let tiles = Tiles::of(reference.sheet(&ctx, pair));
    let mut scroll = doc.scroll;

    let (picked, top) = scrolled(ui, "reference map", &mut scroll, true, |ui| {
        let margin = ((ui.available_size() - size) / 2.0).max(Vec2::splat(12.0)).floor();
        let (response, painter) = ui.allocate_painter(size + margin * 2.0, Sense::click_and_drag());
        let canvas = Canvas { origin: snap(response.rect.min + margin, ppp), cell, width, height };
        let pointer = pointer(ui, &response, &canvas, &mut settings.zoom, &mut doc.zoom_wheel);

        let mut picked = None;
        match pointer.over {
            Some(at) if pointer.pressed && pointer.secondary => {
                let at = canvas.clamped(at);
                reference.picking = Some((reference.picking.map_or(at, |(start, _)| start), at));
            }
            _ => picked = reference.picking.take(),
        }
        let map = reference.map.as_ref()?;
        reference.hover = pointer.over.filter(|&c| canvas.holds(c)).map(|c| Hover {
            x: c.0 as usize,
            y: c.1 as usize,
            block: map.blocks[c.1 as usize * width + c.0 as usize],
        });
        match reference.hover {
            Some(h) => doc.cursor = Some((true, h.x, h.y)),
            None if matches!(doc.cursor, Some((true, ..))) => doc.cursor = None,
            None => {}
        }

        draw(&painter, &canvas, map, tiles, settings);

        if let Some((a, b)) = reference.picking {
            painter.rect_stroke(canvas.tiles(a, b), 0.0, Stroke::new(2.0, ACCENT), StrokeKind::Inside);
        } else if let Some(h) = reference.hover {
            painter.rect_stroke(canvas.tile(h.x, h.y), 0.0, Stroke::new(1.5, Color32::WHITE), StrokeKind::Inside);
        }
        if let Some((false, x, y)) = doc.cursor {
            draw_cursor(&painter, &canvas, (x, y));
        }
        picked
    });
    doc.scroll = scroll;
    doc.reference_top = top;
    if let Some((a, b)) = picked {
        doc.pick_from_reference(&ctx, a, b);
    }
}
