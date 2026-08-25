use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::db::data_dir;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LibraryPrefs {
    pub sort_key: String,
}

impl Default for LibraryPrefs {
    fn default() -> Self {
        Self {
            sort_key: "title".to_string(),
        }
    }
}

fn prefs_path() -> PathBuf {
    data_dir().join("library_prefs.json")
}

pub fn load() -> LibraryPrefs {
    fs::read_to_string(prefs_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(prefs: &LibraryPrefs) {
    let _ = fs::create_dir_all(data_dir());
    if let Ok(text) = serde_json::to_string_pretty(prefs) {
        let _ = fs::write(prefs_path(), text);
    }
}
