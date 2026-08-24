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
    /// The reader's resume position: an EPUB CFI string from foliate-js.
    /// Opaque to Rust — only ever round-tripped through the reader.
    pub locator: Option<String>,
}

/// A book's finished-ness. There's no stored "mark as read" flag — this is
/// derived from `progress`/`locator` so the library can be filtered by
/// status without any extra tracking UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadingStatus {
    Unread,
    Reading,
    Read,
}

/// Reflowable EPUBs rarely hit exactly 100% (footnotes, back matter), so
/// "read" is a high-but-not-total threshold, same as most reading apps use.
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
