use gtk::{gio, glib};

use crate::library::db::book_cache_dir;

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
    asset!("foliate-js/view.js"),
    asset!("foliate-js/epub.js"),
    asset!("foliate-js/fixed-layout.js"),
    asset!("foliate-js/paginator.js"),
    asset!("foliate-js/epubcfi.js"),
    asset!("foliate-js/progress.js"),
    asset!("foliate-js/overlayer.js"),
    asset!("foliate-js/text-walker.js"),
    asset!("foliate-js/search.js"),
];

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
