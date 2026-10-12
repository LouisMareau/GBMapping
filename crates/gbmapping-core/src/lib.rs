//! GBMapping's core: reads the maps and tilesets of Game Boy Advance ROMs (the Pokémon
//! games of the third generation and their hacks) and edits them in place. No UI and
//! no I/O: it's given a ROM's bytes and gives them back.

pub mod behaviors;
pub mod edits;
pub mod format;
pub mod game;
pub mod gbc;
pub mod gfx;
pub mod map_names;
pub mod metatiles;
pub mod pixels;
pub mod profile;
pub mod rom;
pub mod scan;
pub mod structs;
pub mod text;
pub mod tiles;
pub mod tilesets;

pub use edits::Edits;
pub use format::{block, Family, Format};
pub use game::{closest_metatile, Game, Image, Map, MapId, MapInfo, OpenError};
pub use profile::Profile;
pub use rom::Rom;
pub use structs::Pair;
