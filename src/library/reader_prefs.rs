use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::db::data_dir;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReaderPrefs {
    pub theme: String,
    pub font_family: Option<String>,
    pub font_size: i32,
    #[serde(default = "default_rsvp_wpm")]
    pub rsvp_wpm: u32,
    /// Percentage of a 1x line-height (e.g. 140 = 1.4).
    #[serde(default = "default_line_height")]
    pub line_height: i32,
    /// Percentage of 1em of space below each paragraph.
    #[serde(default = "default_paragraph_spacing")]
    pub paragraph_spacing: i32,
    /// Page side margin, in pixels.
    #[serde(default = "default_margin")]
    pub margin: i32,
    #[serde(default)]
    pub justify: bool,
}

fn default_rsvp_wpm() -> u32 {
    300
}

fn default_line_height() -> i32 {
    140
}

fn default_paragraph_spacing() -> i32 {
    100
}

fn default_margin() -> i32 {
    48
}

impl Default for ReaderPrefs {
    fn default() -> Self {
        Self {
            theme: "light".to_string(),
            font_family: None,
            font_size: 100,
            rsvp_wpm: default_rsvp_wpm(),
            line_height: default_line_height(),
            paragraph_spacing: default_paragraph_spacing(),
            margin: default_margin(),
            justify: false,
        }
    }
}

fn prefs_path() -> PathBuf {
    data_dir().join("reader_prefs.json")
}

/// Not a valid `Uuid` string, so it can't collide with a real book id —
/// holds the migrated pre-per-book default (see `load_all`) permanently,
/// rather than that default only surviving until the first real
/// `save_for` overwrites the file's legacy shape.
const DEFAULT_KEY: &str = "__default__";

/// Before prefs became per-book, this file held one flat `ReaderPrefs`
/// object. That shape fails to parse as the per-book map, so on first
/// load here it's recovered separately and folded into the map under
/// `DEFAULT_KEY` — otherwise a pre-existing theme/font choice would
/// silently vanish the moment any book's prefs were first saved (which
/// overwrites the file in the new shape).
fn load_all() -> HashMap<String, ReaderPrefs> {
    let Ok(text) = fs::read_to_string(prefs_path()) else {
        return HashMap::new();
    };
    if let Ok(map) = serde_json::from_str::<HashMap<String, ReaderPrefs>>(&text) {
        return map;
    }
    match serde_json::from_str::<ReaderPrefs>(&text) {
        Ok(legacy_default) => HashMap::from([(DEFAULT_KEY.to_string(), legacy_default)]),
        Err(_) => HashMap::new(),
    }
}

pub fn load_for(id: Uuid) -> ReaderPrefs {
    let mut all = load_all();
    all.remove(&id.to_string())
        .or_else(|| all.remove(DEFAULT_KEY))
        .unwrap_or_default()
}

pub fn save_for(id: Uuid, prefs: &ReaderPrefs) {
    let mut all = load_all();
    all.insert(id.to_string(), prefs.clone());
    let _ = fs::create_dir_all(data_dir());
    if let Ok(text) = serde_json::to_string_pretty(&all) {
        let _ = fs::write(prefs_path(), text);
    }
}
