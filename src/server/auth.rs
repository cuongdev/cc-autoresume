use axum::{
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use crate::{config::Config, server::AppState};

/// Constant-time string compare.
fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() { return false; }
    let mut diff = 0u8;
    for i in 0..a.len() { diff |= a[i] ^ b[i]; }
    diff == 0
}

/// Extract the presented token from `Authorization: Bearer X` or `?token=X`.
fn presented(req: &Request<axum::body::Body>) -> Option<String> {
    if let Some(h) = req.headers().get(axum::http::header::AUTHORIZATION) {
        if let Ok(s) = h.to_str() {
            if let Some(t) = s.strip_prefix("Bearer ") { return Some(t.to_string()); }
        }
    }
    req.uri().query().and_then(|q| {
        q.split('&').find_map(|kv| kv.strip_prefix("token=").map(|t| t.to_string()))
    })
}

pub async fn require_token(State(s): State<AppState>, req: Request<axum::body::Body>, next: Next) -> Response {
    let cfg = Config::load(&s.config_path());
    let ok = !cfg.token.is_empty()
        && presented(&req).map(|t| ct_eq(&t, &cfg.token)).unwrap_or(false);
    if ok { next.run(req).await } else { (StatusCode::UNAUTHORIZED, "unauthorized").into_response() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ct_eq_works() {
        assert!(ct_eq("abc", "abc"));
        assert!(!ct_eq("abc", "abd"));
        assert!(!ct_eq("abc", "ab"));
    }
}
