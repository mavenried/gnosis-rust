use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::{gio, glib};
use soup::prelude::*;

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

