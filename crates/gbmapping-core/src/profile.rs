//! A profile: what a ROM can't tell by itself. Its maps' names (a decompilation's
//! `MAP_` names aren't in the ROM), addresses for the rare ROM the scan gets wrong, and
//! the maps' originals: a hack that redraws another game's maps with its own tilesets
//! can keep each map as it was, on the tilesets it came with, in a layout no map uses.
//! The profile says which layout that is, for the editor to show it next to the map.
//!
//! ```json
//! {
//!   "game": "pokeemerald",
//!   "name": "My hack",
//!   "gMapGroups": 7006500,
//!   "gRegionMapEntries": 8585828,
//!   "gMapLayouts": 8069344,
//!   "mapsec16": true,
//!   "groups": [{"name": "gMapGroup_TownsAndRoutes", "maps": [
//!     {"name": "PetalburgCity"}, "SlateportCity", {"name": "PalletTown", "layout": 602, "original": 878}]}]
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Profile {
    /// The family: "pokeemerald", "pokefirered", "pokeruby" (else told by the ROM's game code)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// ROM offset of the map groups table (else found)
    #[serde(rename = "gMapGroups", skip_serializing_if = "Option::is_none")]
    pub map_groups: Option<usize>,
    /// ROM offset of the region map's names (else found)
    #[serde(rename = "gRegionMapEntries", skip_serializing_if = "Option::is_none")]
    pub region_map_entries: Option<usize>,
    /// ROM offset of the layouts table: its entry `id - 1` points to the layout `id`
    #[serde(rename = "gMapLayouts", skip_serializing_if = "Option::is_none")]
    pub map_layouts: Option<usize>,
    /// The map headers' region map section is 16 bits wide
    pub mapsec16: bool,
    pub groups: Vec<ProfileGroup>,
    /// The region map sections' names, when the ROM's can't be read
    #[serde(rename = "regionNames", skip_serializing_if = "Option::is_none")]
    pub region_names: Option<RegionNames>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct ProfileGroup {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub maps: Vec<ProfileMap>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ProfileMap {
    Name(String),
    Map {
        name: String,
        /// The map's layout, as an id of the layouts table
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layout: Option<usize>,
        /// The layout that keeps the map as it was before it was redrawn
        #[serde(default, skip_serializing_if = "Option::is_none")]
        original: Option<usize>,
    },
}

impl ProfileMap {
    pub fn name(&self) -> &str {
        match self {
            Self::Name(name) | Self::Map { name, .. } => name,
        }
    }

    pub fn original(&self) -> Option<usize> {
        match self {
            Self::Name(_) => None,
            Self::Map { original, .. } => *original,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RegionNames {
    List(Vec<Option<String>>),
    Map(BTreeMap<String, String>),
}

impl RegionNames {
    pub fn get(&self, mapsec: u16) -> Option<&str> {
        match self {
            Self::List(names) => names.get(mapsec as usize)?.as_deref(),
            Self::Map(names) => names.get(&mapsec.to_string()).map(String::as_str),
        }
    }
}

impl Profile {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let value: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("not a profile: {e}"))?;
        if !value.is_object() {
            return Err("not a profile: not a JSON object".to_string());
        }
        serde_json::from_value(value).map_err(|e| format!("not a profile: {e}"))
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn map_name(&self, group: usize, num: usize) -> Option<&str> {
        self.groups.get(group)?.maps.get(num).map(ProfileMap::name).filter(|n| !n.is_empty())
    }

    /// The id, in the layouts table, of the layout that keeps a map as it was
    pub fn original_layout(&self, group: usize, num: usize) -> Option<usize> {
        self.groups.get(group)?.maps.get(num)?.original()
    }

    /// "gMapGroup_TownsAndRoutes" -> "TownsAndRoutes"
    pub fn group_name(&self, group: usize) -> Option<&str> {
        let name = self.groups.get(group)?.name.as_str();
        let name = name.strip_prefix("gMapGroup_").unwrap_or(name);
        (!name.is_empty()).then_some(name)
    }

    /// The groups' sizes, when the profile lists their maps
    pub fn group_sizes(&self) -> Vec<usize> {
        self.groups.iter().map(|g| g.maps.len()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_readme_example() {
        let p = Profile::from_json(
            r#"{"game": "pokeemerald", "name": "My hack", "gMapGroups": 7006500, "gRegionMapEntries": 8585828, "mapsec16": true,
                "groups": [{"name": "gMapGroup_TownsAndRoutes", "maps": [{"name": "PetalburgCity"}, "SlateportCity"]}]}"#,
        )
        .unwrap();
        assert_eq!(p.map_groups, Some(7006500));
        assert!(p.mapsec16);
        assert_eq!(p.map_name(0, 0), Some("PetalburgCity"));
        assert_eq!(p.map_name(0, 1), Some("SlateportCity"));
        assert_eq!(p.map_name(0, 2), None);
        assert_eq!(p.group_name(0), Some("TownsAndRoutes"));
        assert_eq!(p.group_sizes(), [2]);
        assert_eq!((p.map_layouts, p.original_layout(0, 0)), (None, None));
    }

    #[test]
    fn the_maps_originals() {
        let p = Profile::from_json(
            r#"{"gMapLayouts": 8069344, "groups": [{"name": "Kanto", "maps": [
                {"name": "PalletTown", "layout": 602, "original": 878}, {"name": "Route1", "layout": null}, "Route2"]}]}"#,
        )
        .unwrap();
        assert_eq!(p.map_layouts, Some(8069344));
        assert_eq!(p.original_layout(0, 0), Some(878));
        assert_eq!(p.original_layout(0, 1), None);
        assert_eq!(p.original_layout(0, 2), None);
        assert_eq!(p.original_layout(3, 0), None);
        assert_eq!(p.map_name(0, 1), Some("Route1"));
        // written back, a map without an original stays as short as it was
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#"{"name":"Route1"}"#), "{json}");
        assert_eq!(Profile::from_json(&json).unwrap(), p);
    }

    #[test]
    fn empty_and_unknown_fields() {
        assert!(Profile::from_json("{}").unwrap().is_empty());
        assert!(Profile::from_json(r#"{"something": 1}"#).unwrap().is_empty());
        assert!(Profile::from_json("[]").is_err());
        let p = Profile::from_json(r#"{"regionNames": {"3": "OLDALE"}}"#).unwrap();
        assert_eq!(p.region_names.unwrap().get(3), Some("OLDALE"));
    }
}
