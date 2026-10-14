//! The Anim tab: the animations of the map's tiles, frame by frame.
//!
//! The doors that open, and the tiles a tileset keeps changing (water, flowers): their
//! frames side by side, and one of them large to draw on, the one before it showing
//! through when asked. Played, the large one goes from frame to frame. See
//! `gbmapping_core::anims`.

use crate::doc::{texture, Doc};
use crate::draw::{self, Layers};
use crate::map_view::snap;
use crate::side::WARNING;
use egui::{pos2, vec2, Color32, Rect, Sense, Stroke, StrokeKind, TextureHandle, Ui};
use gbmapping_core::anims::{Animation, Kind};
use gbmapping_core::gfx::Palette;
use gbmapping_core::{Game, Image, Pair};
use std::rc::Rc;

const ACCENT: Color32 = Color32::from_rgb(76, 175, 80);
const GROUND: Color32 = Color32::from_gray(17);
/// How many frames a second an animation is played at: slow enough to see each
const SPEED: f64 = 4.0;
/// How much of the frame drawn on the one before hides
const GHOST: f32 = 0.45;

/// The frames of an animation side by side, as last drawn
struct Atlas {
    /// The animation, the pair it's drawn with, and when (see `Anim::stamp`)
    of: (usize, Pair, u64),
    texture: TextureHandle,
}

/// The animations listed, and what for: a map's tilesets, with or without all the others
struct Listed {
    wanted: (Pair, bool),
    list: Rc<Vec<usize>>,
}

/// What the tab keeps from one frame of the screen to the next
#[derive(Default)]
pub struct Anim {
    /// Every animation of the ROM, once looked for
    found: Option<Rc<Vec<Animation>>>,
    /// Those listed, and for what: a map's tilesets, or all
    listed: Option<Listed>,
    /// Every animation of the ROM is listed, not only those of the map's tilesets
    pub all: bool,
    pub selected: usize,
    pub frame: usize,
    /// The frame's tile last pointed at: the palette shown is its
    pub tile: usize,
    /// The frame before shows through the one drawn on
    pub ghost: bool,
    pub playing: bool,
    /// Goes up whenever a frame may look different: what was drawn of them is drawn again
    stamp: u64,
    atlas: Option<Atlas>,
}

impl Anim {
    /// The tilesets were written to: the frames may look different, and be other maps'
    pub fn changed(&mut self) {
        self.stamp += 1;
        self.listed = None;
    }
}

impl Doc {
    /// Every animation of the ROM's tiles. Looking for them goes through the whole ROM: once.
    pub fn animations(&mut self) -> Rc<Vec<Animation>> {
        let game = &self.game;
        self.anim.found.get_or_insert_with(|| Rc::new(game.animations())).clone()
    }

    /// The animations to list for a map: those of its two tilesets' tiles, and the doors
    /// that one of its metatiles is. Or all of them.
    fn listed_animations(&mut self, pair: Pair) -> Rc<Vec<usize>> {
        let found = self.animations();
        let wanted = (pair, self.anim.all);
        if let Some(made) = self.anim.listed.as_ref().filter(|made| made.wanted == wanted) {
            return made.list.clone();
        }
        let listed: Rc<Vec<usize>> = Rc::new((0..found.len()).filter(|&i| wanted.1 || self.game.animates(&found[i], pair)).collect());
        self.anim.listed = Some(Listed { wanted, list: listed.clone() });
        listed
    }
}

/// The frames of an animation, one after the other from left to right
fn atlas(game: &Game, animation: &Animation, tile_palettes: &[u8], palettes: &[Palette]) -> Image {
    let (width, height) = animation.size();
    let mut image = Image::new(width * animation.frames.len().max(1), height);
    for frame in 0..animation.frames.len() {
        for (i, &(color, palette)) in game.frame_pixels(animation, frame, tile_palettes).iter().enumerate() {
            if color != 0 {
                let o = ((i / width) * image.width + frame * width + i % width) * 4;
                let [r, g, b] = palettes.get(palette as usize).map_or([255, 0, 255], |colors| colors[color as usize & 0xF]);
                image.rgba[o..o + 4].copy_from_slice(&[r, g, b, 255]);
            }
        }
    }
    image
}

/// The frames in the order they're played: a door opens, stays open, and closes; tiles
/// go round as the game has them
fn played(animation: &Animation) -> Vec<usize> {
    let count = animation.frames.len();
    match animation.kind {
        Kind::Door { .. } => (0..count).chain([count - 1; 2]).chain((0..count).rev()).chain([0; 2]).collect(),
        Kind::Tiles { .. } if animation.sequence.is_empty() => (0..count).collect(),
        Kind::Tiles { .. } => animation.sequence.clone(),
    }
}

/// What to call the tileset an animation's tiles are of. One no map is drawn with is
/// there for the maps' originals.
fn whose(doc: &Doc, tileset: usize) -> String {
    let drawn_with = doc.game.maps.iter().any(|m| m.layout.primary == tileset || m.layout.secondary == tileset);
    if drawn_with {
        doc.tileset_label(tileset)
    } else {
        format!("0x{tileset:06X} · the maps' originals")
    }
}

pub fn show(ui: &mut Ui, doc: &mut Doc, pair: Pair) {
    let ctx = ui.ctx().clone();
    let ppp = ui.pixels_per_point();
    let small = |text: String| egui::RichText::new(text).small();
    let found = doc.animations();
    if found.is_empty() {
        ui.label("No animation of tiles was found in this ROM.");
        ui.weak("The doors that open and the tiles that change (water, flowers) are looked for by the shape of their data.");
        return;
    }

    // -- which animation ---------------------------------------------------------------------
    ui.checkbox(&mut doc.anim.all, "All of the ROM's").on_hover_text(
        "Off: the animations of this map's two tilesets, and the doors its metatiles are.\nOn: every animation found in the ROM.",
    );
    let listed = doc.listed_animations(pair);
    if listed.is_empty() {
        ui.label(small("None of this map's tiles are animated, and none of its metatiles is a door that opens.".to_string()));
        return;
    }
    if !listed.contains(&doc.anim.selected) {
        (doc.anim.selected, doc.anim.frame) = (listed[0], 0);
    }
    egui::ComboBox::from_id_salt("animation")
        .selected_text(found[doc.anim.selected].label())
        .width(ui.available_width() - 8.0)
        .height(420.0)
        .truncate()
        .show_ui(ui, |ui| {
            // each under the tileset it's of, as the maps are under their group
            let mut under = String::new();
            for &i in listed.iter() {
                let heading = match found[i].kind {
                    Kind::Tiles { tileset, .. } => whose(doc, tileset),
                    Kind::Door { .. } => format!("Doors · the table at 0x{:06X}", found[i].table),
                };
                if heading != under {
                    ui.label(egui::RichText::new(&heading).small().weak());
                    under = heading;
                }
                if ui.selectable_label(i == doc.anim.selected, found[i].label()).clicked() {
                    (doc.anim.selected, doc.anim.frame) = (i, 0);
                }
            }
        });
    let index = doc.anim.selected;
    let animation = &found[index];
    let count = animation.frames.len();
    let (width, height) = animation.size();

    // what it's drawn with: the pair its tileset is in, the palettes its tiles are drawn in
    let with = doc.game.animation_pair(animation, pair);
    let usage = doc.tile_usage();
    let tile_palettes = doc.game.animation_palettes(animation, &usage);
    let palettes = doc.game.palettes(with);
    let how = match animation.kind {
        Kind::Door { .. } => "one after the other as the door opens, and back as it closes".to_string(),
        Kind::Tiles { .. } => {
            let order: Vec<String> = animation.sequence.iter().map(|f| (f + 1).to_string()).collect();
            format!("over and over, in the order {}", order.join(" "))
        }
    };
    let tiles = if animation.tiles == 1 { "1 tile".to_string() } else { format!("{} tiles", animation.tiles) };
    // the same frames may go over several sets of tiles, one after the other: flowers that don't all move together
    let places = match animation.also.len() {
        0 => String::new(),
        more => format!(" They go over {} sets of tiles of the tileset, not all at the same time.", more + 1),
    };
    ui.label(
        small(format!("{count} frames of {tiles}, at 0x{:06X} in the ROM. The game shows them {how}.{places}", animation.frames[0])).weak(),
    );

    // -- played: the frame shown is the one the time says -----------------------------------
    if doc.anim.playing {
        let order = played(animation);
        doc.anim.frame = order[(ui.input(|i| i.time) * SPEED) as usize % order.len()];
        ctx.request_repaint_after(std::time::Duration::from_millis(40));
    }
    let mut frame = doc.anim.frame.min(count - 1);

    // -- its frames, side by side: click one to draw on it ---------------------------------------
    let of = (index, with, doc.anim.stamp);
    if doc.anim.atlas.as_ref().is_none_or(|atlas| atlas.of != of) {
        let image = atlas(&doc.game, animation, &tile_palettes, &palettes);
        doc.anim.atlas = Some(Atlas { of, texture: texture(&ctx, "frames", &image) });
    }
    let Some(frames) = doc.anim.atlas.as_ref().map(|atlas| atlas.texture.id()) else { return };
    let picture = |frame: usize| Rect::from_min_size(pos2(frame as f32 / count as f32, 0.0), vec2(1.0 / count as f32, 1.0));
    let scale = if width <= 32 { 3.0 } else { 2.0 };
    let (each, gap) = (vec2(width as f32, height as f32) * scale, 6.0);
    // room around them for the outline of the one drawn on
    let room = vec2(2.0, 2.0);
    let across = (((ui.available_width() - room.x * 2.0 + gap) / (each.x + gap)).floor() as usize).clamp(1, count);
    let rows = count.div_ceil(across);
    let all = vec2(across as f32 * (each.x + gap) - gap, rows as f32 * (each.y + gap) - gap);
    let (response, painter) = ui.allocate_painter(all + room * 2.0, Sense::click());
    let origin = snap(response.rect.min + room, ppp);
    let place = |frame: usize| {
        Rect::from_min_size(origin + vec2((frame % across) as f32 * (each.x + gap), (frame / across) as f32 * (each.y + gap)), each)
    };
    for f in 0..count {
        painter.rect_filled(place(f), 0.0, GROUND);
        painter.image(frames, place(f), picture(f), Color32::WHITE);
        let outline = if f == frame { Stroke::new(2.0, ACCENT) } else { Stroke::new(1.0, Color32::from_gray(70)) };
        painter.rect_stroke(place(f), 0.0, outline, StrokeKind::Outside);
    }
    if let Some(p) = response.interact_pointer_pos().filter(|_| response.clicked()) {
        if let Some(clicked) = (0..count).find(|&f| place(f).contains(p)) {
            (frame, doc.anim.playing) = (clicked, false);
        }
    }

    // -- from frame to frame ---------------------------------------------------------------------
    ui.horizontal(|ui| {
        if ui.button("◀").on_hover_text("The frame before").clicked() {
            (frame, doc.anim.playing) = ((frame + count - 1) % count, false);
        }
        if ui.button("▶").on_hover_text("The frame after").clicked() {
            (frame, doc.anim.playing) = ((frame + 1) % count, false);
        }
        ui.toggle_value(&mut doc.anim.playing, "Play")
            .on_hover_text("Goes from frame to frame, in the order the game shows them. Drawing stops it.");
        ui.checkbox(&mut doc.anim.ghost, "Ghost")
            .on_hover_text("The frame before shows through the one drawn on: what moves from one to the next is seen.");
        ui.label(small(format!("{} / {count}", frame + 1)));
    });
    draw::pens(ui, &mut doc.draw.pen);

    // -- the frame, large, the one before it over it as a ghost --------------------------------
    let pixels = doc.game.frame_pixels(animation, frame, &tile_palettes);
    // a door's first frame has none before; tiles that go round have their last
    let before = match animation.kind {
        Kind::Door { .. } => frame.checked_sub(1),
        Kind::Tiles { .. } => Some((frame + count - 1) % count).filter(|&f| f != frame),
    };
    let ghost = before.filter(|_| doc.anim.ghost).map(|f| doc.game.frame_pixels(animation, f, &tile_palettes));
    let layers = Layers { width, height, under: None, pixels: &pixels, over: ghost.as_ref().map(|ghost| (&ghost[..], GHOST)) };
    let fits = (ui.available_width() / width as f32).min(300.0 / height as f32);
    let cell = ((fits * ppp).floor() / ppp).clamp(2.0, 20.0);
    let pointed = draw::canvas(ui, &layers, &palettes, cell);
    if let Some(tile) = pointed.held.or(pointed.over).and_then(|(x, y)| animation.tile_at(x, y)) {
        doc.anim.tile = tile;
    }
    // drawn on, it stays on the frame it was on
    doc.anim.playing &= pointed.held.is_none();
    let drawn = draw::stroke(&mut doc.draw, &pointed, &layers, &palettes);
    if doc.game.draw_on_frame(animation, frame, &drawn) {
        doc.anim.stamp += 1;
        ctx.request_repaint();
    }
    doc.anim.frame = frame;

    // -- the colours: the palette of the tile pointed at --------------------------------------
    ui.add_space(4.0);
    let tile = doc.anim.tile.min(animation.tiles - 1);
    let palette = tile_palettes.get(tile).copied().unwrap_or(0) as usize;
    ui.label(small(format!("Tile {} of the frame is drawn in palette {palette}", tile + 1)));
    draw::swatches(ui, &mut doc.draw, &palettes, palette);
    draw::color_button(ui, doc, with, &palettes, palette);
    let what = match &animation.kind {
        Kind::Door { metatiles, .. } if metatiles.len() > 1 => {
            format!("These frames are {} doors': drawn on, they all open that way, on every map.", metatiles.len())
        }
        Kind::Door { .. } => "Drawn on, the door opens that way on every map it's on.".to_string(),
        Kind::Tiles { .. } => "Drawn on, the tiles change that way on every map of the tileset.".to_string(),
    };
    ui.label(small(what).color(WARNING));

    // a stroke, a colour dragged: one step once the button is released
    if !ui.input(|i| i.pointer.any_down()) && doc.game.commit() {
        doc.tileset_changed();
    }
}
