//! Finding a ROM's tables by their shape, so that an original game, a build of a
//! decompilation and a hack all open without anyone giving addresses.

use crate::format::Family;
use crate::rom::Rom;
use crate::structs::Reader;
use crate::text;
use std::collections::HashMap;

/// The map groups table (`gMapGroups`): each group's map headers
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapGroups {
    pub table: usize,
    /// Per group: where its array of header pointers is, and the headers
    pub groups: Vec<(usize, Vec<usize>)>,
}

impl MapGroups {
    pub fn map_count(&self) -> usize {
        self.groups.iter().map(|(_, headers)| headers.len()).sum()
    }
}

const MAX_GROUPS: usize = 256;
const MAX_MAPS_IN_GROUP: usize = 256;

struct GroupScanner<'a> {
    reader: Reader<'a>,
    headers: HashMap<usize, bool>,
}

impl GroupScanner<'_> {
    fn is_header(&mut self, o: usize) -> bool {
        let reader = self.reader;
        *self.headers.entry(o).or_insert_with(|| reader.header(o).is_some())
    }

    /// The word at `o` points to an array that starts with a map header's pointer
    fn group_at(&mut self, o: usize) -> Option<usize> {
        let array = self.reader.aligned(o)?;
        let first = self.reader.aligned(array)?;
        self.is_header(first).then_some(array)
    }

    /// The table at `table`: its groups, each running until the next group's array
    /// (the counts aren't in the ROM) or until the pointers stop being map headers.
    /// `limits`, when given, are the groups' sizes.
    fn read(&mut self, table: usize, limits: &[usize]) -> MapGroups {
        let mut starts = Vec::new();
        while starts.len() < MAX_GROUPS {
            match self.group_at(table + starts.len() * 4) {
                Some(array) if !starts.contains(&array) => starts.push(array),
                _ => break,
            }
        }
        let mut bounds = starts.clone();
        bounds.push(table);
        bounds.sort_unstable();
        let groups = starts
            .iter()
            .enumerate()
            .map(|(g, &start)| {
                let end = bounds.iter().copied().find(|&b| b > start).unwrap_or(usize::MAX);
                let limit = limits.get(g).copied().unwrap_or(MAX_MAPS_IN_GROUP).min(MAX_MAPS_IN_GROUP);
                let mut headers = Vec::new();
                while headers.len() < limit && start + headers.len() * 4 < end {
                    match self.reader.aligned(start + headers.len() * 4) {
                        Some(h) if self.is_header(h) => headers.push(h),
                        _ => break,
                    }
                }
                (start, headers)
            })
            .collect();
        MapGroups { table, groups }
    }
}

/// The map groups table at a known address
pub fn read_map_groups(reader: Reader, table: usize, limits: &[usize]) -> Option<MapGroups> {
    let groups = GroupScanner { reader, headers: HashMap::new() }.read(table, limits);
    (groups.map_count() > 0).then_some(groups)
}

/// The map groups table, found: the run of group pointers that gives the most maps
pub fn find_map_groups(reader: Reader) -> Option<MapGroups> {
    let mut scanner = GroupScanner { reader, headers: HashMap::new() };
    let mut best: Option<MapGroups> = None;
    let mut o = 0;
    while o + 4 <= reader.rom.len() {
        if scanner.group_at(o).is_none() {
            o += 4;
            continue;
        }
        let groups = scanner.read(o, &[]);
        o += groups.groups.len().max(1) * 4;
        if groups.map_count() > best.as_ref().map_or(0, MapGroups::map_count) {
            best = Some(groups);
        }
    }
    best
}

/// Where the games keep their places' names, by region map section
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionNames {
    pub table: usize,
    /// Bytes per entry, and where the name's pointer is in it
    pub stride: usize,
    pub name: usize,
    /// The section the table starts with
    pub first: u16,
    pub count: usize,
}

const NAME_MAX_BYTES: usize = 32;
const MIN_SECTIONS: usize = 32;

impl SectionNames {
    /// Hoenn's games: `struct RegionMapLocation { u8 x, y, width, height; const u8 *name; }` per section
    fn hoenn(table: usize, count: usize) -> Self {
        Self { table, stride: 8, name: 4, first: 0, count }
    }

    /// Kanto's games: a name pointer per section, from the first Kanto section on
    fn kanto(table: usize, count: usize) -> Self {
        Self { table, stride: 4, name: 0, first: 0x58, count }
    }

    pub fn get(&self, rom: &Rom, mapsec: u16) -> Option<String> {
        let i = (mapsec as usize).checked_sub(self.first as usize)?;
        if i >= self.count {
            return None;
        }
        let name = text::decode(rom, rom.ptr(self.table + i * self.stride + self.name)?, NAME_MAX_BYTES)?;
        text::is_name(&name).then_some(name)
    }
}

fn hoenn_entry(rom: &Rom, o: usize) -> bool {
    let b = match rom.bytes().get(o..o + 4) {
        Some(b) => b,
        None => return false,
    };
    b[0] <= 64
        && b[1] <= 64
        && (1..=16).contains(&b[2])
        && (1..=16).contains(&b[3])
        && rom.ptr(o + 4).is_some_and(|p| text::decode(rom, p, NAME_MAX_BYTES).is_some())
}

fn kanto_entry(rom: &Rom, o: usize) -> bool {
    rom.ptr(o).and_then(|p| text::decode(rom, p, NAME_MAX_BYTES)).is_some_and(|s| text::is_name(&s) && !s.chars().any(char::is_lowercase))
}

/// The longest run of entries `stride` bytes apart that `valid` accepts
fn longest_run(rom: &Rom, stride: usize, valid: impl Fn(&Rom, usize) -> bool) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    // an entry already counted in a run isn't the start of a longer one
    let mut counted_until = vec![0usize; stride / 4];
    let mut o = 0;
    while o + stride <= rom.len() {
        let phase = (o / 4) % (stride / 4);
        if o >= counted_until[phase] && valid(rom, o) {
            let mut count = 1;
            while valid(rom, o + count * stride) {
                count += 1;
            }
            counted_until[phase] = o + count * stride;
            if count > best.map_or(0, |(_, c)| c) {
                best = Some((o, count));
            }
        }
        o += 4;
    }
    best
}

/// The places' names table: at `known` when that holds one, else found by its shape
pub fn find_section_names(rom: &Rom, family: Family, known: Option<usize>) -> Option<SectionNames> {
    let kanto = family == Family::FireRedLeafGreen;
    let (stride, valid): (usize, fn(&Rom, usize) -> bool) = if kanto { (4, kanto_entry) } else { (8, hoenn_entry) };
    let run_from = |o: usize| (0..).take_while(|i| valid(rom, o + i * stride)).count();
    let (table, count) = match known.map(|o| (o, run_from(o))).filter(|&(_, count)| count > 0) {
        Some(found) => found,
        None => longest_run(rom, stride, valid).filter(|&(_, count)| count >= MIN_SECTIONS)?,
    };
    Some(if kanto { SectionNames::kanto(table, count) } else { SectionNames::hoenn(table, count) })
}
