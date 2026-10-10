//! The map: drawn from the metatile sheet, painted with the pointer.

use crate::app::Settings;
use crate::doc::{sheet_uv, Doc, Hover, Stamp, Tool};
use egui::{pos2, vec2, Align2, Color32, FontId, Mesh, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
use gbmapping_core::format::block;
use gbmapping_core::gfx::METATILE;

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

pub fn show(ui: &mut Ui, doc: &mut Doc, settings: &mut Settings) {
    let Some((width, height, pair)) = doc.map.as_ref().map(|m| (m.width, m.height, m.pair)) else {
        ui.centered_and_justified(|ui| ui.weak("No map"));
        return;
    };
    let ppp = ui.pixels_per_point();
    let cell = METATILE as f32 * scale_for(settings.zoom, ppp);
    let size = vec2(width as f32 * cell, height as f32 * cell);
    let ctx = ui.ctx().clone();
    let (sheet_texture, sheet_count, sheet_rows) = {
        let sheet = doc.sheet(&ctx, pair);
        (sheet.texture.id(), sheet.count, sheet.rows)
    };

    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        // a map smaller than the view sits in its middle
        let margin = ((ui.available_size() - size) / 2.0).max(Vec2::splat(12.0)).floor();
        let (response, painter) = ui.allocate_painter(size + margin * 2.0, Sense::click_and_drag());
        let origin = snap(response.rect.min + margin, ppp);
        let map_rect = Rect::from_min_size(origin, size);
        let cell_at = |p: Pos2| (((p.x - origin.x) / cell).floor() as isize, ((p.y - origin.y) / cell).floor() as isize);
        let inside = |c: (isize, isize)| c.0 >= 0 && c.1 >= 0 && (c.0 as usize) < width && (c.1 as usize) < height;
        let clamped = |c: (isize, isize)| (c.0.clamp(0, width as isize - 1) as usize, c.1.clamp(0, height as isize - 1) as usize);

        // -- the pointer ---------------------------------------------------------
        let (pointer, primary, secondary, middle, delta, zoom_delta) = ui.input(|i| {
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
        let over = pointer.filter(|_| response.contains_pointer() || pressed).map(cell_at);

        match over {
            Some(at) if pressed && primary => doc.stroke_to(at, settings.tool, settings.elevation, settings.auto_collision),
            _ => doc.end_stroke(),
        }
        match over {
            Some(at) if pressed && secondary => {
                let at = clamped(at);
                doc.picking = Some((doc.picking.map_or(at, |(start, _)| start), at));
            }
            _ => {
                if let (Some((a, b)), Some(map)) = (doc.picking.take(), doc.map.as_ref()) {
                    doc.stamp = Stamp::from_map(map, a, b);
                    doc.scroll_picker = true;
                }
            }
        }
        if pressed && middle {
            ui.scroll_with_delta(delta);
        }
        if response.contains_pointer() && zoom_delta != 1.0 {
            // Ctrl + wheel, or a pinch: a zoom level per notch
            doc.zoom_wheel += zoom_delta.ln();
            if doc.zoom_wheel.abs() > 0.2 {
                settings.zoom = (settings.zoom as i32 + doc.zoom_wheel.signum() as i32).clamp(1, MAX_ZOOM as i32) as u32;
                doc.zoom_wheel = 0.0;
            }
        }
        let Some(map) = doc.map.as_ref() else { return };
        doc.hover = over.filter(|&c| inside(c)).map(|c| Hover {
            x: c.0 as usize,
            y: c.1 as usize,
            block: map.blocks[c.1 as usize * width + c.0 as usize],
        });
        if doc.hover.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }

        // -- the tiles in view -----------------------------------------------------
        let view = painter.clip_rect().intersect(map_rect);
        if view.is_negative() {
            return;
        }
        let x0 = ((view.min.x - origin.x) / cell).floor().max(0.0) as usize;
        let y0 = ((view.min.y - origin.y) / cell).floor().max(0.0) as usize;
        let x1 = (((view.max.x - origin.x) / cell).ceil() as usize).min(width);
        let y1 = (((view.max.y - origin.y) / cell).ceil() as usize).min(height);
        let rect_of = |x: usize, y: usize| Rect::from_min_size(origin + vec2(x as f32 * cell, y as f32 * cell), Vec2::splat(cell));
        let uv_of = |id: usize| sheet_uv(id, sheet_rows);

        let mut tiles = Mesh::with_texture(sheet_texture);
        let mut overlay = Mesh::default();
        for y in y0..y1 {
            for x in x0..x1 {
                let b = map.blocks[y * width + x];
                let id = block::metatile(b) as usize;
                if id < sheet_count {
                    tiles.add_rect_with_uv(rect_of(x, y), uv_of(id), Color32::WHITE);
                } else {
                    overlay.add_colored_rect(rect_of(x, y), MISSING);
                }
                if settings.show_collision && block::collision(b) != 0 {
                    overlay.add_colored_rect(rect_of(x, y), Color32::from_rgba_unmultiplied(229, 60, 45, 115));
                }
            }
        }
        painter.add(Shape::mesh(tiles));
        painter.add(Shape::mesh(overlay));

        if settings.show_elevation {
            let font = FontId::monospace((cell * 0.45).clamp(7.0, 14.0));
            let tag = Vec2::splat((cell * 0.5).clamp(8.0, 18.0));
            for y in y0..y1 {
                for x in x0..x1 {
                    let e = block::elevation(map.blocks[y * width + x]);
                    let r = Rect::from_min_size(rect_of(x, y).min, tag);
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
        painter.rect_stroke(map_rect, 0.0, Stroke::new(1.0, Color32::from_white_alpha(40)), StrokeKind::Outside);

        // -- what the pointer would do ----------------------------------------------
        let outline = |a: (usize, usize), b: (usize, usize)| {
            let r = rect_of(a.0.min(b.0), a.1.min(b.1)).union(rect_of(a.0.max(b.0), a.1.max(b.1))).intersect(map_rect);
            painter.rect_stroke(r, 0.0, Stroke::new(2.0, ACCENT), StrokeKind::Inside);
        };
        if let Some((a, b)) = doc.picking {
            outline(a, b);
        } else if let (Some(h), None) = (doc.hover, doc.stroke.as_ref()) {
            match settings.tool {
                Tool::Paint => {
                    // the stamp, see-through, where it would land
                    let mut ghost = Mesh::with_texture(sheet_texture);
                    for dy in 0..doc.stamp.height {
                        for dx in 0..doc.stamp.width {
                            let id = doc.stamp.cell(dx as isize, dy as isize).metatile as usize;
                            if h.x + dx < width && h.y + dy < height && id < sheet_count {
                                ghost.add_rect_with_uv(rect_of(h.x + dx, h.y + dy), uv_of(id), Color32::from_white_alpha(170));
                            }
                        }
                    }
                    painter.add(Shape::mesh(ghost));
                    outline((h.x, h.y), ((h.x + doc.stamp.width - 1).min(width - 1), (h.y + doc.stamp.height - 1).min(height - 1)));
                }
                _ => outline((h.x, h.y), (h.x, h.y)),
            }
        }
    });
}
