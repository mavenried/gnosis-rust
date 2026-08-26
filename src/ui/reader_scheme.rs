use gtk::{gio, glib};

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

pub fn register(context: &webkit6::WebContext) {
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

        finish_not_found(request);
    });
}

fn finish_not_found(request: &webkit6::URISchemeRequest) {
    let mut error = glib::Error::new(gio::IOErrorEnum::NotFound, "Not found");
    request.finish_error(&mut error);
}
