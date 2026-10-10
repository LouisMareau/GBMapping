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
        Some(out)
    }
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
