use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::{gio, glib};
use soup::prelude::*;

/// The `gnosis-reader:` scheme this module registers, serving two things:
///
/// - `gnosis-reader:///shell/...` — the bundled reader UI (`reader.html`,
///   `reader.js`, and the vendored `foliate-js/` modules), embedded into the
///   binary at compile time so the app doesn't depend on the source tree's
///   layout at runtime.
/// - `gnosis-reader:///book/<id>` — the currently-open book's raw file
///   bytes, streamed straight from disk. foliate-js's own `fetch()` +
///   bundled zip reader do the unzipping; nothing here parses the EPUB.
///   `<id>` is only there so each book gets a distinct URL — `fetch()`
///   caches by URL, so reusing one fixed URL for every book would keep
///   serving the first book's cached bytes forever; the actual bytes always
///   come from `current_book`, not from anything encoded in `<id>`.
///   Honors `Range` request headers (serving `206 Partial Content`): a
///   multi-hundred-MB/GB EPUB (large fixed-layout comics in particular) is
///   otherwise pulled into a single in-memory `Blob` before any zip parsing
///   can even start (see `reader.js`'s `openBookOverRange`, which drives
///   zip.js with a custom range-request-based reader instead of a
///   whole-file fetch — specifically so this matters).
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
    asset!("foliate-js/vendor/zip.js"),
    asset!("foliate-js/vendor/fflate.js"),
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

/// Registers the scheme on `context`. `current_book` is shared with the
/// reader page: it sets the path before opening a book, and this handler
/// reads it back on each request to `/book/current`.
pub fn register(context: &webkit6::WebContext, current_book: Rc<RefCell<Option<PathBuf>>>) {
    context.register_uri_scheme(SCHEME, move |request| {
        let path = request.path().map(|p| p.to_string()).unwrap_or_default();
        let trimmed = path.trim_start_matches('/');

        if let Some(asset_path) = trimmed.strip_prefix("shell/") {
            match ASSETS.iter().find(|(name, _)| *name == asset_path) {
                Some((name, bytes)) => {
                    let stream =
                        gio::MemoryInputStream::from_bytes(&glib::Bytes::from_static(bytes));
                    request.finish(&stream, bytes.len() as i64, Some(mime_for(name)));
                }
                None => finish_not_found(request),
            }
            return;
        }

        if trimmed.starts_with("book/") {
            let Some(book_path) = current_book.borrow().clone() else {
                finish_not_found(request);
                return;
            };
            let Ok(file_size) = std::fs::metadata(&book_path).map(|m| m.len()) else {
                finish_not_found(request);
                return;
            };
            let file_size = file_size as i64;

            let range = request
                .http_headers()
                .and_then(|headers| headers.one("Range"))
                .and_then(|value| parse_range(&value, file_size));

            let headers = soup::MessageHeaders::new(soup::MessageHeadersType::Response);
            headers.append("Accept-Ranges", "bytes");

            // A `finish_with_response` stream is read to its own real EOF
            // regardless of the `stream_length` passed to
            // `URISchemeResponse::new` — WebKit doesn't truncate a live,
            // merely-seeked file stream at that length for custom schemes,
            // so a Range request would otherwise still get everything from
            // `start` through the true end of the file. Reading exactly the
            // requested slice into memory first (same idiom already used
            // for the `/shell/` assets below) sidesteps that: a
            // `MemoryInputStream` genuinely EOFs where the buffer ends.
            let response = match range {
                Some((start, end)) => {
                    let length = (end - start + 1) as usize;
                    let Ok(bytes) = read_range(&book_path, start as u64, length) else {
                        finish_not_found(request);
                        return;
                    };
                    let stream =
                        gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes));
                    let response = webkit6::URISchemeResponse::new(&stream, length as i64);
                    response.set_status(206, Some("Partial Content"));
                    headers.set_content_range(start, end, file_size);
                    response
                }
                None => {
                    let gfile = gio::File::for_path(&book_path);
                    let Ok(stream) = gfile.read(gio::Cancellable::NONE) else {
                        finish_not_found(request);
                        return;
                    };
                    webkit6::URISchemeResponse::new(&stream, file_size)
                }
            };
            response.set_content_type("application/epub+zip");
            response.set_http_headers(headers);
            request.finish_with_response(&response);
            return;
        }

        finish_not_found(request);
    });
}

/// Parses an HTTP `Range` request header (`bytes=start-end`, `bytes=start-`,
/// or the suffix form `bytes=-N`) into an inclusive `(start, end)` byte
/// range. Only the first range of a multi-range request is honored (zip
/// readers only ever ask for one at a time); anything unparseable or out of
/// bounds falls back to `None` (a full, non-partial response).
fn parse_range(value: &str, file_size: i64) -> Option<(i64, i64)> {
    let spec = value.strip_prefix("bytes=")?;
    let spec = spec.split(',').next()?.trim();
    let (start_str, end_str) = spec.split_once('-')?;

    if start_str.is_empty() {
        let suffix_len: i64 = end_str.parse().ok()?;
        let start = (file_size - suffix_len).max(0);
        return (start < file_size).then_some((start, file_size - 1));
    }

    let start: i64 = start_str.parse().ok()?;
    let end = if end_str.is_empty() {
        file_size - 1
    } else {
        end_str.parse::<i64>().ok()?.min(file_size - 1)
    };

    (start <= end && start < file_size).then_some((start, end))
}

fn read_range(path: &std::path::Path, start: u64, length: usize) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(start))?;
    let mut buf = vec![0u8; length];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

fn finish_not_found(request: &webkit6::URISchemeRequest) {
    let mut error = glib::Error::new(gio::IOErrorEnum::NotFound, "Not found");
    request.finish_error(&mut error);
}

