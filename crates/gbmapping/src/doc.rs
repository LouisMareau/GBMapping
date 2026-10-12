//! An opened ROM and what's being done to it: the map on screen, the stamp in hand,
//! the stroke under way. No drawing here.

use crate::reference::Reference;
use egui::{ColorImage, TextureHandle, TextureOptions};
use gbmapping_core::format::block;
use gbmapping_core::game::BlockUsage;
use gbmapping_core::gfx::METATILE;
use gbmapping_core::structs::Layout;
use gbmapping_core::tiles::TileUsage;
use gbmapping_core::{closest_metatile, Game, Image, Map, Pair};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::rc::Rc;

/// Metatiles per row in the picker: the games' tilesets are laid out for 8
pub const PICKER_COLUMNS: usize = 8;
/// Tiles per row in the tileset tab
pub const TILE_COLUMNS: usize = 16;

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
    /// What the pointer is over in the tileset tab's tiles
    pub tile_hint: String,
    /// The pair, metatile and quarter the tileset tab's tiles were last shown for
    pub tiles_shown: Option<(Pair, u16, usize)>,
    /// Where the two maps' views begin on the screen, and how tall the strip over the
    /// edited map is: adjusted until both begin at the same height, tile facing tile
    pub map_top: f32,
    pub reference_top: f32,
    pub strip: f32,
    sheets: HashMap<Pair, Sheet>,
    usage: HashMap<Pair, HashMap<u16, BlockUsage>>,
    pub layers: Option<LayerTextures>,
    /// How the metatiles use the tiles, and the maps the metatiles: until something changes
    tile_usage: Option<Rc<TileUsage>>,
    metatile_uses: Option<HashMap<usize, Vec<u32>>>,
    /// The tilesets the tileset tab lists (for a pair, with or without the others), and their tiles as drawn
    tile_sections: Option<TileSections>,
    tile_sheets: HashMap<(Pair, bool, Option<u8>), Rc<TilesTexture>>,
    /// The Draw tab's layer, pen and colour
    pub draw: crate::draw::Draw,
    /// The picker moves metatiles: one dragged onto another changes places with it
    pub moving: bool,
    /// Where the metatile being dragged would go
    pub move_to: Option<(usize, usize)>,
    /// How many quarters of metatiles draw a tile, as last counted
    tile_users: Option<((Pair, u16), usize)>,
}

/// A tileset's tiles as the tileset tab draws them
pub struct TilesTexture {
    pub texture: TextureHandle,
    /// The palette each tile is drawn in
    pub palettes: Vec<u8>,
}

/// The tileset tab's list, as made for a map's pair (and its original's), with or
/// without the other tilesets
struct TileSections {
    pair: Pair,
    original: Option<Pair>,
    all: bool,
    list: Rc<Vec<TileSection>>,
}

/// A tileset in the tileset tab's list of tiles
pub struct TileSection {
    pub label: String,
    /// The pair it's taken in (its palettes), and whether it's that pair's secondary one
    pub pair: Pair,
    pub secondary: bool,
    pub count: usize,
    /// Where its tiles begin in the map's pair, when it's one of the map's two tilesets
    pub base: Option<u16>,
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
            tile_hint: String::new(),
            tiles_shown: None,
            map_top: 0.0,
            reference_top: 0.0,
            strip: 0.0,
            sheets: HashMap::new(),
            usage: HashMap::new(),
            layers: None,
            tile_usage: None,
            metatile_uses: None,
            tile_sections: None,
            tile_sheets: HashMap::new(),
            draw: crate::draw::Draw::default(),
            moving: false,
            move_to: None,
            tile_users: None,
        };
        doc.open_map(0);
        // a game painted by blocks begins with a whole block in hand
        if doc.grain() > 1 {
            doc.pick_metatiles((0, 0), (0, 0));
        }
        doc
    }

    /// How many metatiles a side the game's own unit is (see `Format::block_size`): what's
    /// taken and painted is whole units. 1 for the Advance games; 2 for the Game Boy
    /// Color ones, whose blocks are four metatiles.
    pub fn grain(&self) -> usize {
        self.game.format.block_size()
    }

    /// A rectangle of tiles, grown to whole units: its top left and bottom right corners
    pub fn whole(&self, a: (usize, usize), b: (usize, usize)) -> ((usize, usize), (usize, usize)) {
        let g = self.grain();
        ((a.0.min(b.0) / g * g, a.1.min(b.1) / g * g), (a.0.max(b.0) / g * g + g - 1, a.1.max(b.1) / g * g + g - 1))
    }

    /// Takes the picker's metatiles from one corner to the other as the stamp
    pub fn pick_metatiles(&mut self, a: (usize, usize), b: (usize, usize)) {
        let (a, b) = self.whole(a, b);
        self.stamp = Stamp::from_picker(a.0, a.1, b.0.min(PICKER_COLUMNS - 1), b.1);
    }

    /// Takes the map's tiles from one corner to the other as the stamp
    pub fn pick_tiles(&mut self, a: (usize, usize), b: (usize, usize)) {
        let (a, b) = self.whole(a, b);
        let Some(map) = self.map.as_ref() else { return };
        let mut stamp = Stamp::from_map(map, a, (b.0.min(map.width - 1), b.1.min(map.height - 1)));
        // one unit: the picker shows where it is
        if stamp.picker.is_none() && (stamp.width, stamp.height) == (self.grain(), self.grain()) {
            let first = stamp.first() as usize;
            stamp.picker = Some((first % PICKER_COLUMNS, first / PICKER_COLUMNS));
        }
        self.stamp = stamp;
        self.scroll_picker = true;
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
        self.forget_tilesets();
        self.layers = None;
        if let Some(index) = self.map.as_ref().map(|m| m.index) {
            self.map = self.game.load_map(index);
        }
        self.changed();
    }

    fn changed(&mut self) {
        self.edited = self.game.edited_layouts();
        self.unsaved_edits = true;
        self.metatile_uses = None;
    }

    // What was drawn or counted from the tilesets is to be drawn and counted again
    fn forget_tilesets(&mut self) {
        self.sheets.clear();
        self.tile_usage = None;
        self.tile_sections = None;
        self.tile_sheets.clear();
        self.tile_users = None;
    }

    /// Makes two of the picker's metatiles (whole blocks, in a game of blocks) change
    /// places, `from` and `to` being where they are in the picker. The maps drawn with
    /// their tileset are renumbered to match: nothing on them looks different.
    pub fn move_metatile(&mut self, from: (usize, usize), to: (usize, usize)) {
        self.end_stroke();
        let Some(pair) = self.map.as_ref().map(|m| m.pair) else { return };
        let ((from, _), (to, _)) = (self.whole(from, from), self.whole(to, to));
        if from == to {
            return;
        }
        let g = self.grain();
        let id = |unit: (usize, usize), i: usize| ((unit.1 + i / g) * PICKER_COLUMNS + unit.0 + i % g) as u16;
        // all of the two, or nothing
        let there = |game: &Game, unit: (usize, usize)| (0..g * g).all(|i| game.locate(pair, id(unit, i)).is_some());
        if !there(&self.game, from) || !there(&self.game, to) {
            self.notice = Some("There's no metatile there to change places with".to_string());
            return;
        }
        let mut renumbered = 0;
        for i in 0..g * g {
            match self.game.swap_metatiles(pair, id(from, i), id(to, i)) {
                Ok(tiles) => renumbered += tiles,
                Err(e) => {
                    self.notice = Some(e.to_string());
                    return;
                }
            }
        }
        if !self.game.commit() {
            return;
        }
        self.reload();
        self.pick_metatiles(to, to);
        // a block goes by its own number, not by its quarters'
        let number = |unit: (usize, usize)| if g > 1 { gbmapping_core::gbc::block_of(id(unit, 0)).0 } else { id(unit, 0) as usize };
        let what = if g > 1 { "Blocks" } else { "Metatiles" };
        self.notice = Some(format!(
            "{what} 0x{:03X} and 0x{:03X} changed places, and {renumbered} tiles of the maps were renumbered to match: no map looks different. What names one by its number elsewhere (a script that places it, the game's doors) isn't changed.",
            number(from),
            number(to)
        ));
    }

    /// What to call a tileset: where it is, and the maps drawn with it
    pub fn tileset_label(&self, offset: usize) -> String {
        let mut users = self.game.maps.iter().filter(|m| m.layout.primary == offset || m.layout.secondary == offset);
        let first = users.next().map_or("no map", |m| m.name.as_str());
        let new = if self.game.is_new_tileset(offset) { "new · " } else { "" };
        match users.count() {
            0 => format!("{new}0x{offset:06X} · {first}"),
            more => format!("{new}0x{offset:06X} · {first} + {more} maps"),
        }
    }

    /// The tilesets a map can be given as its primary or its secondary one: those the maps are drawn with
    pub fn tileset_choices(&self, primary: bool) -> Vec<usize> {
        self.game.tilesets().into_iter().filter(|&(_, as_primary)| as_primary == primary).map(|(t, _)| t.offset).collect()
    }

    /// Gives the map on screen another tileset, primary or secondary: one of the ROM's, a
    /// copy of the one it has (`None`), or a new, empty secondary one (`empty`). It's the
    /// map's layout that's given it: every map drawn from that layout changes with it.
    pub fn give_tileset(&mut self, primary: bool, tileset: Option<usize>, empty: bool) {
        self.end_stroke();
        let Some((layout, pair)) = self.map.as_ref().map(|m| (m.layout.offset, m.pair)) else { return };
        let had = if primary { pair.primary } else { pair.secondary };
        let which = if primary { "primary" } else { "secondary" };
        let made = match (tileset, empty) {
            (Some(tileset), _) => Ok(tileset),
            (None, true) => self.game.empty_tileset(had),
            (None, false) => self.game.duplicate_tileset(had, primary),
        };
        let given = match made {
            Ok(given) if self.game.set_layout_tileset(layout, primary, given) => given,
            Ok(given) => {
                self.notice = Some(format!("There's no tileset at 0x{given:06X}"));
                return;
            }
            Err(e) => {
                self.notice = Some(e.to_string());
                return;
            }
        };
        self.game.commit();
        self.reload();
        let maps = match self.game.layout_users(layout).len() {
            1 => "This map".to_string(),
            n => format!("The {n} maps drawn from this layout"),
        };
        self.notice = Some(match (tileset, empty) {
            (Some(_), _) => format!("{maps}: {which} tileset 0x{given:06X} from now on"),
            (None, true) => format!(
                "{maps}: a new, empty secondary tileset (0x{given:06X}). What was drawn with the old one shows nothing until it's redrawn; the old one's tiles are under \"All tilesets\"."
            ),
            (None, false) => format!("{maps}: a copy of the {which} tileset, its own (0x{given:06X}). What's changed in it changes no other map."),
        });
    }

    /// How many quarters of metatiles draw a tile of a pair: drawing on it changes them all
    pub fn tile_users(&mut self, pair: Pair, tile: u16) -> usize {
        match self.tile_users {
            Some((counted, users)) if counted == (pair, tile) => users,
            _ => {
                let users = self.game.tile_entries(pair, tile);
                self.tile_users = Some(((pair, tile), users));
                users
            }
        }
    }

    /// A tileset was written to (its metatiles, its tiles, its palettes)
    pub fn tileset_changed(&mut self) {
        self.forget_tilesets();
        self.changed();
    }

    /// How the metatiles use the tiles: which palette each tile is drawn in
    pub fn tile_usage(&mut self) -> Rc<TileUsage> {
        let game = &self.game;
        self.tile_usage.get_or_insert_with(|| Rc::new(game.tile_usage())).clone()
    }

    /// How many tiles of the maps a metatile of a tileset is on
    pub fn metatile_uses(&mut self, tileset: usize, local: usize) -> u32 {
        let game = &self.game;
        self.metatile_uses.get_or_insert_with(|| game.metatile_uses()).get(&tileset).and_then(|uses| uses.get(local)).copied().unwrap_or(0)
    }

    /// The next metatile of a pair's secondary tileset that no map shows, after `id`
    /// (and from the tileset's first one again): one to rebuild without changing any map
    pub fn next_free_metatile(&mut self, pair: Pair, id: u16) -> Option<u16> {
        let first = self.game.format.primary_metatiles;
        let count = self.game.metatile_count(pair);
        let after = (id as usize + 1).max(first);
        let mut candidates = (after..count).chain(first..after.min(count));
        candidates.find(|&i| self.metatile_uses(pair.secondary, i - first) == 0).map(|i| i as u16)
    }

    /// Takes a metatile as the stamp, and shows it in the picker
    pub fn select_metatile(&mut self, id: u16) {
        let at = (id as usize % PICKER_COLUMNS, id as usize / PICKER_COLUMNS);
        self.pick_metatiles(at, at);
        self.scroll_picker = true;
    }

    /// The tilesets whose tiles the tileset tab lists for a pair: its own two, and when
    /// `all` every other one: those of the map's original, those the maps use (of the
    /// same primary tileset first), then the other originals'
    pub fn tile_sections(&mut self, pair: Pair, all: bool) -> Rc<Vec<TileSection>> {
        let game = &self.game;
        let original = self.map.as_ref().and_then(|m| game.maps.get(m.index)?.original).map(|l| l.pair());
        if let Some(made) = self.tile_sections.as_ref().filter(|made| (made.pair, made.original, made.all) == (pair, original, all)) {
            return made.list.clone();
        }
        // a Game Boy Color map has one tileset, whose tiles go by a byte
        if game.format.block_size() > 1 {
            let count = game.tileset_tile_count(pair, false).min(256);
            let section = TileSection { label: "This map's tileset".to_string(), pair, secondary: false, count, base: Some(0) };
            let list = Rc::new(vec![section]);
            self.tile_sections = Some(TileSections { pair, original, all, list: list.clone() });
            return list;
        }
        let own = |secondary: bool| TileSection {
            label: format!(
                "This map's {} tileset · 0x{:06X}",
                if secondary { "secondary" } else { "primary" },
                if secondary { pair.secondary } else { pair.primary }
            ),
            pair,
            secondary,
            count: game.tileset_tile_count(pair, secondary),
            base: Some(if secondary { game.format.primary_tiles as u16 } else { 0 }),
        };
        let mut sections = vec![own(false), own(true)];
        if all {
            // a tileset the maps are on, or one the ROM keeps for what its maps were
            let other = |offset: usize, kept: bool| -> Option<TileSection> {
                let (from, secondary) = game.pair_with(offset).filter(|_| offset != pair.primary && offset != pair.secondary)?;
                // the empty tiles a tileset ends with are of no use to another one
                let count = Some(game.tileset_drawn_tiles(from, secondary)).filter(|&count| count > 0)?;
                let on = |l: Layout| l.primary == offset || l.secondary == offset;
                let mut users = game.maps.iter().filter(|m| if kept { m.original.is_some_and(on) } else { on(m.layout) });
                let first = users.next().map_or(String::new(), |m| m.name.clone());
                let more = users.count();
                let maps = if more > 0 { format!("{first} + {more} maps") } else { first };
                let label = format!("0x{offset:06X} · {}{maps}", if kept { "original of " } else { "" });
                Some(TileSection { label, pair: from, secondary, count, base: None })
            };
            let mut others: Vec<TileSection> = game.tilesets().iter().filter_map(|(t, _)| other(t.offset, false)).collect();
            // the secondary tilesets that go with this map's primary one first: their primary colours are this map's
            others.sort_by_key(|s| (s.pair.primary != pair.primary, !s.secondary));
            let mut kept: Vec<TileSection> = game.original_tilesets().iter().filter_map(|(t, _)| other(t.offset, true)).collect();
            // what this map was drawn with before comes right under its own tilesets
            let (mut its, rest): (Vec<TileSection>, Vec<TileSection>) = {
                let of_this = |s: &TileSection| {
                    original.is_some_and(|o| if s.secondary { s.pair.secondary == o.secondary } else { s.pair.primary == o.primary })
                };
                kept.drain(..).partition(of_this)
            };
            for section in &mut its {
                section.label = format!(
                    "This map's original · {} tileset · 0x{:06X}",
                    if section.secondary { "secondary" } else { "primary" },
                    if section.secondary { section.pair.secondary } else { section.pair.primary }
                );
            }
            its.sort_by_key(|s| s.secondary);
            sections.extend(its);
            sections.extend(others);
            sections.extend(rest);
        }
        let list = Rc::new(sections);
        self.tile_sections = Some(TileSections { pair, original, all, list: list.clone() });
        list
    }

    /// A tileset's tiles, drawn: each in its usual palette, or all in `palette`
    pub fn tile_sheet(&mut self, ctx: &egui::Context, pair: Pair, secondary: bool, palette: Option<u8>) -> Rc<TilesTexture> {
        if let Some(sheet) = self.tile_sheets.get(&(pair, secondary, palette)) {
            return sheet.clone();
        }
        let usage = self.tile_usage();
        let sheet = self.game.tile_sheet(pair, secondary, &usage, palette, TILE_COLUMNS);
        let drawn = Rc::new(TilesTexture { texture: texture(ctx, "tiles", &sheet.image), palettes: sheet.palettes });
        self.tile_sheets.insert((pair, secondary, palette), drawn.clone());
        drawn
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
            // the map on screen may be another one drawn from the same blocks: it shows the change too
            let there = self.game.maps[index].layout;
            let same_layout = self.map.as_ref().is_some_and(|m| m.layout.offset == there.offset || m.layout.blocks == there.blocks);
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
        let (blocks, whole) = (self.grain() > 1, self.whole(a, b));
        let Some(reference) = self.reference.as_mut() else { return };
        let Some(from) = reference.map.as_ref() else { return };
        let same_tiles = reference.own && from.pair == pair;
        // a map painted by blocks takes whole blocks, and only its own
        if blocks && !same_tiles {
            self.notice =
                Some("This map is painted by whole blocks: only its own, as they were, can be taken from the reference".to_string());
            return;
        }
        let (a, b) = if blocks { (whole.0, (whole.1 .0.min(from.width - 1), whole.1 .1.min(from.height - 1))) } else { (a, b) };
        let (from_pair, mut stamp) = (from.pair, Stamp::from_map(from, a, b));
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
        // whole units are painted: the stroke goes from one's corner to another's
        let g = self.grain() as isize;
        let at = (at.0.div_euclid(g) * g, at.1.div_euclid(g) * g);
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

    /// Replaces the area of one metatile around a tile with the stamp, tiled from that
    /// tile. Where whole blocks are painted, the area of one block.
    fn fill(&mut self, at: (isize, isize), pair: Pair, auto: bool) {
        let Some(map) = self.map.as_ref() else { return };
        let g = self.grain() as isize;
        let w = map.width as isize;
        // the area is made of units: a tile, or a block of them
        let (uw, uh) = (w / g, map.height as isize / g);
        let start = (at.0.div_euclid(g), at.1.div_euclid(g));
        if start.0 < 0 || start.1 < 0 || start.0 >= uw || start.1 >= uh {
            return;
        }
        let unit = |x: isize, y: isize| -> Vec<u16> {
            (0..g * g).map(|i| block::metatile(map.blocks[((y * g + i / g) * w + x * g + i % g) as usize])).collect()
        };
        let target = unit(start.0, start.1);
        let stamp = self.stamp.clone();
        let mut seen = vec![false; (uw * uh) as usize];
        let mut queue = VecDeque::from([start]);
        seen[(start.1 * uw + start.0) as usize] = true;
        let mut area = Vec::new();
        while let Some((x, y)) = queue.pop_front() {
            area.push((x, y));
            for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                if nx < 0 || ny < 0 || nx >= uw || ny >= uh || seen[(ny * uw + nx) as usize] {
                    continue;
                }
                seen[(ny * uw + nx) as usize] = true;
                if unit(nx, ny) == target {
                    queue.push_back((nx, ny));
                }
            }
        }
        for (ux, uy) in area {
            for i in 0..g * g {
                let (x, y) = (ux * g + i % g, uy * g + i / g);
                self.put((x, y), stamp.cell(x - start.0 * g, y - start.1 * g), pair, auto);
            }
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
        let whole = self.grain() > 1;
        let Some(map) = self.map.as_mut() else { return };
        let cell = self.stamp.cell(x as isize, y as isize);
        if cell.metatile as usize >= self.game.metatile_count(map.pair) || x >= map.border_width || y >= map.border_height {
            return;
        }
        // where blocks are painted whole, the border is one block: all of it at once
        let across = map.border_width;
        for (i, tile) in map.border.iter_mut().enumerate() {
            let (bx, by) = (i % across, i / across);
            if whole || (bx, by) == (x, y) {
                *tile = block::with_metatile(*tile, self.stamp.cell(bx as isize, by as isize).metatile);
            }
        }
        self.game.store_map(map);
        if self.game.commit() {
            self.changed();
        }
    }
}
