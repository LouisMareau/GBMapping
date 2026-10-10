//! A profile: what a ROM can't tell by itself. Its maps' names (a decompilation's
//! `MAP_` names aren't in the ROM), and addresses for the rare ROM the scan gets wrong.
//!
//! ```json
//! {
//!   "game": "pokeemerald",
//!   "name": "My hack",
//!   "gMapGroups": 7006500,
//!   "gRegionMapEntries": 8585828,
//!   "mapsec16": true,
//!   "groups": [{"name": "gMapGroup_TownsAndRoutes", "maps": [{"name": "PetalburgCity"}, "SlateportCity"]}]
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
    Map { name: String },
}

impl ProfileMap {
    pub fn name(&self) -> &str {
        match self {
            Self::Name(name) | Self::Map { name } => name,
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
