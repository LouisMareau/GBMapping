//! A map's events: the people and things on it (object events), its warps, the
//! triggers that run a script when stepped on (coord events), and its signs and hidden
//! items (background events).
//!
//! They're four lists the map's header points to, through its struct MapEvents: a
//! count and a pointer per list. An object event tells its sprite on a byte, after its
//! number; hacks built on pokeemerald-expansion tell it on 16 bits after its kind (to
//! have more than 255): which way a ROM does is told by its maps (see `objects_wide`). A list that shrinks is written where it is; one that
//! grows is moved to the ROM's free end (or grows there, when it already is). Scripts
//! aren't written here, but for the simplest kind: a text said, see
//! `Game::new_message_script`.

use crate::format::Family;
use crate::game::Game;
use crate::rom::{Rom, GBA_ROM_BASE};
use crate::text;
use serde::{Deserialize, Serialize};
use std::fmt;

const EVENTS_SIZE: usize = 20;
/// The most a list holds: its count is a byte
pub const MOST: usize = 255;
/// Space left before what's written at the ROM's free end
const MARGIN: usize = 256;

/// A sign's kind: read from any side, or from below, above, the right, the left
pub const SIGN_KINDS: [&str; 5] = ["Read from any side", "Read from below", "Read from above", "Read from the left", "Read from the right"];
pub const HIDDEN_ITEM: u8 = 7;
pub const SECRET_BASE: u8 = 8;
/// An object event that shows one of another map's (FireRed/LeafGreen: across a connection)
pub const CLONE: u8 = 255;

/// The trainer types: an object event that's a trainer sees the player, or not
pub const TRAINER_TYPES: [&str; 4] = ["Not a trainer", "Trainer", "Trainer who sees all around", "Buried trainer"];

/// The standard scripts a message is said with (`callstd`): the person turns to the player, a sign doesn't
pub const MSGBOX_NPC: u8 = 2;
pub const MSGBOX_SIGN: u8 = 3;

/// A warp's map when it's the one the player came from
pub const DYNAMIC_MAP: (u8, u8) = (0x7F, 0x7F);

/// The four lists
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum List {
    Objects,
    Warps,
    Triggers,
    Signs,
}

impl List {
    pub const ALL: [Self; 4] = [Self::Objects, Self::Warps, Self::Triggers, Self::Signs];

    /// Bytes an entry takes in the ROM
    pub fn size(self) -> usize {
        match self {
            Self::Objects => 24,
            Self::Warps => 8,
            Self::Triggers => 16,
            Self::Signs => 12,
        }
    }

    /// Where its count is in struct MapEvents (its pointer is 4 + 4 times that further)
    fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Objects => "People and objects",
            Self::Warps => "Warps",
            Self::Triggers => "Triggers",
            Self::Signs => "Signs and hidden items",
        }
    }
}

/// A person, an item ball, a boulder: what the player sees on the map and talks to
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Object {
    /// Its number on the map, which scripts name it by
    pub id: u8,
    /// Its sprite
    pub graphics: u16,
    /// 0; 255 in FireRed/LeafGreen for a clone of another map's (see `CLONE`)
    #[serde(skip_serializing_if = "is_zero")]
    pub kind: u8,
    pub x: i16,
    pub y: i16,
    /// A clone's: the number of the object it shows
    pub elevation: u8,
    pub movement: u8,
    /// How far it walks from its place, across and down (0 to 15)
    pub range: [u8; 2],
    /// See `TRAINER_TYPES`. A clone's: the number of the map its object is on
    pub trainer: u16,
    /// How far a trainer sees; a berry tree's number. A clone's: the group of the map its object is on
    pub sight: u16,
    /// Run when it's talked to (a ROM address: 0x08…), or 0 for none
    #[serde(with = "hex")]
    pub script: u32,
    /// Set, it isn't there (a flag's number), or 0
    pub flag: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Warp {
    pub x: i16,
    pub y: i16,
    pub elevation: u8,
    /// The warp it leads to, on the map it leads to
    pub warp: u8,
    pub group: u8,
    pub num: u8,
}

/// A tile that runs a script when it's stepped on, while a variable has a value
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Trigger {
    pub x: i16,
    pub y: i16,
    pub elevation: u8,
    /// The variable, or 0: always
    pub var: u16,
    pub value: u16,
    #[serde(with = "hex")]
    pub script: u32,
}

/// What the player reads by facing a tile (a sign, a bookshelf), or finds there
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sign {
    pub x: i16,
    pub y: i16,
    pub elevation: u8,
    /// See `SIGN_KINDS`, `HIDDEN_ITEM`, `SECRET_BASE`
    pub kind: u8,
    /// The script read (a ROM address); a hidden item (see `HiddenItem`); a secret base's number
    #[serde(with = "hex")]
    pub data: u32,
}

impl Sign {
    pub fn script(&self) -> Option<u32> {
        (self.kind < HIDDEN_ITEM).then_some(self.data)
    }
}

/// A hidden item, as a sign's data holds it
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HiddenItem {
    pub item: u16,
    /// Set once it's found: a number counted from the games' first hidden item flag
    pub flag: u16,
    /// FireRed/LeafGreen: how many, and whether it's found by stepping on it
    pub quantity: u8,
    pub underfoot: bool,
}

impl HiddenItem {
    pub fn of(family: Family, data: u32) -> Self {
        let item = data as u16;
        match family {
            Family::FireRedLeafGreen => {
                HiddenItem { item, flag: (data >> 16) as u8 as u16, quantity: ((data >> 24) & 0x7F) as u8, underfoot: data >> 31 == 1 }
            }
            _ => HiddenItem { item, flag: (data >> 16) as u16, quantity: 1, underfoot: false },
        }
    }

    pub fn data(self, family: Family) -> u32 {
        match family {
            Family::FireRedLeafGreen => {
                self.item as u32 | (self.flag as u8 as u32) << 16 | ((self.quantity & 0x7F) as u32) << 24 | (self.underfoot as u32) << 31
            }
            _ => self.item as u32 | (self.flag as u32) << 16,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Events {
    pub objects: Vec<Object>,
    pub warps: Vec<Warp>,
    pub triggers: Vec<Trigger>,
    pub signs: Vec<Sign>,
}

impl Events {
    pub fn len(&self, list: List) -> usize {
        match list {
            List::Objects => self.objects.len(),
            List::Warps => self.warps.len(),
            List::Triggers => self.triggers.len(),
            List::Signs => self.signs.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        List::ALL.iter().all(|&list| self.len(list) == 0)
    }

    /// Where an event is
    pub fn place(&self, list: List, i: usize) -> Option<(i16, i16)> {
        match list {
            List::Objects => self.objects.get(i).map(|e| (e.x, e.y)),
            List::Warps => self.warps.get(i).map(|e| (e.x, e.y)),
            List::Triggers => self.triggers.get(i).map(|e| (e.x, e.y)),
            List::Signs => self.signs.get(i).map(|e| (e.x, e.y)),
        }
    }

    pub fn move_to(&mut self, list: List, i: usize, x: i16, y: i16) {
        let place = match list {
            List::Objects => self.objects.get_mut(i).map(|e| (&mut e.x, &mut e.y)),
            List::Warps => self.warps.get_mut(i).map(|e| (&mut e.x, &mut e.y)),
            List::Triggers => self.triggers.get_mut(i).map(|e| (&mut e.x, &mut e.y)),
            List::Signs => self.signs.get_mut(i).map(|e| (&mut e.x, &mut e.y)),
        };
        if let Some((ex, ey)) = place {
            (*ex, *ey) = (x, y);
        }
    }

    pub fn remove(&mut self, list: List, i: usize) {
        match list {
            List::Objects if i < self.objects.len() => drop(self.objects.remove(i)),
            List::Warps if i < self.warps.len() => drop(self.warps.remove(i)),
            List::Triggers if i < self.triggers.len() => drop(self.triggers.remove(i)),
            List::Signs if i < self.signs.len() => drop(self.signs.remove(i)),
            _ => {}
        }
    }

    /// A copy of an event, one tile to its right, at the end of its list: returns where it is.
    /// A person's copy gets the next number.
    pub fn duplicate(&mut self, list: List, i: usize) -> Option<usize> {
        match list {
            List::Objects => {
                let mut copy = self.objects.get(i)?.clone();
                copy.id = self.next_object_id();
                copy.x += 1;
                self.objects.push(copy);
            }
            List::Warps => {
                let mut copy = self.warps.get(i)?.clone();
                copy.x += 1;
                self.warps.push(copy);
            }
            List::Triggers => {
                let mut copy = self.triggers.get(i)?.clone();
                copy.x += 1;
                self.triggers.push(copy);
            }
            List::Signs => {
                let mut copy = self.signs.get(i)?.clone();
                copy.x += 1;
                self.signs.push(copy);
            }
        }
        Some(self.len(list) - 1)
    }

    /// The number a new person gets: one past the highest
    pub fn next_object_id(&self) -> u8 {
        self.objects.iter().map(|o| o.id).max().map_or(1, |id| id.saturating_add(1))
    }

    /// The events at a tile, the one drawn over the others first (people over the rest)
    pub fn at(&self, x: i16, y: i16) -> Vec<(List, usize)> {
        let mut found = Vec::new();
        for list in [List::Objects, List::Signs, List::Triggers, List::Warps] {
            for i in (0..self.len(list)).rev() {
                if self.place(list, i) == Some((x, y)) {
                    found.push((list, i));
                }
            }
        }
        found
    }

    /// Every script the events run (their ROM addresses)
    pub fn scripts(&self) -> Vec<u32> {
        let objects = self.objects.iter().filter(|o| o.kind != CLONE).map(|o| o.script);
        let triggers = self.triggers.iter().map(|t| t.script);
        let signs = self.signs.iter().filter_map(Sign::script);
        objects.chain(triggers).chain(signs).filter(|&s| s != 0).collect()
    }

    /// Changes the scripts the events run: those `to` has an answer for
    pub fn rename_scripts(&mut self, to: impl Fn(u32) -> Option<u32>) {
        for o in self.objects.iter_mut().filter(|o| o.kind != CLONE) {
            o.script = to(o.script).unwrap_or(o.script);
        }
        for t in &mut self.triggers {
            t.script = to(t.script).unwrap_or(t.script);
        }
        for s in self.signs.iter_mut().filter(|s| s.kind < HIDDEN_ITEM) {
            s.data = to(s.data).unwrap_or(s.data);
        }
    }
}

fn is_zero(v: &u8) -> bool {
    *v == 0
}

/// A ROM address in JSON: "0x08123456"
mod hex {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &u32, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("0x{v:08X}"))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
        let text = String::deserialize(d)?;
        let digits = text.trim().trim_start_matches("0x").trim_start_matches("0X");
        u32::from_str_radix(digits, 16).map_err(serde::de::Error::custom)
    }
}

// -- in the ROM ------------------------------------------------------------------------------

fn read_object(rom: &Rom, at: usize, wide: bool) -> Object {
    let range = rom.u16(at + 10);
    let (graphics, kind) = if wide { (rom.u16(at + 2), rom.u8(at + 1)) } else { (rom.u8(at + 1) as u16, rom.u8(at + 2)) };
    Object {
        id: rom.u8(at),
        graphics,
        kind,
        x: rom.u16(at + 4) as i16,
        y: rom.u16(at + 6) as i16,
        elevation: rom.u8(at + 8),
        movement: rom.u8(at + 9),
        range: [(range & 0xF) as u8, ((range >> 4) & 0xF) as u8],
        trainer: rom.u16(at + 12),
        sight: rom.u16(at + 14),
        script: rom.u32(at + 16),
        flag: rom.u16(at + 20),
    }
}

/// An entry is written field by field: what's between the fields stays as it was
fn write_object(rom: &mut Rom, at: usize, o: &Object, wide: bool) {
    rom.write8(at, o.id);
    if wide {
        rom.write8(at + 1, o.kind);
        rom.write16(at + 2, o.graphics);
    } else {
        rom.write8(at + 1, o.graphics as u8);
        rom.write8(at + 2, o.kind);
    }
    rom.write16(at + 4, o.x as u16);
    rom.write16(at + 6, o.y as u16);
    rom.write8(at + 8, o.elevation);
    rom.write8(at + 9, o.movement);
    let range = (rom.u16(at + 10) & 0xFF00) | (o.range[0] & 0xF) as u16 | ((o.range[1] & 0xF) as u16) << 4;
    rom.write16(at + 10, range);
    rom.write16(at + 12, o.trainer);
    rom.write16(at + 14, o.sight);
    rom.write32(at + 16, o.script);
    rom.write16(at + 20, o.flag);
}

fn read_warp(rom: &Rom, at: usize) -> Warp {
    Warp {
        x: rom.u16(at) as i16,
        y: rom.u16(at + 2) as i16,
        elevation: rom.u8(at + 4),
        warp: rom.u8(at + 5),
        num: rom.u8(at + 6),
        group: rom.u8(at + 7),
    }
}

fn write_warp(rom: &mut Rom, at: usize, w: &Warp) {
    rom.write16(at, w.x as u16);
    rom.write16(at + 2, w.y as u16);
    rom.write8(at + 4, w.elevation);
    rom.write8(at + 5, w.warp);
    rom.write8(at + 6, w.num);
    rom.write8(at + 7, w.group);
}

fn read_trigger(rom: &Rom, at: usize) -> Trigger {
    Trigger {
        x: rom.u16(at) as i16,
        y: rom.u16(at + 2) as i16,
        elevation: rom.u8(at + 4),
        var: rom.u16(at + 6),
        value: rom.u16(at + 8),
        script: rom.u32(at + 12),
    }
}

fn write_trigger(rom: &mut Rom, at: usize, t: &Trigger) {
    rom.write16(at, t.x as u16);
    rom.write16(at + 2, t.y as u16);
    rom.write8(at + 4, t.elevation);
    rom.write16(at + 6, t.var);
    rom.write16(at + 8, t.value);
    rom.write32(at + 12, t.script);
}

fn read_sign(rom: &Rom, at: usize) -> Sign {
    Sign { x: rom.u16(at) as i16, y: rom.u16(at + 2) as i16, elevation: rom.u8(at + 4), kind: rom.u8(at + 5), data: rom.u32(at + 8) }
}

fn write_sign(rom: &mut Rom, at: usize, s: &Sign) {
    rom.write16(at, s.x as u16);
    rom.write16(at + 2, s.y as u16);
    rom.write8(at + 4, s.elevation);
    rom.write8(at + 5, s.kind);
    rom.write32(at + 8, s.data);
}

/// A list of `struct MapEvents` at `events`, read with `read`
fn read_list<T>(rom: &Rom, events: usize, list: List, read: impl Fn(&Rom, usize) -> T) -> Vec<T> {
    let count = rom.u8(events + list.index()) as usize;
    let Some(at) = rom.ptr(events + 4 + 4 * list.index()) else { return Vec::new() };
    (0..count).take_while(|i| at + (i + 1) * list.size() <= rom.len()).map(|i| read(rom, at + i * list.size())).collect()
}

/// The events of the map whose header is at `header`, in a ROM
pub(crate) fn read_events(rom: &Rom, header: usize, wide: bool) -> Events {
    let Some(at) = rom.ptr(header + 4).filter(|&at| at + EVENTS_SIZE <= rom.len()) else { return Events::default() };
    Events {
        objects: read_list(rom, at, List::Objects, |rom, o| read_object(rom, o, wide)),
        warps: read_list(rom, at, List::Warps, read_warp),
        triggers: read_list(rom, at, List::Triggers, read_trigger),
        signs: read_list(rom, at, List::Signs, read_sign),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventsError {
    NoMap,
    TooMany(List),
    /// The ROM has no free space left at its end
    NoRoom,
}

impl fmt::Display for EventsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::NoMap => write!(f, "There's no such map"),
            Self::TooMany(list) => write!(f, "{}: a map holds {MOST} at most", list.name()),
            Self::NoRoom => write!(f, "The ROM has no room left at its end for the events"),
        }
    }
}

/// What a script says, when it's the simplest kind: a text in a message box
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Says {
    pub text: String,
    /// The standard script it's said with (`MSGBOX_NPC`, `MSGBOX_SIGN`…)
    pub std: u8,
    /// It does nothing else: a new script that says another text does what it did
    pub only: bool,
}

/// What a script does, as far as the editor tells
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptView {
    None,
    Says(Says),
    /// A trainer's battle: the trainer's number, and what they say before it
    Battle {
        trainer: u16,
        intro: Option<String>,
    },
    Other,
}

/// A script the editor made: a text said, and how
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MadeScript {
    pub text: String,
    pub std: u8,
}

/// The names the decompilations give to the sprites, the movement types and the items
pub struct Names {
    pub graphics: &'static [(u16, &'static str)],
    pub movements: &'static [(u16, &'static str)],
    pub items: &'static [(u16, &'static str)],
}

impl Names {
    pub fn of(family: Family) -> Self {
        use crate::event_names::*;
        match family {
            Family::Emerald => Names { graphics: EMERALD_GRAPHICS, movements: EMERALD_MOVEMENTS, items: EMERALD_ITEMS },
            // Ruby and Sapphire have Emerald's items and movements; their sprites aren't all Emerald's
            Family::RubySapphire => Names { graphics: &[], movements: EMERALD_MOVEMENTS, items: EMERALD_ITEMS },
            Family::FireRedLeafGreen => Names { graphics: FIRERED_GRAPHICS, movements: FIRERED_MOVEMENTS, items: FIRERED_ITEMS },
            Family::Crystal => Names { graphics: &[], movements: &[], items: &[] },
        }
    }

    pub fn find(table: &[(u16, &'static str)], value: u16) -> Option<&'static str> {
        table.binary_search_by_key(&value, |&(v, _)| v).ok().map(|i| table[i].1)
    }
}

impl Game {
    /// The events of a map (none when it has no struct MapEvents)
    pub fn events(&self, index: usize) -> Events {
        let wide = self.objects_wide();
        self.maps.get(index).map_or_else(Events::default, |m| read_events(&self.rom, m.header.offset, wide))
    }

    /// The events of a map as the ROM was opened
    pub fn events_then(&self, index: usize) -> Events {
        let wide = self.objects_wide();
        self.maps.get(index).map_or_else(Events::default, |m| read_events(self.original(), m.header.offset, wide))
    }

    /// Does an object event tell its sprite on 16 bits, after its kind (pokeemerald-expansion
    /// and the hacks built on it), rather than on the byte before its kind? The maps tell:
    /// a sprite is rarely 0, and a kind nearly always is.
    pub fn objects_wide(&self) -> bool {
        *self.wide_objects.get_or_init(|| {
            let rom = self.original();
            let (mut second, mut third) = (0usize, 0usize);
            for m in &self.maps {
                let Some(at) = rom.ptr(m.header.offset + 4).filter(|&at| at + EVENTS_SIZE <= rom.len()) else { continue };
                let Some(list) = rom.ptr(at + 4) else { continue };
                for i in (0..rom.u8(at) as usize).take_while(|i| list + (i + 1) * List::Objects.size() <= rom.len()) {
                    second += (rom.u8(list + i * 24 + 1) != 0) as usize;
                    third += (rom.u8(list + i * 24 + 2) != 0) as usize;
                }
            }
            third > second
        })
    }

    /// Writes a map's events: each list where it is when it isn't longer, else where
    /// there's room at the ROM's end; a map without a struct MapEvents gets one. To be committed.
    pub fn set_events(&mut self, index: usize, events: &Events) -> Result<(), EventsError> {
        let header = self.maps.get(index).ok_or(EventsError::NoMap)?.header.offset;
        if let Some(&list) = List::ALL.iter().find(|&&list| events.len(list) > MOST) {
            return Err(EventsError::TooMany(list));
        }
        // where each list goes, before anything's written: nothing is when there's no room
        let at = match self.rom.ptr(header + 4).filter(|&at| at + EVENTS_SIZE <= self.rom.len()) {
            Some(at) => at,
            None => {
                let at = self.claim(EVENTS_SIZE)?;
                (0..EVENTS_SIZE).for_each(|i| self.rom.write8(at + i, 0));
                self.rom.write32(header + 4, GBA_ROM_BASE + at as u32);
                at
            }
        };
        let wide = self.objects_wide();
        for list in List::ALL {
            let count = events.len(list);
            let Some(place) = self.place_list(at, list, count)? else { continue };
            for i in 0..count {
                let o = place + i * list.size();
                match list {
                    List::Objects => write_object(&mut self.rom, o, &events.objects[i], wide),
                    List::Warps => write_warp(&mut self.rom, o, &events.warps[i]),
                    List::Triggers => write_trigger(&mut self.rom, o, &events.triggers[i]),
                    List::Signs => write_sign(&mut self.rom, o, &events.signs[i]),
                }
            }
        }
        Ok(())
    }

    /// Where a list of `count` entries goes, its count and pointer written: where it is
    /// when it fits there, else at the ROM's free end, zeroed (`None` for an empty list)
    fn place_list(&mut self, events: usize, list: List, count: usize) -> Result<Option<usize>, EventsError> {
        let (had, size) = (self.rom.u8(events + list.index()) as usize, list.size());
        let pointer = events + 4 + 4 * list.index();
        self.rom.write8(events + list.index(), count as u8);
        if count == 0 {
            return Ok(None);
        }
        let old = self.rom.ptr(pointer);
        let place = match old {
            Some(old) if count <= had => old,
            // a list moved to the free end before grows there, into the filler after it
            Some(old) if old >= self.opened_free_tail() && self.is_filler(old + had * size, (count - had) * size + MARGIN) => old,
            _ => self.claim(count * size)?,
        };
        if Some(place) != old || count > had {
            // what's new of it begins zeroed, as the games' lists are between their fields
            let from = if Some(place) == old { had * size } else { 0 };
            (from..count * size).for_each(|i| self.rom.write8(place + i, 0));
        }
        if Some(place) != old {
            self.rom.write32(pointer, GBA_ROM_BASE + place as u32);
        }
        Ok(Some(place))
    }

    /// Room for `size` bytes at the ROM's free end
    fn claim(&self, size: usize) -> Result<usize, EventsError> {
        let at = (self.rom.free_tail() + MARGIN).next_multiple_of(4);
        if at + size > self.rom.len() {
            return Err(EventsError::NoRoom);
        }
        Ok(at)
    }

    /// Are these bytes the ROM's filler (0xFF), unused?
    fn is_filler(&self, at: usize, size: usize) -> bool {
        self.rom.bytes().get(at..at + size).is_some_and(|bytes| bytes.iter().all(|&b| b == 0xFF))
    }

    /// Where the ROM's free end began when it was opened: what's after was put there here
    fn opened_free_tail(&self) -> usize {
        *self.opened_free.get_or_init(|| self.original().free_tail())
    }

    /// A script that says a text in a message box, with a standard script
    /// (`MSGBOX_NPC`, `MSGBOX_SIGN`), written at the ROM's free end with its text:
    /// `msgbox text, std` then `end`. Returns its address; to be committed.
    pub fn new_message_script(&mut self, message: &str, std: u8) -> Result<u32, EventsError> {
        let mut bytes = text::encode_message(message);
        bytes.push(text::TERMINATOR);
        let at = self.claim(12 + bytes.len())?;
        let said = GBA_ROM_BASE + (at + 12) as u32;
        // loadword 0, text; callstd std; end
        let script = [&[0x0F, 0x00][..], &said.to_le_bytes(), &[0x09, std, 0x02, 0, 0, 0]].concat();
        for (i, b) in script.into_iter().chain(bytes).enumerate() {
            self.rom.write8(at + i, b);
        }
        let address = GBA_ROM_BASE + at as u32;
        self.made_scripts.insert(address, MadeScript { text: message.to_string(), std });
        Ok(address)
    }

    /// The scripts the editor made, by address
    pub fn made_script(&self, script: u32) -> Option<&MadeScript> {
        self.made_scripts.get(&script)
    }

    /// What a script does: a text said, a trainer's battle, something else
    pub fn script_view(&self, script: u32) -> ScriptView {
        let rom = &self.rom;
        let Some(start) = script.checked_sub(GBA_ROM_BASE).map(|o| o as usize).filter(|&o| o < rom.len()) else {
            return if script == 0 { ScriptView::None } else { ScriptView::Other };
        };
        let address = |o: usize| (rom.u32(o)).checked_sub(GBA_ROM_BASE).map(|o| o as usize).filter(|&o| o < rom.len());
        let message = |o: usize| text::decode_message(rom, o, 2048);
        // trainerbattle type, trainer, local id, intro text…
        if rom.u8(start) == 0x5C {
            return ScriptView::Battle { trainer: rom.u16(start + 2), intro: address(start + 6).and_then(message) };
        }
        // lock, lockall, faceplayer, then msgbox: loadword 0, text; callstd std
        let mut at = start;
        while at < start + 3 && matches!(rom.u8(at), 0x6A | 0x69 | 0x5A) {
            at += 1;
        }
        if rom.u8(at) != 0x0F || rom.u8(at + 1) != 0x00 || rom.u8(at + 6) != 0x09 {
            return ScriptView::Other;
        }
        let Some(said) = address(at + 2).and_then(message) else { return ScriptView::Other };
        // then release, releaseall, end
        let mut end = at + 8;
        while end < at + 10 && matches!(rom.u8(end), 0x6C | 0x6B) {
            end += 1;
        }
        ScriptView::Says(Says { text: said, std: rom.u8(at + 7), only: rom.u8(end) == 0x02 })
    }

    /// The next hidden item flag no hidden item of any map uses
    pub fn next_hidden_item_flag(&self) -> u16 {
        let family = self.family;
        let used = (0..self.maps.len()).flat_map(|i| self.events(i).signs).filter(|s| s.kind == HIDDEN_ITEM);
        used.map(|s| HiddenItem::of(family, s.data).flag + 1).max().unwrap_or(0)
    }
}
