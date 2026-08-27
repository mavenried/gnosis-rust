use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct Book {
    pub id: Uuid,
    pub title: String,
    pub author: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<f64>,
    pub path: PathBuf,
    pub format: String,
    pub cover_path: Option<PathBuf>,
    pub added_at: i64,
    pub progress: f64,
    pub locator: Option<String>,
    pub last_opened_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadingStatus {
    Unread,
    Reading,
    Read,
}

const READ_THRESHOLD: f64 = 0.97;

impl Book {
    pub fn now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    pub fn reading_status(&self) -> ReadingStatus {
        if self.locator.is_none() && self.progress <= 0.0 {
            ReadingStatus::Unread
        } else if self.progress >= READ_THRESHOLD {
            ReadingStatus::Read
        } else {
            ReadingStatus::Reading
        }
    }
}
