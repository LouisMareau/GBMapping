//! The Pokémon GBA games' text encoding (the Western charmap): enough for names.

use crate::rom::Rom;

pub const TERMINATOR: u8 = 0xFF;

/// byte -> character ('\0' where the byte isn't a plain character)
pub static CHARMAP: [char; 256] = charmap();

const fn charmap() -> [char; 256] {
    const fn run<const N: usize>(mut t: [char; 256], start: usize, chars: [char; N]) -> [char; 256] {
        let mut i = 0;
        while i < N {
            t[start + i] = chars[i];
            i += 1;
        }
        t
    }
    let mut t = ['\0'; 256];
    t[0x00] = ' ';
    t = run(t, 0x01, ['À', 'Á', 'Â', 'Ç', 'È', 'É', 'Ê', 'Ë', 'Ì']);
    t = run(t, 0x0B, ['Î', 'Ï', 'Ò', 'Ó', 'Ô', 'Œ', 'Ù', 'Ú', 'Û', 'Ñ', 'ß', 'à', 'á']);
    t = run(t, 0x19, ['ç', 'è', 'é', 'ê', 'ë', 'ì']);
    t = run(t, 0x20, ['î', 'ï', 'ò', 'ó', 'ô', 'œ', 'ù', 'ú', 'û', 'ñ', 'º', 'ª']);
    t = run(t, 0x2D, ['&', '+']);
    t = run(t, 0x35, ['=', ';']);
    t = run(t, 0x51, ['¿', '¡']);
    t = run(t, 0x5A, ['Í', '%', '(', ')']);
    t[0x68] = 'â';
    t[0x6F] = 'í';
    t = run(t, 0x85, ['<', '>']);
    t = run(
        t,
        0xA1,
        ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '!', '?', '.', '-', '·', '…', '“', '”', '‘', '’', '♂', '♀', '¥', ',', '×', '/'],
    );
    let mut i = 0;
    while i < 26 {
        t[0xBB + i] = (b'A' + i as u8) as char;
        t[0xD5 + i] = (b'a' + i as u8) as char;
        i += 1;
    }
    t = run(t, 0xEF, ['▶', ':', 'Ä', 'Ö', 'Ü', 'ä', 'ö', 'ü']);
    t
}

/// The text at `offset`, up to its terminator: `None` when there's no terminator within
/// `max` bytes or when a byte isn't a plain character (so: not a name)
pub fn decode(rom: &Rom, offset: usize, max: usize) -> Option<String> {
    let mut s = String::new();
    let mut i = 0;
    while i < max {
        let c = *rom.bytes().get(offset + i)?;
        i += 1;
        match c {
            TERMINATOR => return Some(s),
            PLACEHOLDER => {
                s.push_str(PLACEHOLDERS.get(*rom.bytes().get(offset + i)? as usize)?);
                i += 1;
            }
            _ => match CHARMAP[c as usize] {
                '\0' => return None,
                ch => s.push(ch),
            },
        }
    }
    None
}

/// A word the game fills in: Emerald's "{AQUA} HIDEOUT" is such a name
const PLACEHOLDER: u8 = 0xFD;
const PLACEHOLDERS: [&str; 14] =
    ["", "PLAYER", "…", "…", "…", "", "RIVAL", "VERSION", "AQUA", "MAGMA", "ARCHIE", "MAXIE", "KYOGRE", "GROUDON"];

/// The bytes of a text (without the terminator); characters outside the charmap become spaces
pub fn encode(s: &str) -> Vec<u8> {
    s.chars().map(|ch| CHARMAP.iter().position(|&c| c == ch).unwrap_or(0) as u8).collect()
}

/// Could this be a place's name? Short, starting with a letter or a digit.
pub fn is_name(s: &str) -> bool {
    (2..=24).contains(&s.chars().count()) && s.chars().next().is_some_and(char::is_alphanumeric)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let s = "LITTLEROOT TOWN";
        let mut bytes = encode(s);
        assert_eq!(bytes[0], 0xC6);
        bytes.push(TERMINATOR);
        let rom = Rom::new(bytes, "t");
        assert_eq!(decode(&rom, 0, 32).as_deref(), Some(s));
        assert_eq!(decode(&rom, 0, 8), None, "no terminator in reach");
    }

    #[test]
    fn accents_and_marks() {
        let s = "BOURG-EN-VOL é ROUTE 101 ’";
        let mut bytes = encode(s);
        assert_eq!((bytes[5], bytes[13], bytes[20], bytes[25]), (0xAE, 0x1B, 0x00, 0xB4));
        bytes.push(TERMINATOR);
        assert_eq!(decode(&Rom::new(bytes, "t"), 0, 64).as_deref(), Some(s));
    }

    #[test]
    fn names() {
        assert!(is_name("ROUTE 101"));
        assert!(is_name("S.S. TIDAL"));
        assert!(!is_name(""));
        assert!(!is_name("?"));
        assert!(!is_name("  "));
    }
}
