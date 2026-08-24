use std::fs;
use std::io::Write;
use std::path::PathBuf;

use time::OffsetDateTime;

use super::db::data_dir;

fn log_path() -> PathBuf {
    data_dir().join("gnosis.log")
}

/// Appends a timestamped line to the activity log, shown on the Settings
/// page. Never fails loudly — logging is a best-effort diagnostic aid, not
/// something that should interrupt the operation being logged.
pub fn log(message: &str) {
    let _ = fs::create_dir_all(data_dir());
    let line = format!("[{}] {message}\n", timestamp());
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Reads the whole activity log for display.
pub fn read() -> String {
    fs::read_to_string(log_path()).unwrap_or_default()
}

pub fn clear() {
    let _ = fs::write(log_path(), "");
}

fn timestamp() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        now.year(),
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}
