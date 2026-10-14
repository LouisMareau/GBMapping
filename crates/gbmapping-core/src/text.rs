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

// -- messages ------------------------------------------------------------------------------
//
// What people and signs say: the characters, and what lays a message out. A text box has
// two lines: the first break of a paragraph goes to the second line (0xFE), the next
// ones scroll it up (0xFA), and a new paragraph begins a new box (0xFB). Written out, a
// break is a new line and a new paragraph is an empty line between two, which is how the
// decompilations' texts are laid out; anything else is told between braces.

const SCROLL: u8 = 0xFA;
const PARAGRAPH: u8 = 0xFB;
const CONTROL: u8 = 0xFC;
const NEWLINE: u8 = 0xFE;

/// What the games fill a message in with (0xFD, then its number)
const FILLED_IN: [&str; 14] = [
    "",
    "PLAYER",
    "STR_VAR_1",
    "STR_VAR_2",
    "STR_VAR_3",
    "KUN",
    "RIVAL",
    "VERSION",
    "AQUA",
    "MAGMA",
    "ARCHIE",
    "MAXIE",
    "KYOGRE",
    "GROUDON",
];

/// How many bytes follow 0xFC: its code, and the code's arguments (the games' GetExtCtrlCodeLength)
const CONTROL_LENGTHS: [usize; 25] = [1, 2, 2, 2, 4, 2, 2, 1, 2, 1, 1, 3, 2, 2, 2, 1, 3, 2, 2, 2, 2, 1, 1, 1, 1];

/// Bytes told as they are: in hexadecimal, between braces
fn braced(bytes: &[u8]) -> String {
    format!("{{{}}}", bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" "))
}

/// A message at `offset`, up to its terminator, written out (see above): `None` when
/// there's no terminator within `max` bytes
pub fn decode_message(rom: &Rom, offset: usize, max: usize) -> Option<String> {
    let bytes = rom.bytes().get(offset..(offset.checked_add(max)?).min(rom.len()))?;
    let mut s = String::new();
    let mut i = 0;
    while let Some(&c) = bytes.get(i) {
        i += 1;
        match c {
            TERMINATOR => return Some(s),
            NEWLINE | SCROLL => s.push('\n'),
            PARAGRAPH => s.push_str("\n\n"),
            PLACEHOLDER => {
                let n = *bytes.get(i)?;
                match FILLED_IN.get(n as usize).filter(|name| !name.is_empty()) {
                    Some(name) => s.push_str(&format!("{{{name}}}")),
                    None => s.push_str(&braced(&[c, n])),
                }
                i += 1;
            }
            CONTROL => {
                let n = CONTROL_LENGTHS.get(*bytes.get(i)? as usize).copied().unwrap_or(1);
                s.push_str(&braced(bytes.get(i - 1..i + n)?));
                i += n;
            }
            // a symbol, the keypad's buttons, a character of the moment: one byte follows
            0xF7..=0xF9 => {
                s.push_str(&braced(bytes.get(i - 1..i + 1)?));
                i += 1;
            }
            _ => match CHARMAP[c as usize] {
                '\0' => s.push_str(&braced(&[c])),
                ch => s.push(ch),
            },
        }
    }
    None
}

/// The bytes of a message written out (see above), without its terminator. A
/// character the games don't have becomes a space; ' and " become the games' quotes.
pub fn encode_message(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let s = s.replace("\r\n", "\n");
    for (p, paragraph) in s.trim_end_matches('\n').split("\n\n").enumerate() {
        if p > 0 {
            out.push(PARAGRAPH);
        }
        let mut quoted = false;
        for (l, line) in paragraph.split('\n').enumerate() {
            if l > 0 {
                out.push(if l == 1 { NEWLINE } else { SCROLL });
            }
            let mut rest = line;
            while let Some(ch) = rest.chars().next() {
                // between braces: a name the game fills in, or bytes in hexadecimal
                if let Some(end) = rest.find('}').filter(|_| ch == '{') {
                    let inside = &rest[1..end];
                    let hex: Option<Vec<u8>> = inside.split_whitespace().map(|b| u8::from_str_radix(b, 16).ok()).collect();
                    if let Some(n) = FILLED_IN.iter().position(|name| !name.is_empty() && *name == inside) {
                        out.extend([PLACEHOLDER, n as u8]);
                        rest = &rest[end + 1..];
                        continue;
                    }
                    if let Some(bytes) = hex.filter(|bytes| !bytes.is_empty()) {
                        out.extend(bytes);
                        rest = &rest[end + 1..];
                        continue;
                    }
                }
                let byte = match ch {
                    '\'' => 0xB4,
                    '"' => {
                        quoted = !quoted;
                        if quoted {
                            0xB1
                        } else {
                            0xB2
                        }
                    }
                    _ => CHARMAP.iter().position(|&c| c == ch).unwrap_or(0) as u8,
                };
                out.push(byte);
                rest = &rest[ch.len_utf8()..];
            }
        }
    }
    out
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
    fn messages() {
        let s = "Hi, {PLAYER}!\nIt's me.\nThird line\n\nA new box. {FC 01 04}Red";
        let mut bytes = encode_message(s);
        assert_eq!(&bytes[..6], &[0xC2, 0xDD, 0xB8, 0x00, PLACEHOLDER, 1]);
        assert_eq!(bytes.iter().filter(|&&b| b == NEWLINE).count(), 1);
        assert_eq!(bytes.iter().filter(|&&b| b == SCROLL).count(), 1);
        assert_eq!(bytes.iter().filter(|&&b| b == PARAGRAPH).count(), 1);
        assert!(bytes.windows(3).any(|w| w == [CONTROL, 0x01, 0x04]));
        bytes.push(TERMINATOR);
        let rom = Rom::new(bytes, "t");
        assert_eq!(decode_message(&rom, 0, 200).as_deref(), Some(s.replace("It's", "It’s").as_str()));
        assert_eq!(decode_message(&rom, 0, 6), None, "no terminator in reach");
        // what the games have that isn't a character is kept
        let rom = Rom::new(vec![0xBB, 0x53, 0x54, 0xF9, 0x05, 0xFF], "t");
        assert_eq!(decode_message(&rom, 0, 16).as_deref(), Some("A{53}{54}{F9 05}"));
        assert_eq!(encode_message("A{53}{54}{F9 05}"), [0xBB, 0x53, 0x54, 0xF9, 0x05]);
        assert_eq!(encode_message("\"Hi\""), [0xB1, 0xC2, 0xDD, 0xB2]);
        assert_eq!(encode_message("a\r\nb\n"), [0xD5, NEWLINE, 0xD6]);
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
