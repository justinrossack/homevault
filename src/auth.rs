//! Login mit Benutzername + Passwort. Sessions liegen in SQLite und
//! überstehen damit Neustarts. Gespeichert wird nur ein Hash des Cookies.

use std::time::Duration;

use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use rand::RngCore;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::json;

use crate::{AppState, db, error::AppError, users};

const COOKIE: &str = "hv_session";
const SESSION_TTL: Duration = Duration::from_secs(60 * 60 * 24 * 30);

/// Angemeldeter Benutzer, wird von `require_auth` in den Request gelegt.
#[derive(Clone)]
pub struct CurrentUser {
    pub id: i64,
    pub name: String,
    pub admin: bool,
    pub token: String,
}

fn token_hash(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

pub fn create_session(conn: &Connection, user_id: i64) -> rusqlite::Result<String> {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    conn.execute("DELETE FROM sessions WHERE expires_at < ?", [db::now()])?;
    conn.execute(
        "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?, ?, ?)",
        params![token_hash(&token), user_id, db::now() + SESSION_TTL.as_secs() as i64],
    )?;
    Ok(token)
}

/// Meldet einen Benutzer überall ab, optional bis auf eine Sitzung.
pub fn delete_user_sessions(conn: &Connection, user_id: i64, keep_token: Option<&str>) -> rusqlite::Result<()> {
    let keep = keep_token.map(token_hash).unwrap_or_default();
    conn.execute(
        "DELETE FROM sessions WHERE user_id = ? AND token_hash != ?",
        params![user_id, keep],
    )?;
    Ok(())
}

fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|c| c.trim().strip_prefix(COOKIE)?.strip_prefix('='))
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

fn cookie(value: &str, max_age: u64) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{COOKIE}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}"
    ))
    .unwrap()
}

pub fn session_cookie(token: &str) -> HeaderValue {
    cookie(token, SESSION_TTL.as_secs())
}

async fn current_user(state: &AppState, headers: &HeaderMap) -> Result<Option<CurrentUser>, AppError> {
    let Some(token) = session_token(headers) else {
        return Ok(None);
    };
    state
        .db
        .call(move |c| {
            Ok(c.query_row(
                "SELECT u.id, u.name, u.admin FROM sessions s JOIN users u ON u.id = s.user_id
                 WHERE s.token_hash = ? AND s.expires_at > ?",
                params![token_hash(&token), db::now()],
                |r| Ok(CurrentUser { id: r.get(0)?, name: r.get(1)?, admin: r.get(2)?, token: token.clone() }),
            )
            .optional()?)
        })
        .await
}

#[derive(Deserialize)]
pub struct LoginBody {
    username: String,
    password: String,
}

pub async fn login(State(state): State<AppState>, Json(body): Json<LoginBody>) -> Result<Response, AppError> {
    let Some(user) = users::verify(&state.db, &body.username, &body.password).await? else {
        // bremst Passwort-Raten aus
        tokio::time::sleep(Duration::from_millis(500)).await;
        return Err(AppError::BadRequest("Benutzername oder Passwort falsch".into()));
    };
    let token = state.db.call(move |c| Ok(create_session(c, user.id)?)).await?;
    let mut res = Json(json!({ "ok": true })).into_response();
    res.headers_mut().insert(header::SET_COOKIE, session_cookie(&token));
    Ok(res)
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, AppError> {
    if let Some(t) = session_token(&headers) {
        state
            .db
            .call(move |c| Ok(c.execute("DELETE FROM sessions WHERE token_hash = ?", [token_hash(&t)])?))
            .await?;
    }
    let mut res = Json(json!({ "ok": true })).into_response();
    res.headers_mut().insert(header::SET_COOKIE, cookie("", 0));
    Ok(res)
}

/// Status für das Frontend: angemeldet? Einrichtung nötig? Servername.
pub async fn me(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<serde_json::Value>, AppError> {
    let user = current_user(&state, &headers).await?;
    let (setup_needed, server_name) = state
        .db
        .call(|c| {
            Ok((
                users::user_count(c)? == 0,
                db::get_setting(c, "server_name")?.unwrap_or_else(|| users::DEFAULT_SERVER_NAME.into()),
            ))
        })
        .await?;
    Ok(Json(json!({
        "authenticated": user.is_some(),
        "user": user.map(|u| json!({ "name": u.name, "admin": u.admin })),
        "setup_needed": setup_needed,
        "server_name": server_name,
    })))
}

pub async fn require_auth(State(state): State<AppState>, mut req: Request, next: Next) -> Result<Response, AppError> {
    let user = current_user(&state, req.headers()).await?.ok_or(AppError::Unauthorized)?;
    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}
