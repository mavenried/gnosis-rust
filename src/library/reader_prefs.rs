use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::db::data_dir;

/// The reader's display preferences (theme, font, size) — remembered across
/// books and app restarts, since re-picking them every session would be
/// annoying. Deliberately separate from the reading-position/library data
/// in SQLite: this is pure UI preference, not library content.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReaderPrefs {
    pub theme: String,
    pub font_family: Option<String>,
    pub font_size: i32,
}

impl Default for ReaderPrefs {
    fn default() -> Self {
        Self {
            theme: "light".to_string(),
            font_family: None,
            font_size: 100,
        }
    }
}

fn prefs_path() -> PathBuf {
    data_dir().join("reader_prefs.json")
}

pub fn load() -> ReaderPrefs {
    fs::read_to_string(prefs_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(prefs: &ReaderPrefs) {
    let _ = fs::create_dir_all(data_dir());
    if let Ok(text) = serde_json::to_string_pretty(prefs) {
        let _ = fs::write(prefs_path(), text);
    }
}
