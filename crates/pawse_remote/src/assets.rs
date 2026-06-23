use axum::{
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct WebAssets;

pub async fn static_handler(uri: Uri) -> Response {
    let trimmed = uri.path().trim_start_matches('/');
    let path = if trimmed.is_empty() {
        "index.html"
    } else {
        trimmed
    };

    match WebAssets::get(path) {
        Some(file) => serve_file(path, file.data.into_owned()),
        None => match WebAssets::get("index.html") {
            Some(index) => serve_file("index.html", index.data.into_owned()),
            None => (StatusCode::NOT_FOUND, "not found").into_response(),
        },
    }
}

fn serve_file(path: &str, data: Vec<u8>) -> Response {
    ([(header::CONTENT_TYPE, content_type_for(path))], data).into_response()
}

fn content_type_for(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        _ => "application/octet-stream",
    }
}
