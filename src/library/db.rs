use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use uuid::Uuid;

use super::models::Book;

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("gnosis")
}

pub fn covers_dir() -> PathBuf {
    data_dir().join("covers")
}

pub fn init_db() -> Result<Connection> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir).context("creating data directory")?;
    std::fs::create_dir_all(covers_dir()).context("creating covers directory")?;

    let conn = Connection::open(dir.join("library.db")).context("opening library database")?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS books (
            id            TEXT PRIMARY KEY,
            title         TEXT NOT NULL,
            author        TEXT,
            series        TEXT,
            series_index  REAL,
            path          TEXT NOT NULL UNIQUE,
            format        TEXT NOT NULL,
            cover_path    TEXT,
            added_at      INTEGER NOT NULL,
            progress      REAL NOT NULL DEFAULT 0
        )",
        [],
    )
    .context("creating books table")?;

    Ok(conn)
}

pub fn insert_book(conn: &Connection, book: &Book) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO books
            (id, title, author, series, series_index, path, format, cover_path, added_at, progress)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            book.id.to_string(),
            book.title,
            book.author,
            book.series,
            book.series_index,
            book.path.to_string_lossy(),
            book.format,
            book.cover_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            book.added_at,
            book.progress,
        ],
    )
    .context("inserting book")?;
    Ok(())
}

pub fn list_books(conn: &Connection) -> Result<Vec<Book>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, author, series, series_index, path, format, cover_path, added_at, progress
         FROM books ORDER BY title COLLATE NOCASE ASC",
    )?;

    let rows = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let path: String = row.get(5)?;
        let cover_path: Option<String> = row.get(7)?;

        Ok(Book {
            id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
            title: row.get(1)?,
            author: row.get(2)?,
            series: row.get(3)?,
            series_index: row.get(4)?,
            path: PathBuf::from(path),
            format: row.get(6)?,
            cover_path: cover_path.map(PathBuf::from),
            added_at: row.get(8)?,
            progress: row.get(9)?,
        })
    })?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("listing books")
}

pub fn delete_book(conn: &Connection, id: Uuid) -> Result<()> {
    conn.execute("DELETE FROM books WHERE id = ?1", params![id.to_string()])
        .context("deleting book")?;
    Ok(())
}

pub fn update_progress(conn: &Connection, id: Uuid, progress: f64) -> Result<()> {
    conn.execute(
        "UPDATE books SET progress = ?1 WHERE id = ?2",
        params![progress, id.to_string()],
    )
    .context("updating progress")?;
    Ok(())
}

pub fn book_exists_at(conn: &Connection, path: &Path) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM books WHERE path = ?1",
        params![path.to_string_lossy()],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}
