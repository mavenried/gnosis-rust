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
}

impl Book {
    pub fn now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}
