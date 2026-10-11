//! Das gebaute Frontend (web/dist) wird ins Binary eingebettet.

use axum::{
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web/dist/"]
struct Assets;

pub async fn handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    if let Some(res) = asset(path) {
        return res;
    }
    // SPA-Fallback: unbekannte Pfade liefern index.html
    asset("index.html").unwrap_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            "Frontend nicht gebaut. Bitte `npm run build` in web/ ausführen.",
        )
            .into_response()
    })
}

fn asset(path: &str) -> Option<Response> {
    if path.is_empty() {
        return asset("index.html");
    }
    let file = Assets::get(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    // Vite-Assets haben einen Hash im Namen und ändern sich nie
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Some(
        (
            [
                (header::CONTENT_TYPE, mime.as_ref()),
                (header::CACHE_CONTROL, cache),
            ],
            file.data,
        )
            .into_response(),
    )
}
