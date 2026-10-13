//! The reader page and foliate-js, served to WebKit as `omaread://app/…`.
//!
//! In debug builds rust-embed reads straight from `web/`, so editing the page
//! needs only a reload; release builds carry the files in the binary.

use gtk::{gio, glib};
use rust_embed::RustEmbed;

pub const SCHEME: &str = "omaread";
pub const READER_URI: &str = "omaread://app/app/reader.html";

#[derive(RustEmbed)]
#[folder = "../../web/"]
#[include = "app/*"]
#[include = "foliate-js/*.js"]
#[include = "foliate-js/vendor/*.js"]
struct Web;

pub fn register(context: &webkit::WebContext) {
    if let Some(security) = context.security_manager() {
        security.register_uri_scheme_as_secure(SCHEME);
        security.register_uri_scheme_as_cors_enabled(SCHEME);
    }
    context.register_uri_scheme(SCHEME, |request| {
        let path = request.path().map(|p| p.to_string()).unwrap_or_default();
        match Web::get(path.trim_start_matches('/')) {
            Some(file) => {
                let bytes = glib::Bytes::from_owned(file.data.into_owned());
                let len = bytes.len() as i64;
                let stream = gio::MemoryInputStream::from_bytes(&bytes);
                request.finish(&stream, len, Some(mime_of(&path)));
            }
            None => {
                let mut error = glib::Error::new(gio::IOErrorEnum::NotFound, &format!("not found: {path}"));
                request.finish_error(&mut error);
            }
        }
    });
}

fn mime_of(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("js" | "mjs") => "text/javascript",
        Some("html") => "text/html",
        Some("css") => "text/css",
        Some("svg") => "image/svg+xml",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}
