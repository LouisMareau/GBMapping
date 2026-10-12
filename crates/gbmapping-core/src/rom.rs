//! A ROM in memory: little-endian readers, GBA pointers, LZ77, journaled writes.
//!
//! Readers never panic: a read outside the ROM gives 0, a pointer outside it
//! gives `None`. A hack's data can be anything.

pub const GBA_ROM_BASE: u32 = 0x0800_0000;

/// One byte changed by an edit
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub offset: u32,
    pub old: u8,
    pub new: u8,
}

/// What one edit changed in the ROM: undone by writing the old bytes back
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Patch {
    pub changes: Vec<Change>,
}

impl Patch {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RomHeader {
    pub title: String,
    pub code: String,
    pub maker: String,
    pub version: u8,
}

#[derive(Clone)]
pub struct Rom {
    bytes: Vec<u8>,
    pub name: String,
    journal: Vec<Change>,
}

impl Rom {
    pub fn new(bytes: Vec<u8>, name: impl Into<String>) -> Self {
        Self { bytes, name: name.into(), journal: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn u8(&self, o: usize) -> u8 {
        self.bytes.get(o).copied().unwrap_or(0)
    }

    pub fn u16(&self, o: usize) -> u16 {
        match self.bytes.get(o..o.wrapping_add(2)) {
            Some(b) => u16::from_le_bytes([b[0], b[1]]),
            None => 0,
        }
    }

    pub fn u32(&self, o: usize) -> u32 {
        match self.bytes.get(o..o.wrapping_add(4)) {
            Some(b) => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            None => 0,
        }
    }

    /// The GBA ROM pointer (0x08xxxxxx) at offset `o`, as a ROM offset
    pub fn ptr(&self, o: usize) -> Option<usize> {
        if o.checked_add(4)? > self.bytes.len() {
            return None;
        }
        let v = self.u32(o);
        let p = v.checked_sub(GBA_ROM_BASE)? as usize;
        (p < self.bytes.len()).then_some(p)
    }

    /// Is the word at `o` zero (a NULL pointer)?
    pub fn is_null(&self, o: usize) -> bool {
        o + 4 <= self.bytes.len() && self.u32(o) == 0
    }

    fn ascii(&self, o: usize, length: usize) -> String {
        (0..length)
            .map(|i| self.u8(o + i))
            .take_while(|&c| c != 0)
            .map(|c| if c.is_ascii_graphic() || c == b' ' { c as char } else { '?' })
            .collect()
    }

    /// The GBA cartridge header
    pub fn header(&self) -> RomHeader {
        RomHeader { title: self.ascii(0xA0, 12), code: self.ascii(0xAC, 4), maker: self.ascii(0xB0, 2), version: self.u8(0xBC) }
    }

    /// Does this look like a GBA ROM? (the header's fixed byte)
    pub fn is_gba(&self) -> bool {
        self.bytes.len() >= 0xC0 && self.u8(0xB2) == 0x96
    }

    // -- writes ---------------------------------------------------------------

    pub fn write8(&mut self, o: usize, v: u8) {
        if let Some(b) = self.bytes.get_mut(o) {
            if *b != v {
                self.journal.push(Change { offset: o as u32, old: *b, new: v });
                *b = v;
            }
        }
    }

    pub fn write16(&mut self, o: usize, v: u16) {
        let b = v.to_le_bytes();
        self.write8(o, b[0]);
        self.write8(o + 1, b[1]);
    }

    pub fn write32(&mut self, o: usize, v: u32) {
        for (i, b) in v.to_le_bytes().into_iter().enumerate() {
            self.write8(o + i, b);
        }
    }

    /// Everything written since the last call, as one undoable patch
    pub fn take_patch(&mut self) -> Patch {
        Patch { changes: std::mem::take(&mut self.journal) }
    }

    /// Writes a patch's old bytes back (not journaled)
    pub fn revert(&mut self, patch: &Patch) {
        for c in patch.changes.iter().rev() {
            if let Some(b) = self.bytes.get_mut(c.offset as usize) {
                *b = c.old;
            }
        }
    }

    /// Writes a patch's new bytes again (not journaled)
    pub fn reapply(&mut self, patch: &Patch) {
        for c in &patch.changes {
            if let Some(b) = self.bytes.get_mut(c.offset as usize) {
                *b = c.new;
            }
        }
    }

    // -- compression ------------------------------------------------------------

    /// GBA BIOS LZ77 (type 0x10): the decompressed bytes, or `None` if it isn't LZ77 data
    pub fn lz77(&self, o: usize) -> Option<Vec<u8>> {
        self.lz77_read(o).map(|(data, _)| data)
    }

    /// How many bytes the LZ77 data at `o` takes in the ROM
    pub fn lz77_len(&self, o: usize) -> Option<usize> {
        self.lz77_read(o).map(|(_, len)| len)
    }

    /// Where the ROM's unused end begins: after its last byte that isn't 0xFF, the
    /// filler the games' cartridges and the decompilations' builds end with
    pub fn free_tail(&self) -> usize {
        self.bytes.iter().rposition(|&b| b != 0xFF).map_or(0, |last| last + 1)
    }

    fn lz77_read(&self, o: usize) -> Option<(Vec<u8>, usize)> {
        let b = &self.bytes;
        if o.checked_add(4)? > b.len() || b[o] != 0x10 {
            return None;
        }
        let size = (self.u32(o) >> 8) as usize;
        if size == 0 || size > 0x20_0000 {
            return None;
        }
        let mut out = Vec::with_capacity(size);
        let mut src = o + 4;
        while out.len() < size {
            let flags = *b.get(src)?;
            src += 1;
            for bit in (0..8).rev() {
                if out.len() >= size {
                    break;
                }
                if flags & (1 << bit) != 0 {
                    let b1 = *b.get(src)? as usize;
                    let b2 = *b.get(src + 1)? as usize;
                    src += 2;
                    let length = (b1 >> 4) + 3;
                    let disp = (((b1 & 0xF) << 8) | b2) + 1;
                    if disp > out.len() {
                        return None;
                    }
                    for _ in 0..length {
                        if out.len() >= size {
                            break;
                        }
                        out.push(out[out.len() - disp]);
                    }
                } else {
                    out.push(*b.get(src)?);
                    src += 1;
                }
            }
        }
        Some((out, src - o))
    }
}

/// Compresses data as GBA BIOS LZ77 (type 0x10), the way the games' own data is: no
/// match begins one byte back, so that the console can also decompress it straight into
/// video memory. Padded to a multiple of 4 bytes.
pub fn lz77_compress(data: &[u8]) -> Vec<u8> {
    const MIN: usize = 3;
    const MAX: usize = 18;
    const WINDOW: usize = 4096;
    const TRIES: usize = 256;
    let mut out = Vec::with_capacity(data.len() / 2 + 8);
    out.extend_from_slice(&((data.len() as u32) << 8 | 0x10).to_le_bytes());
    // where each run of 3 bytes was last seen, and where the one before that was
    let mut last = vec![usize::MAX; 1 << 15];
    let mut before = vec![usize::MAX; data.len()];
    let key = |i: usize| ((data[i] as usize * 0x9E5) ^ (data[i + 1] as usize * 0x1F3) ^ (data[i + 2] as usize * 0x7)) & 0x7FFF;
    let mut i = 0;
    while i < data.len() {
        let flags_at = out.len();
        out.push(0);
        for bit in (0..8).rev() {
            if i >= data.len() {
                break;
            }
            let (mut length, mut back) = (0, 0);
            if i + MIN <= data.len() {
                let most = MAX.min(data.len() - i);
                let (mut candidate, mut tries) = (last[key(i)], 0);
                while candidate != usize::MAX && i - candidate <= WINDOW && tries < TRIES {
                    if i - candidate >= 2 {
                        let same = (0..most).take_while(|&n| data[candidate + n] == data[i + n]).count();
                        if same > length {
                            (length, back) = (same, i - candidate);
                            if same == most {
                                break;
                            }
                        }
                    }
                    candidate = before[candidate];
                    tries += 1;
                }
            }
            let step = if length >= MIN {
                out[flags_at] |= 1 << bit;
                out.push(((length - 3) << 4 | (back - 1) >> 8) as u8);
                out.push((back - 1) as u8);
                length
            } else {
                out.push(data[i]);
                1
            };
            for at in i..i + step {
                if at + MIN <= data.len() {
                    before[at] = last[key(at)];
                    last[key(at)] = at;
                }
            }
            i += step;
        }
    }
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readers_never_panic() {
        let rom = Rom::new(vec![1, 2, 3], "tiny");
        assert_eq!(rom.u8(9), 0);
        assert_eq!(rom.u16(2), 0);
        assert_eq!(rom.u32(usize::MAX - 1), 0);
        assert_eq!(rom.ptr(0), None);
        assert_eq!(rom.lz77(0), None);
    }

    #[test]
    fn pointers() {
        let mut bytes = vec![0u8; 16];
        bytes[0..4].copy_from_slice(&0x0800_0008u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x0800_0010u32.to_le_bytes());
        let rom = Rom::new(bytes, "p");
        assert_eq!(rom.ptr(0), Some(8));
        assert_eq!(rom.ptr(4), None, "one past the end");
        assert_eq!(rom.ptr(8), None, "NULL");
        assert!(rom.is_null(8));
    }

    #[test]
    fn lz77_round() {
        // "ABABABAB" : two literals, then a copy of 6 from 2 back
        let data = [0x10, 8, 0, 0, 0b0010_0000, b'A', b'B', 0x30, 0x01];
        let rom = Rom::new(data.to_vec(), "lz");
        assert_eq!(rom.lz77(0).as_deref(), Some(&b"ABABABAB"[..]));
    }

    #[test]
    fn lz77_there_and_back() {
        let mut state = 12345u32;
        let mut noise = move || {
            state = state.wrapping_mul(1103515245).wrapping_add(12345);
            (state >> 16) as u8
        };
        let tiles: Vec<u8> = (0..4096).map(|i| if i % 512 < 300 { (i / 32) as u8 } else { noise() & 0x33 }).collect();
        let samples: [&[u8]; 6] =
            [b"A", b"AB", b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", b"ABCABCABCABCABCABCABCABCABCABCABC", &[0; 5000], &tiles];
        for data in samples {
            let packed = lz77_compress(data);
            assert_eq!(packed.len() % 4, 0);
            let mut bytes = packed.clone();
            bytes.extend_from_slice(&[0xFF; 8]);
            let rom = Rom::new(bytes, "lz");
            assert_eq!(rom.lz77(0).as_deref(), Some(data), "{} bytes", data.len());
            let len = rom.lz77_len(0).unwrap();
            assert!(len <= packed.len() && packed.len() - len < 4, "the length read is the length written, less the padding");
            assert_eq!(rom.free_tail(), packed.iter().rposition(|&b| b != 0xFF).unwrap() + 1);
        }
        // runs are packed, and never from one byte back (video memory is written two bytes at a time)
        let packed = lz77_compress(&[7; 5000]);
        assert!(packed.len() < 700, "{} bytes", packed.len());
        let mut at = 4;
        while at < packed.len() - 3 {
            let flags = packed[at];
            at += 1;
            for bit in (0..8).rev() {
                if at + 1 >= packed.len() {
                    break;
                }
                if flags & (1 << bit) != 0 {
                    let back = ((packed[at] as usize & 0xF) << 8 | packed[at + 1] as usize) + 1;
                    assert!(back >= 2);
                    at += 2;
                } else {
                    at += 1;
                }
            }
        }
        assert_eq!(Rom::new(vec![0xFF; 16], "empty").free_tail(), 0);
    }

    #[test]
    fn patches_undo_and_redo() {
        let mut rom = Rom::new(vec![0u8; 8], "w");
        rom.write16(2, 0xBEEF);
        rom.write8(2, 0xEF); // unchanged: not journaled
        let patch = rom.take_patch();
        assert_eq!(patch.changes.len(), 2);
        assert!(rom.take_patch().is_empty());
        rom.revert(&patch);
        assert_eq!(rom.u16(2), 0);
        rom.reapply(&patch);
        assert_eq!(rom.u16(2), 0xBEEF);
    }
}
