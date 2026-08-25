use std::fs;
use std::io::Write;
use std::path::PathBuf;

use time::OffsetDateTime;

use super::db::data_dir;

fn log_path() -> PathBuf {
    data_dir().join("gnosis.log")
}

pub fn log(message: &str) {
    tracing::info!("{message}");

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
