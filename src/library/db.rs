use std::collections::HashMap;
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

pub fn collection_covers_dir() -> PathBuf {
    covers_dir().join("collections")
}

pub fn book_cache_dir() -> PathBuf {
    data_dir().join("books")
}

pub fn init_db() -> Result<Connection> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir).context("creating data directory")?;
    std::fs::create_dir_all(covers_dir()).context("creating covers directory")?;
    std::fs::create_dir_all(collection_covers_dir())
        .context("creating collection covers directory")?;
    std::fs::create_dir_all(book_cache_dir()).context("creating book cache directory")?;

    let conn = Connection::open(dir.join("library.db")).context("opening library database")?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
        .context("configuring database pragmas")?;
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
            progress      REAL NOT NULL DEFAULT 0,
            locator       TEXT,
            last_opened_at INTEGER
        )",
        [],
    )
    .context("creating books table")?;

    conn.execute("ALTER TABLE books ADD COLUMN locator TEXT", [])
        .ok();
    conn.execute("ALTER TABLE books ADD COLUMN last_opened_at INTEGER", [])
        .ok();

    conn.execute(
        "CREATE TABLE IF NOT EXISTS collection_covers (
            kind        TEXT NOT NULL,
            name        TEXT NOT NULL,
            cover_path  TEXT NOT NULL,
            PRIMARY KEY (kind, name)
        )",
        [],
    )
    .context("creating collection_covers table")?;

    Ok(conn)
}

pub fn insert_book(conn: &Connection, book: &Book) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO books
            (id, title, author, series, series_index, path, format, cover_path, added_at, progress, locator, last_opened_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
            book.locator,
            book.last_opened_at,
        ],
    )
    .context("inserting book")?;
    Ok(())
}

pub fn list_books(conn: &Connection) -> Result<Vec<Book>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, author, series, series_index, path, format, cover_path, added_at, progress, locator, last_opened_at
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
            locator: row.get(10)?,
            last_opened_at: row.get(11)?,
        })
    })?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("listing books")
}

pub fn touch_last_opened(conn: &Connection, id: Uuid) -> Result<()> {
    conn.execute(
        "UPDATE books SET last_opened_at = ?1 WHERE id = ?2",
        params![Book::now(), id.to_string()],
    )
    .context("updating last opened time")?;
    Ok(())
}

pub fn delete_book(conn: &Connection, id: Uuid) -> Result<()> {
    conn.execute("DELETE FROM books WHERE id = ?1", params![id.to_string()])
        .context("deleting book")?;
    Ok(())
}

pub fn update_reader_position(
    conn: &Connection,
    id: Uuid,
    locator: Option<&str>,
    progress: f64,
) -> Result<()> {
    conn.execute(
        "UPDATE books SET locator = ?1, progress = ?2 WHERE id = ?3",
        params![locator, progress, id.to_string()],
    )
    .context("updating reader position")?;
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

pub fn set_collection_cover(conn: &Connection, kind: &str, name: &str, cover_path: &Path) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO collection_covers (kind, name, cover_path) VALUES (?1, ?2, ?3)",
        params![kind, name, cover_path.to_string_lossy()],
    )
    .context("setting collection cover")?;
    Ok(())
}

pub fn remove_collection_cover(conn: &Connection, kind: &str, name: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM collection_covers WHERE kind = ?1 AND name = ?2",
        params![kind, name],
    )
    .context("removing collection cover")?;
    Ok(())
}

pub fn all_collection_covers(conn: &Connection, kind: &str) -> Result<HashMap<String, PathBuf>> {
    let mut stmt =
        conn.prepare("SELECT name, cover_path FROM collection_covers WHERE kind = ?1")?;
    let rows = stmt.query_map(params![kind], |row| {
        let name: String = row.get(0)?;
        let cover_path: String = row.get(1)?;
        Ok((name, PathBuf::from(cover_path)))
    })?;
    rows.collect::<rusqlite::Result<HashMap<_, _>>>()
        .context("listing collection covers")
}

