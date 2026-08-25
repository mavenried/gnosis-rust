use std::fs;
use std::path::{Path, PathBuf};

use super::db::data_dir;

fn settings_file() -> PathBuf {
    data_dir().join("library_folders.txt")
}

pub fn list_folders() -> Vec<PathBuf> {
    let Ok(contents) = fs::read_to_string(settings_file()) else {
        return Vec::new();
    };
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect()
}

pub fn add_folder(path: &Path) {
    let mut folders = list_folders();
    if folders.iter().any(|f| f == path) {
        return;
    }
    folders.push(path.to_path_buf());
    save(&folders);
}

pub fn remove_folder(path: &Path) {
    let folders: Vec<_> = list_folders().into_iter().filter(|f| f != path).collect();
    save(&folders);
}

fn save(folders: &[PathBuf]) {
    let _ = fs::create_dir_all(data_dir());
    let content = folders
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let _ = fs::write(settings_file(), content);
}
