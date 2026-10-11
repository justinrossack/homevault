use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict(String),
    Gone(String),
    /// Dateityp wird (auf diesem Server) nicht unterstützt
    Unsupported(String),
    /// vorübergehend nicht möglich (Wartung)
    Unavailable(String),
    Internal(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::BadRequest(m) | AppError::Conflict(m) | AppError::Gone(m) | AppError::Unsupported(m) | AppError::Unavailable(m) | AppError::Internal(m) => f.write_str(m),
            AppError::Unauthorized => f.write_str("Nicht angemeldet"),
            AppError::Forbidden => f.write_str("Keine Berechtigung"),
            AppError::NotFound => f.write_str("Nicht gefunden"),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => AppError::NotFound,
            std::io::ErrorKind::AlreadyExists => AppError::Conflict("Existiert bereits".into()),
            std::io::ErrorKind::DirectoryNotEmpty => {
                AppError::Conflict("Ordner ist nicht leer".into())
            }
            _ => AppError::Internal(e.to_string()),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "Nicht angemeldet".into()),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "Keine Berechtigung".into()),
            AppError::NotFound => (StatusCode::NOT_FOUND, "Nicht gefunden".into()),
            AppError::Conflict(m) => (StatusCode::CONFLICT, m),
            AppError::Gone(m) => (StatusCode::GONE, m),
            AppError::Unsupported(m) => (StatusCode::UNSUPPORTED_MEDIA_TYPE, m),
            AppError::Unavailable(m) => (StatusCode::SERVICE_UNAVAILABLE, m),
            AppError::Internal(m) => {
                tracing::error!("{m}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Interner Fehler".into())
            }
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}
