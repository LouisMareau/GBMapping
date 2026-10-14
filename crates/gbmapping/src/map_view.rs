//! The map: drawn from the metatile sheet, painted with the pointer. And the reference
//! next to it: another map drawn the same way, only looked at and picked from.

use crate::app::Settings;
use crate::doc::{sheet_uv, Doc, Hover, Sheet, Tool};
use crate::events;
use egui::{pos2, vec2, Align2, Color32, FontId, Mesh, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, TextureId, Ui, Vec2};
use gbmapping_core::events::List;
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

/// The tiles in view, with the overlays that are on. `grain`: how many tiles a side the
/// game's own unit is; the grid is of those, and only maps of single tiles have a
/// collision and an elevation of their own to show.
fn draw(painter: &Painter, canvas: &Canvas, map: &Map, tiles: Tiles, settings: &Settings, grain: usize) {
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
            if settings.show_collision && grain == 1 && block::collision(b) != 0 {
                overlay.add_colored_rect(canvas.tile(x, y), Color32::from_rgba_unmultiplied(229, 60, 45, 115));
            }
        }
    }
    painter.add(Shape::mesh(mesh));
    painter.add(Shape::mesh(overlay));

    if settings.show_elevation && grain == 1 {
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
        for x in (x0..=x1).filter(|x| x % grain == 0) {
            painter.vline(origin.x + x as f32 * cell, view.y_range(), stroke);
        }
        for y in (y0..=y1).filter(|y| y % grain == 0) {
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
    let grain = doc.grain();
    // a map painted by blocks has no collision or elevation of its own to set (nor events read yet)
    let tool =
        if grain > 1 && matches!(settings.tool, Tool::Collision | Tool::Elevation | Tool::Events) { Tool::Paint } else { settings.tool };
    // in the Events mode, the events' panel is where the reference is
    let synced = settings.show_reference && doc.reference.is_some() && tool != Tool::Events;
    let mut scroll = doc.scroll;
    let mut go_to = None;

    let (_, top) = scrolled(ui, "map", &mut scroll, synced, |ui| {
        // a map smaller than the view sits in its middle
        let margin = ((ui.available_size() - size) / 2.0).max(Vec2::splat(12.0)).floor();
        let (response, painter) = ui.allocate_painter(size + margin * 2.0, Sense::click_and_drag());
        let canvas = Canvas { origin: snap(response.rect.min + margin, ppp), cell, width, height };
        let pointer = pointer(ui, &response, &canvas, &mut settings.zoom, &mut doc.zoom_wheel);

        if tool == Tool::Events {
            go_to = events_pointer(&response, &canvas, &pointer, doc);
        } else {
            match pointer.over {
                Some(at) if pointer.pressed && pointer.primary => doc.stroke_to(at, tool, settings.elevation, settings.auto_collision),
                _ => doc.end_stroke(),
            }
        }
        match pointer.over {
            Some(at) if pointer.pressed && pointer.secondary && tool != Tool::Events => {
                let at = canvas.clamped(at);
                doc.picking = Some((doc.picking.map_or(at, |(start, _)| start), at));
            }
            _ => {
                if let Some((a, b)) = doc.picking.take() {
                    doc.pick_tiles(a, b);
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

        draw(&painter, &canvas, map, tiles, settings, grain);
        if tool == Tool::Events {
            draw_events(&painter, &canvas, doc, &ctx);
            return;
        }

        // -- what the pointer would do ----------------------------------------------
        let outline = |a: (usize, usize), b: (usize, usize)| {
            painter.rect_stroke(canvas.tiles(a, b), 0.0, Stroke::new(2.0, ACCENT), StrokeKind::Inside);
        };
        if let Some((a, b)) = doc.picking {
            let (a, b) = doc.whole(a, b);
            outline(a, (b.0.min(width - 1), b.1.min(height - 1)));
        } else if let (Some(h), None) = (doc.hover, doc.stroke.as_ref()) {
            // whole units: from the corner of the one the pointer is over
            let (x, y) = (h.x / grain * grain, h.y / grain * grain);
            match tool {
                Tool::Paint => {
                    // the stamp, see-through, where it would land
                    let mut ghost = Mesh::with_texture(tiles.texture);
                    for dy in 0..doc.stamp.height {
                        for dx in 0..doc.stamp.width {
                            let id = doc.stamp.cell(dx as isize, dy as isize).metatile as usize;
                            if x + dx < width && y + dy < height && id < tiles.count {
                                ghost.add_rect_with_uv(
                                    canvas.tile(x + dx, y + dy),
                                    sheet_uv(id, tiles.rows),
                                    Color32::from_white_alpha(170),
                                );
                            }
                        }
                    }
                    painter.add(Shape::mesh(ghost));
                    outline((x, y), ((x + doc.stamp.width - 1).min(width - 1), (y + doc.stamp.height - 1).min(height - 1)));
                }
                _ => outline((x, y), ((x + grain - 1).min(width - 1), (y + grain - 1).min(height - 1))),
            }
        }
        if let (true, Some((true, x, y))) = (synced, doc.cursor) {
            draw_cursor(&painter, &canvas, (x, y));
        }
    });
    doc.scroll = scroll;
    doc.map_top = top;
    // a warp double-clicked: its map, its warp selected
    if let Some((index, warp)) = go_to {
        doc.open_map(index);
        if doc.events().is_some_and(|e| warp < e.warps.len()) {
            doc.events.selected = Some((List::Warps, warp));
        }
    }
}

/// The Events mode's pointer: a press picks the event under it (or the tile for a new
/// one), a drag moves the event picked, a click on the one picked picks the next on its
/// tile, a double-click on a warp asks to go where it leads (the map and its warp, returned)
fn events_pointer(response: &egui::Response, canvas: &Canvas, pointer: &Pointer, doc: &mut Doc) -> Option<(usize, usize)> {
    let events = doc.events()?.clone();
    let tile = pointer.over.filter(|&c| canvas.holds(c)).map(|(x, y)| (x as i16, y as i16));
    match (pointer.pressed && pointer.primary, doc.events.dragging) {
        // a press begins: what's there is picked, the one picked already if it's there
        (true, None) => {
            let (x, y) = tile?;
            let here = events.at(x, y);
            doc.events.pressed_on = doc.events.selected.filter(|s| here.contains(s));
            let picked = doc.events.pressed_on.or(here.first().copied());
            doc.events.selected = picked;
            if picked.is_none() {
                doc.events.add_at = Some((x, y));
            }
            doc.events.dragging = Some(false);
        }
        // held: the event picked follows the pointer, a tile at a time
        (true, Some(_)) => {
            let ((x, y), (list, i)) = (tile?, doc.events.selected?);
            if events.place(list, i) != Some((x, y)) {
                let mut moved = events.clone();
                moved.move_to(list, i, x, y);
                doc.write_events(moved);
                doc.events.dragging = Some(true);
            }
        }
        (false, Some(moved)) => {
            doc.events.dragging = None;
            // a click on the one picked already: the next one on its tile
            if let (false, Some(was), Some((x, y))) = (moved, doc.events.pressed_on.take(), tile) {
                let here = events.at(x, y);
                if let Some(k) = here.iter().position(|&s| s == was).filter(|_| here.len() > 1) {
                    doc.events.selected = Some(here[(k + 1) % here.len()]);
                }
            }
            doc.commit_events(false);
        }
        (false, None) => {}
    }
    match doc.events.selected {
        Some((List::Warps, i)) if response.double_clicked() => {
            let w = events.warps.get(i)?;
            let index = doc.game.map_index(gbmapping_core::MapId { group: w.group, num: w.num })?;
            Some((index, w.warp as usize))
        }
        _ => None,
    }
}

/// The map's events over it: people as the game draws them (their feet on their tile),
/// the rest as tinted tiles marked with a letter; the one picked outlined
fn draw_events(painter: &Painter, canvas: &Canvas, doc: &mut Doc, ctx: &egui::Context) {
    let Some(events) = doc.events().cloned() else { return };
    let cell = canvas.cell;
    let inside = |(x, y): (i16, i16)| x >= 0 && y >= 0 && (x as usize) < canvas.width && (y as usize) < canvas.height;
    let font = FontId::proportional((cell * 0.45).clamp(8.0, 15.0));
    let mark = |list: List, i: usize, rect: Rect| {
        let color = events::color(list, &events, i);
        painter.rect_filled(rect.shrink(1.0), 2.0, color.gamma_multiply(0.45));
        painter.rect_stroke(rect.shrink(1.0), 2.0, Stroke::new(1.5, color), StrokeKind::Inside);
        if cell >= 12.0 {
            painter.text(
                rect.left_top() + vec2(3.0, 1.0),
                Align2::LEFT_TOP,
                events::letter(list, &events, i),
                font.clone(),
                Color32::WHITE,
            );
        }
    };
    for list in [List::Warps, List::Triggers, List::Signs] {
        for i in 0..events.len(list) {
            let Some(place) = events.place(list, i).filter(|&p| inside(p)) else { continue };
            mark(list, i, canvas.tile(place.0 as usize, place.1 as usize));
        }
    }
    // the people, those further down over those further up
    let mut people: Vec<usize> = (0..events.objects.len()).collect();
    people.sort_by_key(|&i| events.objects[i].y);
    let scale = cell / METATILE as f32;
    let mut outlines = Vec::new();
    for i in people {
        let o = &events.objects[i];
        if !inside((o.x, o.y)) {
            continue;
        }
        let rect = canvas.tile(o.x as usize, o.y as usize);
        painter.rect_stroke(rect.shrink(1.0), 2.0, Stroke::new(1.0, events::OBJECT.gamma_multiply(0.7)), StrokeKind::Inside);
        match doc.sprite(ctx, o.graphics) {
            Some((texture, size)) => {
                let size = size * scale;
                let at = Rect::from_min_size(pos2(rect.center().x - size.x / 2.0, rect.max.y - size.y), size);
                painter.image(texture, at, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                outlines.push((i, at));
            }
            None => mark(List::Objects, i, rect),
        }
    }
    // the one picked, and where a new one would go
    if let Some((list, i)) = doc.events.selected {
        if let Some(place) = events.place(list, i).filter(|&p| inside(p)) {
            let rect = canvas.tile(place.0 as usize, place.1 as usize);
            painter.rect_stroke(rect, 0.0, Stroke::new(3.0, Color32::from_black_alpha(170)), StrokeKind::Outside);
            painter.rect_stroke(rect, 0.0, Stroke::new(2.0, Color32::WHITE), StrokeKind::Outside);
            if let Some((_, sprite)) = outlines.iter().find(|(o, _)| list == List::Objects && *o == i) {
                painter.rect_stroke(*sprite, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Outside);
            }
        }
    } else if let Some(place) = doc.events.add_at.filter(|&p| inside(p)) {
        let rect = canvas.tile(place.0 as usize, place.1 as usize);
        painter.rect_stroke(rect, 0.0, Stroke::new(1.5, Color32::from_white_alpha(160)), StrokeKind::Inside);
    }
    if let Some(h) = doc.hover {
        painter.rect_stroke(canvas.tile(h.x, h.y), 0.0, Stroke::new(1.0, Color32::from_white_alpha(120)), StrokeKind::Inside);
    }
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

        draw(&painter, &canvas, map, tiles, settings, reference.game.format.block_size());

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
