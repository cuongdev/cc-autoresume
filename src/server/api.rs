use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::json;
use crate::{account, config::Config, pending, stats, server::AppState};
use chrono::Utc;
use crate::{resume, scheduler};

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

#[derive(serde::Deserialize)]
pub struct ModeBody { pub value: String }
pub async fn set_mode(State(s): State<AppState>, Json(b): Json<ModeBody>) -> impl IntoResponse {
    if !["auto","ask","off"].contains(&b.value.as_str()) { return StatusCode::BAD_REQUEST; }
    let mut cfg = Config::load(&s.config_path());
    cfg.mode = b.value;
    let _ = cfg.save(&s.config_path());
    StatusCode::OK
}

#[derive(serde::Deserialize)]
pub struct MsgBody { pub text: String, #[serde(default)] pub cwd: Option<String> }
pub async fn set_message(State(s): State<AppState>, Json(b): Json<MsgBody>) -> impl IntoResponse {
    let mut cfg = Config::load(&s.config_path());
    match b.cwd {
        Some(c) => { cfg.per_project.entry(c).or_default().message = Some(b.text); }
        None => { cfg.default_message = b.text; }
    }
    let _ = cfg.save(&s.config_path());
    StatusCode::OK
}

#[derive(serde::Deserialize)]
pub struct FhBody { pub value: bool }
pub async fn set_force_headless(State(s): State<AppState>, Json(b): Json<FhBody>) -> impl IntoResponse {
    let mut cfg = Config::load(&s.config_path());
    cfg.force_headless = b.value;
    let _ = cfg.save(&s.config_path());
    StatusCode::OK
}

#[derive(serde::Deserialize)]
pub struct TextBody { pub text: String }
pub async fn set_session_message(State(s): State<AppState>, axum::extract::Path(id): axum::extract::Path<String>, Json(b): Json<TextBody>) -> impl IntoResponse {
    match pending::read(&s.pending_dir(), &id) {
        Some(mut r) => { r.message = b.text; let _ = pending::write(&s.pending_dir(), &r); StatusCode::OK }
        None => StatusCode::NOT_FOUND,
    }
}

pub async fn cancel_pending(State(s): State<AppState>, axum::extract::Path(id): axum::extract::Path<String>) -> impl IntoResponse {
    if pending::cancel(&s.pending_dir(), &id) { StatusCode::OK } else { StatusCode::NOT_FOUND }
}

pub async fn arm_pending(State(s): State<AppState>, axum::extract::Path(id): axum::extract::Path<String>) -> impl IntoResponse {
    match pending::read(&s.pending_dir(), &id) {
        Some(mut r) => {
            r.confirmed = true; r.cancelled = false;
            let _ = pending::write(&s.pending_dir(), &r);
            scheduler::pmset_wake(r.fire_at, s.runner.as_ref());
            StatusCode::OK
        }
        None => StatusCode::NOT_FOUND,
    }
}

pub async fn fire_pending(State(s): State<AppState>, axum::extract::Path(id): axum::extract::Path<String>) -> impl IntoResponse {
    if pending::read(&s.pending_dir(), &id).is_none() { return StatusCode::NOT_FOUND; }
    let cfg = Config::load(&s.config_path());
    let now = Utc::now().timestamp();
    let _ = resume::fire(&s.pending_dir(), &id, &cfg, now, s.runner.as_ref(), &scheduler::which_path, "claude");
    StatusCode::OK
}

pub async fn open_terminal(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }

#[derive(serde::Serialize)]
pub struct TokenResp { pub token: String }
pub async fn rotate_token(State(s): State<AppState>) -> impl IntoResponse {
    let mut cfg = Config::load(&s.config_path());
    cfg.token = String::new();
    cfg.ensure_token();
    let _ = cfg.save(&s.config_path());
    Json(TokenResp { token: cfg.token })
}

#[cfg(test)]
mod tests {
    use crate::server::{build_router, test_state};
    use crate::config::Config;
    use crate::pending::{self, Pending};
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

    fn seed(base: &std::path::Path) {
        let dir = base.join("pending");
        let rec = Pending { session_id: "deadbeef".into(), cwd: Some("/x".into()), transcript_path: "/t".into(),
            reset_str: "4pm".into(), fire_at: 9_999_999_999, message: "m".into(), armed_at: 0,
            cancelled: false, confirmed: false, attempts: 0 };
        pending::write(&dir, &rec).unwrap();
    }

    #[tokio::test]
    async fn set_mode_persists() {
        let (base, home, t) = setup();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().method("POST").uri("/api/mode")
            .header("authorization", format!("Bearer {t}")).header("content-type","application/json")
            .body(Body::from(r#"{"value":"off"}"#)).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(Config::load(&base.path().join("config.json")).mode, "off");
    }

    #[tokio::test]
    async fn set_mode_rejects_bad() {
        let (base, home, t) = setup();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().method("POST").uri("/api/mode")
            .header("authorization", format!("Bearer {t}")).header("content-type","application/json")
            .body(Body::from(r#"{"value":"bogus"}"#)).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn cancel_and_arm_and_session_message() {
        let (base, home, t) = setup();
        seed(base.path());
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.clone().oneshot(Request::builder().method("POST").uri("/api/pending/deadbeef/message")
            .header("authorization", format!("Bearer {t}")).header("content-type","application/json")
            .body(Body::from(r#"{"text":"do X"}"#)).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(pending::read(&base.path().join("pending"), "deadbeef").unwrap().message, "do X");
        let res = app.clone().oneshot(Request::builder().method("POST").uri("/api/pending/deadbeef/arm")
            .header("authorization", format!("Bearer {t}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(pending::read(&base.path().join("pending"), "deadbeef").unwrap().confirmed);
        let res = app.oneshot(Request::builder().method("POST").uri("/api/pending/nope/cancel")
            .header("authorization", format!("Bearer {t}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn rotate_changes_token() {
        let (base, home, t) = setup();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.clone().oneshot(Request::builder().method("POST").uri("/api/token/rotate")
            .header("authorization", format!("Bearer {t}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let res = app.oneshot(Request::builder().uri("/api/state")
            .header("authorization", format!("Bearer {t}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
}
