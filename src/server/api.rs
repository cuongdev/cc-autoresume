use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::json;
use crate::{account, config::Config, pending, stats, server::AppState};
use chrono::Utc;

pub async fn get_state(State(s): State<AppState>) -> impl IntoResponse {
    let now = Utc::now().timestamp();
    let cfg = Config::load(&s.config_path());
    let st = stats::Stats::load(&s.stats_path());
    let pend = pending::list_all(&s.pending_dir());
    let next_reset = pend.iter()
        .filter(|r| !r.cancelled && r.confirmed && r.fire_at > now)
        .map(|r| r.fire_at).min();
    let status = s.status.read().await.clone();
    Json(json!({
        "account": account::read(&s.home),
        "mode": cfg.mode,
        "forceHeadless": cfg.force_headless,
        "defaultMessage": cfg.default_message,
        "perProject": cfg.per_project,
        "watcher": status,
        "stats": {
            "limitHits7d": st.hits_7d(now),
            "autoResumes": st.auto_resumes,
            "sessions7d": stats::sessions_7d(&s.home, now),
            "nextResetEpoch": next_reset,
        },
        "pending": pend,
        "recent": st.recent,
    }))
}

// ---- stubs filled in later tasks ----
pub async fn set_mode(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
pub async fn set_message(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
pub async fn set_force_headless(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
pub async fn set_session_message(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
pub async fn cancel_pending(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
pub async fn arm_pending(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
pub async fn fire_pending(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
pub async fn open_terminal(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
pub async fn rotate_token(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }

#[cfg(test)]
mod tests {
    use crate::server::{build_router, test_state};
    use crate::config::Config;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn setup() -> (tempfile::TempDir, tempfile::TempDir, String) {
        let base = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let mut cfg = Config::default(); cfg.token = "secrettoken".into();
        cfg.save(&base.path().join("config.json")).unwrap();
        (base, home, "secrettoken".to_string())
    }

    #[tokio::test]
    async fn state_requires_token() {
        let (base, home, _t) = setup();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri("/api/state").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn state_ok_with_token() {
        let (base, home, t) = setup();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri("/api/state")
            .header("authorization", format!("Bearer {t}"))
            .body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["mode"], "auto");
        assert!(v["stats"]["autoResumes"].is_number());
        assert!(v["pending"].is_array());
    }

    #[tokio::test]
    async fn state_ok_with_query_token() {
        let (base, home, t) = setup();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri(format!("/api/state?token={t}"))
            .body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn healthz_open() {
        let (base, home, _t) = setup();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri("/healthz").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }
}
