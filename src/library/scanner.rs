use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use epub::doc::EpubDoc;
use gdk_pixbuf::prelude::*;
use gdk_pixbuf::{InterpType, Pixbuf, PixbufLoader};
use uuid::Uuid;

use super::db::covers_dir;
use super::models::Book;

/// Cache cover art at most this many pixels wide, same as Foliate's default
/// `cover-size` setting — large enough to look sharp at the ~120px grid card
/// width on HiDPI displays, small enough to avoid caching multi-megabyte
/// cover images verbatim.
const MAX_COVER_WIDTH: i32 = 256;

/// Resolves symlinks and relative segments so the same on-disk file is
/// always represented by the same path, regardless of how it was reached
/// (picked directly vs. found via a folder scan, a symlinked directory,
/// `..` segments, etc.) — this is what path-based duplicate/existence
/// checks rely on. Falls back to the original path if it can't be resolved
/// (e.g. it doesn't exist), so callers still get a sensible error later.
pub fn canonicalize_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Recursively finds every `.epub` file under `root`.
pub fn find_epubs(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];

    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("epub"))
            {
                found.push(path);
            }
        }
    }

    found
}

pub fn scan_epub(path: &Path) -> Result<Book> {
    let mut doc = EpubDoc::new(path)
        .map_err(|e| anyhow!("{e}"))
        .with_context(|| format!("opening epub {}", path.display()))?;

    let id = Uuid::new_v4();

    let title = doc.get_title().unwrap_or_else(|| fallback_title(path));

    let author = doc.mdata("creator").map(|item| item.value.clone());
    let (series, series_index) = parse_series(&doc);

    let cover_path = match doc.get_cover().or_else(|| legacy_cover(&mut doc)) {
        Some((bytes, mime)) => Some(save_cover(id, &bytes, &mime)?),
        None => None,
    };

    Ok(Book {
        id,
        title,
        author,
        series,
        series_index,
        path: path.to_path_buf(),
        format: "epub".to_string(),
        cover_path,
        added_at: Book::now(),
        progress: 0.0,
        locator: None,
    })
}

/// EPUBs express series membership two different ways in the wild: EPUB3's
/// `belongs-to-collection` meta (book number in its `group-position`
/// refinement), and the much more common Calibre convention of plain
/// `calibre:series`/`calibre:series_index` `<meta name content>` pairs.
/// Prefers whichever is present, EPUB3 first.
fn parse_series<R: std::io::Read + std::io::Seek>(
    doc: &EpubDoc<R>,
) -> (Option<String>, Option<f64>) {
    if let Some(item) = doc
        .metadata
        .iter()
        .find(|item| item.property == "belongs-to-collection" && !item.value.trim().is_empty())
    {
        let index = item
            .refinement("group-position")
            .and_then(|r| r.value.trim().parse::<f64>().ok());
        return (Some(item.value.clone()), index);
    }

    let series = doc
        .mdata("calibre:series")
        .map(|item| item.value.clone())
        .filter(|s| !s.trim().is_empty());
    let index = doc
        .mdata("calibre:series_index")
        .and_then(|item| item.value.trim().parse::<f64>().ok());

    (series, index)
}

/// `EpubDoc::get_cover()` only finds a cover via the manifest item's
/// EPUB3 `properties="cover-image"` attribute for version-3.0 packages —
/// but plenty of real-world EPUB3 files still declare their cover the old
/// EPUB2 way, `<meta name="cover" content="ITEM-ID"/>`, without ever
/// setting `properties` on the matching manifest item. Resolves that
/// legacy declaration by hand when the normal lookup misses.
fn legacy_cover<R: std::io::Read + std::io::Seek>(
    doc: &mut EpubDoc<R>,
) -> Option<(Vec<u8>, String)> {
    let id = doc.mdata("cover")?.value.clone();
    doc.get_resource(&id)
}

fn fallback_title(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Untitled".to_string())
}

fn extension_for_mime(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/gif" => "gif",
        "image/svg+xml" => "svg",
        _ => "jpg",
    }
}

/// Caches `bytes` as a cover image, downscaled to [`MAX_COVER_WIDTH`] (never
/// upscaled) and re-encoded as PNG. Falls back to caching the bytes verbatim
/// if they can't be decoded as an image.
fn save_cover(id: Uuid, bytes: &[u8], mime: &str) -> Result<PathBuf> {
    if let Some(scaled) = scale_cover(bytes) {
        let dest = covers_dir().join(format!("{id}.png"));
        if scaled.savev(&dest, "png", &[]).is_ok() {
            return Ok(dest);
        }
    }

    let ext = extension_for_mime(mime);
    let dest = covers_dir().join(format!("{id}.{ext}"));
    std::fs::write(&dest, bytes).context("writing cover image")?;
    Ok(dest)
}

fn scale_cover(bytes: &[u8]) -> Option<Pixbuf> {
    let loader = PixbufLoader::new();
    loader.write(bytes).ok()?;
    loader.close().ok()?;
    let pixbuf = loader.pixbuf()?;

    if pixbuf.width() <= MAX_COVER_WIDTH {
        return Some(pixbuf);
    }

    let ratio = f64::from(MAX_COVER_WIDTH) / f64::from(pixbuf.width());
    let height = ((f64::from(pixbuf.height()) * ratio).round() as i32).max(1);
    pixbuf.scale_simple(MAX_COVER_WIDTH, height, InterpType::Bilinear)
}
