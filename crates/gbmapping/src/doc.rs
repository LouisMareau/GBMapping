//! An opened ROM and what's being done to it: the map on screen, the stamp in hand,
//! the stroke under way. No drawing here.

use crate::reference::Reference;
use egui::{ColorImage, TextureHandle, TextureOptions};
use gbmapping_core::format::block;
use gbmapping_core::game::BlockUsage;
use gbmapping_core::gfx::METATILE;
use gbmapping_core::{closest_metatile, Game, Image, Map, Pair};
use std::collections::{BTreeSet, HashMap, VecDeque};

/// Metatiles per row in the picker: the games' tilesets are laid out for 8
pub const PICKER_COLUMNS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum Tool {
    Paint,
    Fill,
    Collision,
    Elevation,
}

impl Tool {
    pub const ALL: [Self; 4] = [Self::Paint, Self::Fill, Self::Collision, Self::Elevation];

    pub fn label(self) -> &'static str {
        match self {
            Self::Paint => "Paint",
            Self::Fill => "Fill",
            Self::Collision => "Collision",
            Self::Elevation => "Elevation",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Paint => "Paint the selected metatiles (P). Right-click or right-drag the map to pick what's there.",
            Self::Fill => "Fill an area of one metatile with the selection (F)",
            Self::Collision => "Drag to make tiles impassable, or passable again (K)",
            Self::Elevation => "Drag to set the tiles' elevation (L)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StampCell {
    pub metatile: u16,
    /// The collision and elevation bits, when the cell was picked from a map
    pub high: Option<u16>,
}

/// What painting puts down: one metatile or a rectangle of them
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<StampCell>,
    /// Its top-left cell in the picker, when it was taken there
    pub picker: Option<(usize, usize)>,
}

impl Stamp {
    pub fn from_picker(x0: usize, y0: usize, x1: usize, y1: usize) -> Self {
        let (x, y) = (x0.min(x1), y0.min(y1));
        let (width, height) = (x0.abs_diff(x1) + 1, y0.abs_diff(y1) + 1);
        let cells = (0..width * height)
            .map(|i| StampCell { metatile: ((y + i / width) * PICKER_COLUMNS + x + i % width) as u16, high: None })
            .collect();
        Self { width, height, cells, picker: Some((x, y)) }
    }

    pub fn from_map(map: &Map, a: (usize, usize), b: (usize, usize)) -> Self {
        let (x, y) = (a.0.min(b.0), a.1.min(b.1));
        let (width, height) = (a.0.abs_diff(b.0) + 1, a.1.abs_diff(b.1) + 1);
        let cells = (0..width * height)
            .map(|i| {
                let b = map.blocks[(y + i / width) * map.width + x + i % width];
                StampCell { metatile: block::metatile(b), high: Some(b & !block::METATILE_MASK) }
            })
            .collect();
        let mut stamp = Self { width, height, cells, picker: None };
        if width * height == 1 {
            let id = stamp.cells[0].metatile as usize;
            stamp.picker = Some((id % PICKER_COLUMNS, id / PICKER_COLUMNS));
        }
        stamp
    }

    /// The cell that lands `dx`, `dy` away from where the stamp was first put (tiled)
    pub fn cell(&self, dx: isize, dy: isize) -> StampCell {
        self.cells[dy.rem_euclid(self.height as isize) as usize * self.width + dx.rem_euclid(self.width as isize) as usize]
    }

    /// The metatile the tileset tab shows
    pub fn first(&self) -> u16 {
        self.cells[0].metatile
    }
}

/// A press of the main button on the map, until it's released
pub struct Stroke {
    pub tool: Tool,
    /// Where it began: stamps of several tiles are laid on a grid from there
    pub anchor: (isize, isize),
    pub last: (isize, isize),
    /// What the collision tool sets: the opposite of what the first tile had
    pub collision: u8,
}

/// A tileset pair's metatiles, drawn: a texture to paint maps from, and the same
/// picture to compare metatiles by their look
pub struct Sheet {
    pub texture: TextureHandle,
    pub image: Image,
    pub count: usize,
    pub rows: usize,
}

impl Sheet {
    pub fn new(ctx: &egui::Context, game: &Game, pair: Pair) -> Self {
        let image = game.render_sheet(pair, PICKER_COLUMNS);
        Self { texture: texture(ctx, "metatiles", &image), count: game.metatile_count(pair), rows: image.height / METATILE, image }
    }

    /// A metatile's picture
    pub fn picture(&self, id: u16) -> Image {
        let id = id as usize;
        self.image.crop((id % PICKER_COLUMNS) * METATILE, (id / PICKER_COLUMNS) * METATILE, METATILE, METATILE)
    }
}

/// Where a metatile is in a sheet of `rows` rows, as texture coordinates
pub fn sheet_uv(id: usize, rows: usize) -> egui::Rect {
    let (columns, rows) = (PICKER_COLUMNS as f32, rows.max(1) as f32);
    egui::Rect::from_min_size(
        egui::pos2((id % PICKER_COLUMNS) as f32 / columns, (id / PICKER_COLUMNS) as f32 / rows),
        egui::vec2(1.0 / columns, 1.0 / rows),
    )
}

pub fn texture(ctx: &egui::Context, name: &str, image: &Image) -> TextureHandle {
    ctx.load_texture(name, ColorImage::from_rgba_unmultiplied([image.width, image.height], &image.rgba), TextureOptions::NEAREST)
}

/// What the pointer is over on the map, for the status bar
#[derive(Clone, Copy)]
pub struct Hover {
    pub x: usize,
    pub y: usize,
    pub block: u16,
}

pub struct Doc {
    pub game: Game,
    pub file_name: String,
    pub map: Option<Map>,
    pub stamp: Stamp,
    pub stroke: Option<Stroke>,
    /// A right-drag on the map: the rectangle being picked
    pub picking: Option<((usize, usize), (usize, usize))>,
    pub picker_drag: Option<(usize, usize)>,
    pub hover: Option<Hover>,
    /// The metatile the pointer is over in the picker
    pub picker_hover: Option<u16>,
    /// The tile entry selected in the tileset tab (0-3: bottom layer, 4-7: top)
    pub entry: usize,
    /// The layouts changed since the ROM was opened
    pub edited: BTreeSet<usize>,
    /// The ROM changed: the edits kept in the browser are to be written again
    pub unsaved_edits: bool,
    pub scroll_picker: bool,
    pub zoom_wheel: f32,
    /// The read-only view next to the map, when there is one
    pub reference: Option<Reference>,
    /// The tile under the pointer, in the map (false) or in the reference (true): each shows the other's
    pub cursor: Option<(bool, usize, usize)>,
    /// Where the map's view is scrolled to: the reference's keeps to it
    pub scroll: egui::Vec2,
    /// Something to tell in the status line
    pub notice: Option<String>,
    /// Where the two maps' views begin on the screen, and how tall the strip over the
    /// edited map is: adjusted until both begin at the same height, tile facing tile
    pub map_top: f32,
    pub reference_top: f32,
    pub strip: f32,
    sheets: HashMap<Pair, Sheet>,
    usage: HashMap<Pair, HashMap<u16, BlockUsage>>,
    pub tiles: Option<TilesTexture>,
    pub layers: Option<LayerTextures>,
}

/// The tileset tab's sheet of tiles: a pair's tiles in one of its palettes
pub struct TilesTexture {
    pub pair: Pair,
    pub palette: usize,
    pub texture: TextureHandle,
    pub count: usize,
}

/// The tileset tab's two layers of a metatile, as drawn from its entries
pub struct LayerTextures {
    pub pair: Pair,
    pub entries: [u16; 8],
    pub textures: [TextureHandle; 2],
}

impl Doc {
    pub fn new(game: Game, file_name: String) -> Self {
        let mut doc = Self {
            game,
            file_name,
            map: None,
            stamp: Stamp::from_picker(1, 0, 1, 0),
            stroke: None,
            picking: None,
            picker_drag: None,
            hover: None,
            picker_hover: None,
            entry: 0,
            edited: BTreeSet::new(),
            unsaved_edits: false,
            scroll_picker: false,
            zoom_wheel: 0.0,
            reference: None,
            cursor: None,
            scroll: egui::Vec2::ZERO,
            notice: None,
            map_top: 0.0,
            reference_top: 0.0,
            strip: 0.0,
            sheets: HashMap::new(),
            usage: HashMap::new(),
            tiles: None,
            layers: None,
        };
        doc.open_map(0);
        doc
    }

    pub fn open_map(&mut self, index: usize) {
        self.end_stroke();
        self.picking = None;
        self.hover = None;
        self.map = self.game.load_map(index);
    }

    /// The map again from the ROM, and the tilesets' pictures: after anything that
    /// changed the ROM behind the map on screen
    pub fn reload(&mut self) {
        self.stroke = None;
        self.sheets.clear();
        self.tiles = None;
        self.layers = None;
        if let Some(index) = self.map.as_ref().map(|m| m.index) {
            self.map = self.game.load_map(index);
        }
        self.changed();
    }

    fn changed(&mut self) {
        self.edited = self.game.edited_layouts();
        self.unsaved_edits = true;
    }

    /// A tileset's metatiles were written to: their pictures are to be drawn again
    pub fn tileset_changed(&mut self) {
        self.sheets.clear();
        self.changed();
    }

    /// Ends an edit made through the game (tileset edits, imports)
    pub fn commit(&mut self) -> bool {
        let changed = self.game.commit();
        if changed {
            self.reload();
        }
        changed
    }

    pub fn undo(&mut self) -> bool {
        self.end_stroke();
        let at = self.game.undo();
        self.after_undo(at)
    }

    pub fn redo(&mut self) -> bool {
        self.end_stroke();
        let at = self.game.redo();
        self.after_undo(at)
    }

    // Undo acts on the whole ROM: when it changed another map, that map is shown
    fn after_undo(&mut self, at: Option<usize>) -> bool {
        let Some(at) = at else { return false };
        if let Some(index) = self.game.map_at(at) {
            let same_layout = self.map.as_ref().is_some_and(|m| m.layout.offset == self.game.maps[index].layout.offset);
            if !same_layout {
                self.map = self.game.load_map(index);
            }
        }
        self.reload();
        true
    }

    pub fn sheet(&mut self, ctx: &egui::Context, pair: Pair) -> &Sheet {
        let game = &self.game;
        self.sheets.entry(pair).or_insert_with(|| Sheet::new(ctx, game, pair))
    }

    /// Takes a rectangle of the reference's map as the stamp. The reference being this
    /// ROM as it was opened, its tiles come as they were. Another ROM has other tilesets:
    /// each of its metatiles is replaced by the closest-looking one of this map's, with
    /// the collision and elevation it has there.
    pub fn pick_from_reference(&mut self, ctx: &egui::Context, a: (usize, usize), b: (usize, usize)) {
        let Some(pair) = self.map.as_ref().map(|m| m.pair) else { return };
        self.sheet(ctx, pair);
        let Some(reference) = self.reference.as_mut() else { return };
        let Some(from) = reference.map.as_ref() else { return };
        let (from_pair, mut stamp) = (from.pair, Stamp::from_map(from, a, b));
        let same_tiles = reference.own && from_pair == pair;
        if !same_tiles {
            reference.sheet(ctx, from_pair);
            let (Some(theirs), Some(ours)) = (reference.drawn(from_pair), self.sheets.get(&pair)) else { return };
            let mut matches: HashMap<u16, u16> = HashMap::new();
            for cell in &mut stamp.cells {
                let id = cell.metatile;
                cell.metatile = *matches
                    .entry(id)
                    .or_insert_with(|| closest_metatile(&ours.image, PICKER_COLUMNS, ours.count, &theirs.picture(id)).unwrap_or(0));
            }
            let first = stamp.first() as usize;
            stamp.picker = (stamp.cells.len() == 1).then_some((first % PICKER_COLUMNS, first / PICKER_COLUMNS));
        }
        let size = format!("{}×{}", stamp.width, stamp.height);
        self.notice = Some(if same_tiles {
            format!("Picked {size} from the reference: the tiles as they were")
        } else {
            format!("Picked {size} from the reference: the closest-looking metatiles of this map's tilesets, to be checked")
        });
        self.stamp = stamp;
        self.scroll_picker = true;
    }

    fn usage(&mut self, pair: Pair) -> &HashMap<u16, BlockUsage> {
        let game = &self.game;
        self.usage.entry(pair).or_insert_with(|| game.block_usage(pair))
    }

    // -- strokes --------------------------------------------------------------------

    /// The main button is down on the map at a tile (which may be outside it)
    pub fn stroke_to(&mut self, at: (isize, isize), tool: Tool, elevation: u8, auto: bool) {
        let Some(map) = self.map.as_ref() else { return };
        let inside = at.0 >= 0 && at.1 >= 0 && (at.0 as usize) < map.width && (at.1 as usize) < map.height;
        let Some(stroke) = self.stroke.as_mut() else {
            if !inside {
                return;
            }
            let b = map.blocks[at.1 as usize * map.width + at.0 as usize];
            self.stroke = Some(Stroke { tool, anchor: at, last: at, collision: (block::collision(b) == 0) as u8 });
            self.apply(at, elevation, auto);
            return;
        };
        if stroke.last == at || stroke.tool == Tool::Fill {
            return;
        }
        // every tile between the last one and this one: a fast drag leaves no gap
        let (from, mut steps) = (stroke.last, Vec::new());
        stroke.last = at;
        let (dx, dy) = (at.0 - from.0, at.1 - from.1);
        let n = dx.abs().max(dy.abs());
        for i in 1..=n {
            steps.push((from.0 + (dx * i + n / 2 * dx.signum()) / n, from.1 + (dy * i + n / 2 * dy.signum()) / n));
        }
        for step in steps {
            self.apply(step, elevation, auto);
        }
    }

    fn apply(&mut self, at: (isize, isize), elevation: u8, auto: bool) {
        let Some(stroke) = self.stroke.as_ref() else { return };
        let (tool, anchor, collision) = (stroke.tool, stroke.anchor, stroke.collision);
        let Some(pair) = self.map.as_ref().map(|m| m.pair) else { return };
        match tool {
            Tool::Paint => {
                let stamp = self.stamp.clone();
                // a stamp of several tiles is laid on a grid from where the stroke began
                let snap = |v: isize, origin: isize, size: usize| origin + (v - origin).div_euclid(size as isize) * size as isize;
                let (x, y) = (snap(at.0, anchor.0, stamp.width), snap(at.1, anchor.1, stamp.height));
                for dy in 0..stamp.height as isize {
                    for dx in 0..stamp.width as isize {
                        self.put((x + dx, y + dy), stamp.cell(dx, dy), pair, auto);
                    }
                }
            }
            Tool::Fill => self.fill(at, pair, auto),
            Tool::Collision => self.edit_block(at, |b| block::with_collision(b, collision)),
            Tool::Elevation => self.edit_block(at, |b| block::with_elevation(b, elevation)),
        }
    }

    fn edit_block(&mut self, at: (isize, isize), edit: impl Fn(u16) -> u16) {
        let Some(map) = self.map.as_mut() else { return };
        if at.0 >= 0 && at.1 >= 0 && (at.0 as usize) < map.width && (at.1 as usize) < map.height {
            let i = at.1 as usize * map.width + at.0 as usize;
            map.blocks[i] = edit(map.blocks[i]);
        }
    }

    /// Puts a stamp's cell on a tile: its metatile, and its collision and elevation when
    /// it was picked with them, else the metatile's usual ones if `auto`
    fn put(&mut self, at: (isize, isize), cell: StampCell, pair: Pair, auto: bool) {
        if cell.metatile as usize >= self.game.metatile_count(pair) {
            return;
        }
        let usual = if auto && cell.high.is_none() { self.usage(pair).get(&cell.metatile).cloned() } else { None };
        self.edit_block(at, |b| {
            let high = cell.high.or_else(|| usual.as_ref().map(|u| u.choose(b))).unwrap_or(b & !block::METATILE_MASK);
            high | cell.metatile
        });
    }

    /// Replaces the area of one metatile around a tile with the stamp, tiled from that tile
    fn fill(&mut self, at: (isize, isize), pair: Pair, auto: bool) {
        let Some(map) = self.map.as_ref() else { return };
        let (w, h) = (map.width as isize, map.height as isize);
        if at.0 < 0 || at.1 < 0 || at.0 >= w || at.1 >= h {
            return;
        }
        let target = block::metatile(map.blocks[(at.1 * w + at.0) as usize]);
        let stamp = self.stamp.clone();
        let mut seen = vec![false; (w * h) as usize];
        let mut queue = VecDeque::from([at]);
        seen[(at.1 * w + at.0) as usize] = true;
        let mut area = Vec::new();
        while let Some((x, y)) = queue.pop_front() {
            area.push((x, y));
            for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                if nx < 0 || ny < 0 || nx >= w || ny >= h || seen[(ny * w + nx) as usize] {
                    continue;
                }
                seen[(ny * w + nx) as usize] = true;
                if block::metatile(map.blocks[(ny * w + nx) as usize]) == target {
                    queue.push_back((nx, ny));
                }
            }
        }
        for (x, y) in area {
            self.put((x, y), stamp.cell(x - at.0, y - at.1), pair, auto);
        }
    }

    /// The button is released: the stroke goes into the ROM as one step
    pub fn end_stroke(&mut self) {
        if self.stroke.take().is_none() {
            return;
        }
        if let Some(map) = &self.map {
            self.game.store_map(map);
            if self.game.commit() {
                self.changed();
            }
        }
    }

    /// Sets a border tile from the stamp
    pub fn paint_border(&mut self, x: usize, y: usize) {
        let Some(map) = self.map.as_mut() else { return };
        let cell = self.stamp.cell(x as isize, y as isize);
        if cell.metatile as usize >= self.game.metatile_count(map.pair) || x >= map.border_width || y >= map.border_height {
            return;
        }
        let i = y * map.border_width + x;
        map.border[i] = block::with_metatile(map.border[i], cell.metatile);
        self.game.store_map(map);
        if self.game.commit() {
            self.changed();
        }
    }
}
