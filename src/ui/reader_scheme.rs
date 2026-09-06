use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use gtk::{gio, glib};
use uuid::Uuid;

use crate::library::db::book_cache_dir;
use crate::library::epub_reader::EpubReader;

pub const SCHEME: &str = "gnosis-reader";

macro_rules! asset {
    ($path:literal) => {
        (
            $path,
            include_bytes!(concat!("../../assets/", $path)).as_slice(),
        )
    };
}

const ASSETS: &[(&str, &[u8])] = &[
    asset!("reader.html"),
    asset!("reader.js"),
];

static ACTIVE_READERS: Mutex<Option<HashMap<Uuid, Arc<Mutex<EpubReader>>>>> = Mutex::new(None);

pub fn set_active_reader(reader: EpubReader) -> Arc<Mutex<EpubReader>> {
    let mut lock = ACTIVE_READERS.lock().unwrap();
    let map = lock.get_or_insert_with(HashMap::new);
    let id = reader.id;
    let arc = Arc::new(Mutex::new(reader));
    map.insert(id, arc.clone());
    arc
}

pub fn get_active_reader(id: Uuid) -> Option<Arc<Mutex<EpubReader>>> {
    let mut lock = ACTIVE_READERS.lock().unwrap();
    let map = lock.get_or_insert_with(HashMap::new);
    map.get(&id).cloned()
}

fn mime_for(path: &str) -> &'static str {
    if path.ends_with(".html") {
        "text/html"
    } else if path.ends_with(".js") {
        "text/javascript"
    } else if path.ends_with(".css") {
        "text/css"
    } else {
        "application/octet-stream"
    }
}

fn guess_book_mime(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "xhtml" | "html" | "htm" => "application/xhtml+xml",
        "opf" => "application/oebps-package+xml",
        "ncx" => "application/x-dtbncx+xml",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "otf" => "font/otf",
        "ttf" => "font/ttf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "xml" => "application/xml",
        "json" => "application/json",
        "mp3" => "audio/mpeg",
        "mp4" | "m4a" | "m4v" => "video/mp4",
        _ => "application/octet-stream",
    }
}

fn percent_decode(s: &str) -> String {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16) {
                result.push(b);
                i += 3;
                continue;
            }
        } else if bytes[i] == b'+' {
            result.push(b' ');
            i += 1;
            continue;
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).to_string()
}

pub fn register(context: &webkit6::WebContext) {
    context.register_uri_scheme(SCHEME, move |request| {
        let uri = request.uri().map(|u| u.to_string()).unwrap_or_default();
        let path = request.path().map(|p| p.to_string()).unwrap_or_default();
        let trimmed = path.trim_start_matches('/');
        tracing::info!(uri, path, "reader_scheme request");

        if let Some(asset_path) = trimmed.strip_prefix("shell/") {
            match ASSETS.iter().find(|(name, _)| *name == asset_path) {
                Some((name, bytes)) => {
                    tracing::info!(asset_path, "reader_scheme: serving shell asset");
                    let stream =
                        gio::MemoryInputStream::from_bytes(&glib::Bytes::from_static(bytes));
                    request.finish(&stream, bytes.len() as i64, Some(mime_for(name)));
                }
                None => {
                    tracing::info!(asset_path, "reader_scheme: shell asset not found");
                    finish_not_found(request);
                }
            }
            return;
        }

        if let Some(id_str) = trimmed.strip_prefix("book-info/") {
            let id = id_str.trim_matches('/').parse::<Uuid>().unwrap_or_default();
            if let Some(reader_arc) = get_active_reader(id) {
                let info = reader_arc.lock().unwrap().book_info();
                if let Ok(bytes) = serde_json::to_vec(&info) {
                    let len = bytes.len() as i64;
                    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes));
                    request.finish(&stream, len, Some("application/json"));
                    return;
                }
            }
            finish_not_found(request);
            return;
        }

        if let Some(rest) = trimmed.strip_prefix("chapter/") {
            let Some((id_str, rest_idx)) = rest.split_once('/') else {
                finish_not_found(request);
                return;
            };
            let id = id_str.parse::<Uuid>().unwrap_or_default();
            let index_str = match rest_idx.split_once('#') {
                Some((idx, _)) => idx,
                None => rest_idx,
            };
            let index = index_str.parse::<usize>().unwrap_or(0);

            if let Some(reader_arc) = get_active_reader(id) {
                let prefs = crate::library::reader_prefs::load_for(id);
                match reader_arc.lock().unwrap().get_injected_chapter(index, &prefs) {
                    Ok((html, mime)) => {
                        let bytes = html.into_bytes();
                        let len = bytes.len() as i64;
                        let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes));
                        request.finish(&stream, len, Some(&mime));
                        return;
                    }
                    Err(err) => {
                        tracing::error!(%err, "reader_scheme: get_injected_chapter error");
                        finish_not_found(request);
                        return;
                    }
                }
            }
            finish_not_found(request);
            return;
        }

        if let Some(rest) = trimmed.strip_prefix("book-search/") {
            let id_str = match rest.split_once('?') {
                Some((id_part, _)) => id_part,
                None => rest,
            };
            let id = id_str.trim_matches('/').parse::<Uuid>().unwrap_or_default();

            let mut query = String::new();
            let mut chapter_filter = None;

            if let Some((_, q_str)) = uri.split_once('?') {
                for pair in q_str.split('&') {
                    if let Some((k, v)) = pair.split_once('=') {
                        if k == "q" {
                            query = percent_decode(v);
                        } else if k == "chapter" {
                            chapter_filter = percent_decode(v).parse::<usize>().ok();
                        }
                    }
                }
            }

            if let Some(reader_arc) = get_active_reader(id) {
                let matches = reader_arc.lock().unwrap().search(&query, chapter_filter);
                if let Ok(bytes) = serde_json::to_vec(&matches) {
                    let len = bytes.len() as i64;
                    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes));
                    request.finish(&stream, len, Some("application/json"));
                    return;
                }
            }
            finish_not_found(request);
            return;
        }

        if let Some(id) = trimmed.strip_prefix("book-manifest/") {
            let manifest_path = book_cache_dir().join(format!("{id}.json"));
            tracing::info!(id, path = %manifest_path.display(), "reader_scheme: manifest request");
            match std::fs::read(&manifest_path) {
                Ok(bytes) => {
                    tracing::info!(len = bytes.len(), "reader_scheme: manifest served");
                    let len = bytes.len() as i64;
                    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes));
                    request.finish(&stream, len, Some("application/json"));
                }
                Err(err) => {
                    tracing::info!(%err, "reader_scheme: manifest read failed");
                    finish_not_found(request);
                }
            }
            return;
        }

        if let Some(rest) = trimmed.strip_prefix("book/") {
            let Some((id, rel_path)) = rest.split_once('/') else {
                tracing::info!(rest, "reader_scheme: malformed book/ request (no id/path split)");
                finish_not_found(request);
                return;
            };
            if rel_path
                .split('/')
                .any(|segment| segment == ".." || segment.is_empty())
            {
                tracing::info!(rel_path, "reader_scheme: rejected book/ path (traversal guard)");
                finish_not_found(request);
                return;
            }
            let full_path = book_cache_dir().join(id).join(rel_path);
            tracing::info!(id, rel_path, full_path = %full_path.display(), "reader_scheme: book file request");
            match std::fs::read(&full_path) {
                Ok(bytes) => {
                    tracing::info!(len = bytes.len(), "reader_scheme: book file served");
                    let len = bytes.len() as i64;
                    let mime = guess_book_mime(&full_path);
                    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes));
                    request.finish(&stream, len, Some(mime));
                }
                Err(err) => {
                    tracing::info!(%err, "reader_scheme: book file read failed");
                    finish_not_found(request);
                }
            }
            return;
        }

        tracing::info!(path, "reader_scheme: unmatched request");
        finish_not_found(request);
    });
}

fn finish_not_found(request: &webkit6::URISchemeRequest) {
    let mut error = glib::Error::new(gio::IOErrorEnum::NotFound, "Not found");
    request.finish_error(&mut error);
}
